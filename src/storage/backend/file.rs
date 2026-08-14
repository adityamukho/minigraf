/// File-based storage backend for native platforms.
use crate::storage::{FileHeader, PAGE_SIZE, StorageBackend};
use anyhow::Result;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Advisory file lock to prevent multi-process corruption.
///
/// Uses a sidecar `.lock` file with exclusive creation semantics.
/// The lock is released (file deleted) on drop.
struct FileLock {
    path: PathBuf,
}

impl FileLock {
    /// Attempt to acquire an exclusive lock for the given database path.
    /// Returns `Err` if another process already holds the lock.
    ///
    /// If a stale lock file exists (the holder is a *different* process that is
    /// no longer running), it is automatically removed and a new lock is
    /// acquired. This handles the case where the previous process crashed
    /// without cleaning up.
    ///
    /// A lock held by our OWN pid is never treated as stale. It used to be, on
    /// the theory that it could only be a leaked handle — but the overwhelmingly
    /// more likely explanation is a handle that is open and in use right now,
    /// somewhere else in this process. Stealing it produced two live
    /// `FileBackend`s on one file, each caching its own `header.page_count`,
    /// each allocating new pages from that stale count and bounds-checking
    /// `read_page` against it. That is the source of the intermittent
    /// "Page N out of bounds (total pages: M)" in #304: not an allocation
    /// off-by-one, but two page tables diverging. It also corrupted the lock
    /// itself — whichever handle dropped first ran `FileLock::drop` and deleted
    /// the lock file belonging to the survivor, leaving a live handle unlocked
    /// and letting other processes in too.
    ///
    /// The lock content is `<pid>` or, on procfs platforms, `<pid>:<starttime>`
    /// where `starttime` is field 22 of `/proc/<pid>/stat` (clock ticks since
    /// boot). The start time disambiguates "same PID, same live process" from
    /// "same PID number, different process incarnation": under Linux PID
    /// namespaces every container's main process is PID 1, so when a pod dies
    /// and a replacement pod mounts the same volume, the stale lock names the
    /// new process's own PID and a bare-PID check would refuse forever.
    fn acquire(db_path: &Path) -> Result<Self> {
        let lock_path = db_path.with_extension("graph.lock");
        // create_new fails with AlreadyExists if the lock file is present
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
        {
            Ok(mut f) => {
                // Write PID (plus process start time where procfs is available)
                // for diagnostics and stale-lock detection (best-effort)
                match Self::process_start_time(std::process::id()) {
                    Some(start_time) => {
                        let _ = write!(f, "{}:{}", std::process::id(), start_time);
                    }
                    None => {
                        let _ = write!(f, "{}", std::process::id());
                    }
                }
                Ok(FileLock { path: lock_path })
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                // Check if the holder process is still alive
                let holder = std::fs::read_to_string(&lock_path).unwrap_or_default();
                if let Some((pid, start_time)) = Self::parse_lock_content(&holder) {
                    let our_pid = std::process::id();
                    if pid == our_pid {
                        // Same PID number does not imply same process: under PID
                        // namespaces (Kubernetes) a replacement container reuses
                        // PID 1, so a dead predecessor's lock aliases our own
                        // PID. If the recorded start time differs from ours, the
                        // holder is a different, dead incarnation — the lock is
                        // stale. A matching, absent, or unreadable start time
                        // keeps the #304 refusal exactly as-is.
                        let stale = matches!(
                            (start_time, Self::process_start_time(our_pid)),
                            (Some(recorded), Some(ours)) if recorded != ours
                        );
                        if stale {
                            let _ = std::fs::remove_file(&lock_path);
                            return Self::acquire(db_path);
                        }
                        // A handle in THIS process holds the lock. Say so
                        // explicitly: the generic "another process" wording sent
                        // #304 looking for a second process for a long time.
                        anyhow::bail!(
                            "Database is already open in this process (lock file: {}, \
                             holder PID: {} == our own). A second handle on one file \
                             would give each its own page table and corrupt both — \
                             reuse the existing handle instead. `Minigraf` is cheap to \
                             clone and all clones share the same database.",
                            lock_path.display(),
                            pid
                        );
                    }
                    // A live foreign PID can also be a namespace alias for a
                    // dead holder (the previous container's process happened to
                    // have the same in-namespace PID as one of ours). When both
                    // start times are known and differ, the live process is not
                    // the lock's author and the lock is stale.
                    let holder_stale = !Self::is_process_alive(pid)
                        || matches!(
                            (start_time, Self::process_start_time(pid)),
                            (Some(recorded), Some(live)) if recorded != live
                        );
                    if holder_stale {
                        // Stale lock — the holding process died without cleaning
                        // up. Remove and retry.
                        let _ = std::fs::remove_file(&lock_path);
                        return Self::acquire(db_path);
                    }
                }
                anyhow::bail!(
                    "Database is locked by another process (lock file: {}, holder PID: {}). \
                     If no other process is using this database, delete the lock file manually.",
                    lock_path.display(),
                    holder.trim()
                );
            }
            Err(e) => {
                anyhow::bail!(
                    "Failed to acquire database lock at {}: {}",
                    lock_path.display(),
                    e
                );
            }
        }
    }

    /// Check if a process with the given PID is still running.
    fn is_process_alive(pid: u32) -> bool {
        // On Linux/Android, /proc/<pid> exists iff the process is alive.
        let proc_path = format!("/proc/{}", pid);
        if std::path::Path::new(&proc_path).exists() {
            return true;
        }
        // On Linux, if /proc exists but /proc/<pid> doesn't, the process is dead.
        if std::path::Path::new("/proc").exists() {
            return false;
        }
        // On non-procfs systems (macOS, Windows), assume alive (conservative).
        // Users must manually delete stale lock files on these platforms.
        true
    }

    /// Parse lock file content into (PID, optional process start time).
    ///
    /// Accepts both `<pid>:<starttime>` (current) and bare `<pid>` (written by
    /// versions before start times were recorded); anything else is `None`.
    fn parse_lock_content(content: &str) -> Option<(u32, Option<u64>)> {
        let mut parts = content.trim().split(':');
        let pid = parts.next()?.parse::<u32>().ok()?;
        let start_time = match parts.next() {
            Some(s) => Some(s.parse::<u64>().ok()?),
            None => None,
        };
        Some((pid, start_time))
    }

    /// Read a process's start time from `/proc/<pid>/stat` (field 22,
    /// `starttime`, in clock ticks since boot). Two processes in different
    /// PID namespaces may share a PID number, but not a start time.
    ///
    /// Returns `None` on non-procfs platforms and on any read or parse
    /// failure; callers treat `None` as "unknown" and stay conservative.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn process_start_time(pid: u32) -> Option<u64> {
        let stat = std::fs::read_to_string(format!("/proc/{}/stat", pid)).ok()?;
        // The comm field (2) is wrapped in parens and may itself contain
        // spaces or parens, so split at the LAST ')'; what follows is fields
        // 3 onwards, and field 22 is the 20th token of that remainder.
        let after_comm = stat.rsplit_once(')')?.1;
        after_comm.split_whitespace().nth(19)?.parse().ok()
    }

    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    fn process_start_time(_pid: u32) -> Option<u64> {
        None
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// File-based storage backend for native platforms.
///
/// Stores graph data in a single `.graph` file with a page-based structure:
/// - Page 0: File header (metadata)
/// - Page 1+: Data pages (nodes, edges, indexes)
///
/// Supports:
/// - Linux, macOS, Windows (native desktop)
/// - iOS, Android (via FFI)
///
/// File format is cross-platform (endian-safe).
pub struct FileBackend {
    #[allow(dead_code)]
    path: PathBuf,
    file: File,
    header: FileHeader,
    is_new: bool,
    _lock: FileLock,
}

impl FileBackend {
    /// Open or create a .graph file at the given path.
    ///
    /// If the file doesn't exist, creates it with an initial header.
    /// If it exists, validates and loads the header.
    ///
    /// Acquires an advisory file lock (sidecar `.graph.lock` file) to prevent
    /// multi-process corruption. Returns an error if the database is already
    /// opened by another process.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref().to_path_buf();

        // Acquire advisory lock before touching the database file.
        let lock = FileLock::acquire(&path)?;

        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;

        // Check file size using the open file handle's metadata.
        // This is more reliable than checking path metadata separately,
        // as it uses the same file descriptor we'll be reading from.
        let file_len = file.metadata()?.len();

        // Determine if this is an existing file with data or a new/empty one.
        let is_new = file_len < PAGE_SIZE as u64;
        let header = if file_len >= PAGE_SIZE as u64 {
            // File has at least one page - try to read the header
            match Self::read_header(&mut file) {
                Ok(header) => header,
                Err(e) => {
                    // File has content but header is invalid - this is a real error
                    anyhow::bail!(
                        "Failed to read header from existing file (size={}): {}",
                        file_len,
                        e
                    );
                }
            }
        } else {
            // New file or empty file: write initial header
            let header = FileHeader::new();
            Self::write_header(&mut file, &header)?;
            header
        };

        Ok(FileBackend {
            path,
            file,
            header,
            is_new,
            _lock: lock,
        })
    }

    /// Read the file header from page 0.
    fn read_header(file: &mut File) -> Result<FileHeader> {
        file.seek(SeekFrom::Start(0))?;

        let mut header_bytes = vec![0u8; PAGE_SIZE];
        file.read_exact(&mut header_bytes)?;

        let header = FileHeader::from_bytes(&header_bytes)?;
        header.validate()?;

        Ok(header)
    }

    /// Write the file header to page 0.
    fn write_header(file: &mut File, header: &FileHeader) -> Result<()> {
        file.seek(SeekFrom::Start(0))?;

        let header_bytes = header.to_bytes();
        let mut page = vec![0u8; PAGE_SIZE];
        let hlen = header_bytes.len();
        page.get_mut(..hlen)
            .ok_or_else(|| anyhow::anyhow!("header bytes exceed page size"))?
            .copy_from_slice(&header_bytes);

        file.write_all(&page)?;
        file.sync_all()?;

        Ok(())
    }

    /// Get the file path.
    #[allow(dead_code)]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl StorageBackend for FileBackend {
    fn write_page(&mut self, page_id: u64, data: &[u8]) -> Result<()> {
        if data.len() != PAGE_SIZE {
            anyhow::bail!(
                "Invalid page size: {} bytes (expected {})",
                data.len(),
                PAGE_SIZE
            );
        }

        let offset = page_id
            .checked_mul(PAGE_SIZE as u64)
            .ok_or_else(|| anyhow::anyhow!("page offset overflow for page_id {}", page_id))?;
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.write_all(data)?;

        // If writing page 0 (header), update our in-memory header
        if page_id == 0 {
            // Page 0 is the header itself, parse it to update our cached copy
            self.header = FileHeader::from_bytes(data)?;
        } else if page_id >= self.header.page_count {
            // Update page count if this is a new page (but not page 0)
            self.header.page_count = page_id
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("page_count overflow for page_id {}", page_id))?;
            Self::write_header(&mut self.file, &self.header)?;
        }

        Ok(())
    }

    fn read_page(&self, page_id: u64) -> Result<Vec<u8>> {
        if page_id >= self.header.page_count {
            anyhow::bail!(
                "Page {} out of bounds (total pages: {})",
                page_id,
                self.header.page_count
            );
        }

        let offset = page_id
            .checked_mul(PAGE_SIZE as u64)
            .ok_or_else(|| anyhow::anyhow!("page offset overflow for page_id {}", page_id))?;
        let mut file = &self.file;
        file.seek(SeekFrom::Start(offset))?;

        let mut data = vec![0u8; PAGE_SIZE];
        file.read_exact(&mut data)?;

        Ok(data)
    }

    fn sync(&mut self) -> Result<()> {
        self.file.sync_all()?;
        Ok(())
    }

    fn page_count(&self) -> Result<u64> {
        Ok(self.header.page_count)
    }

    fn close(&mut self) -> Result<()> {
        self.sync()
    }

    fn backend_name(&self) -> &'static str {
        "file"
    }

    fn is_new(&self) -> bool {
        self.is_new
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    /// #304: a second handle on a file this process already has open must be
    /// refused, not granted by deleting our own lock file.
    ///
    /// Before the fix `acquire` treated `pid == our_pid` as a stale lock, so
    /// this second open succeeded and the two backends diverged: each caches
    /// its own `header.page_count`, allocates new pages from it, and
    /// bounds-checks `read_page` against it.
    #[test]
    fn test_second_open_in_same_process_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let temp_path = dir.path().join("test_minigraf_self_steal.graph");

        let first = FileBackend::open(&temp_path).unwrap();

        // FileBackend is not Debug, so match on the Result rather than
        // expect_err.
        let msg = match FileBackend::open(&temp_path) {
            Ok(_) => panic!("a second handle in the same process must be refused"),
            Err(e) => e.to_string(),
        };
        assert!(
            msg.contains("already open in this process"),
            "error should name the same-process case, got: {msg}"
        );

        // The refusal must not have removed the lock the first handle holds.
        let lock_path = temp_path.with_extension("graph.lock");
        assert!(
            lock_path.exists(),
            "a refused acquire must leave the existing lock in place"
        );
        drop(first);
        assert!(
            !lock_path.exists(),
            "dropping the only handle should release the lock"
        );
    }

    /// The refusal must not regress the sequential open/drop/reopen cycle that
    /// `temporal_reasoning` relies on between commits.
    #[test]
    fn test_reopen_after_drop_still_works() {
        let dir = tempfile::tempdir().unwrap();
        let temp_path = dir.path().join("test_minigraf_reopen.graph");

        for _ in 0..3 {
            let backend = FileBackend::open(&temp_path).unwrap();
            drop(backend);
        }
        assert!(FileBackend::open(&temp_path).is_ok());
    }

    /// A lock left behind by a *different*, dead process is still stale and
    /// must still be self-healed — that behaviour is what the `pid == our_pid`
    /// branch was overreaching from, and it must survive the fix.
    ///
    /// procfs-only: without `/proc`, `is_process_alive` cannot distinguish a
    /// dead PID from a live one and deliberately errs towards "alive" (see its
    /// own comment), so reclamation is a Linux/Android guarantee only. On other
    /// platforms a stale lock must still be removed by hand.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn test_dead_other_process_lock_is_still_reclaimed() {
        let dir = tempfile::tempdir().unwrap();
        let temp_path = dir.path().join("test_minigraf_stale.graph");
        let lock_path = temp_path.with_extension("graph.lock");

        // PID 1 is always alive; instead spawn a real child and let it exit so
        // we have a genuinely dead PID that is not us.
        let child = std::process::Command::new("true").spawn().unwrap();
        let dead_pid = child.id();
        let mut child = child;
        child.wait().unwrap();

        std::fs::write(&lock_path, dead_pid.to_string()).unwrap();

        let backend = FileBackend::open(&temp_path);
        assert!(
            backend.is_ok(),
            "a dead other process's lock must still be reclaimed: {:?}",
            backend.err()
        );
    }

    /// PID namespaces (Kubernetes) make every container's main process PID 1,
    /// so a replacement pod mounting the same volume finds a stale lock naming
    /// its own PID. A recorded start time that differs from ours identifies
    /// the holder as a different, dead incarnation — the lock must be
    /// reclaimed, not refused forever.
    ///
    /// procfs-only for the same reason as
    /// `test_dead_other_process_lock_is_still_reclaimed`.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn test_namespaced_same_pid_lock_with_wrong_start_time_is_reclaimed() {
        let dir = tempfile::tempdir().unwrap();
        let temp_path = dir.path().join("test_minigraf_pidns_stale.graph");
        let lock_path = temp_path.with_extension("graph.lock");

        let our_pid = std::process::id();
        let our_start = FileLock::process_start_time(our_pid).unwrap();
        std::fs::write(&lock_path, format!("{}:{}", our_pid, our_start + 1)).unwrap();

        let backend = FileBackend::open(&temp_path);
        assert!(
            backend.is_ok(),
            "a lock naming our PID with a different start time is a dead \
             namespaced predecessor and must be reclaimed: {:?}",
            backend.err()
        );
    }

    /// The disambiguation must not weaken #304: a lock naming our PID with
    /// OUR OWN start time is a live handle in this process and must still be
    /// refused.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn test_same_pid_lock_with_our_own_start_time_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let temp_path = dir.path().join("test_minigraf_pidns_live.graph");
        let lock_path = temp_path.with_extension("graph.lock");

        let our_pid = std::process::id();
        let our_start = FileLock::process_start_time(our_pid).unwrap();
        std::fs::write(&lock_path, format!("{}:{}", our_pid, our_start)).unwrap();

        let msg = match FileBackend::open(&temp_path) {
            Ok(_) => panic!("a lock with our PID and our start time must be refused"),
            Err(e) => e.to_string(),
        };
        assert!(
            msg.contains("already open in this process"),
            "error should name the same-process case, got: {msg}"
        );
        assert!(
            lock_path.exists(),
            "a refused acquire must leave the existing lock in place"
        );
    }

    /// Backward compatibility: lock files written before start times were
    /// recorded contain a bare PID. One naming our own PID must still be
    /// refused exactly as before (#304 semantics unchanged for old locks).
    #[test]
    fn test_legacy_bare_pid_lock_with_our_pid_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let temp_path = dir.path().join("test_minigraf_legacy_lock.graph");
        let lock_path = temp_path.with_extension("graph.lock");

        std::fs::write(&lock_path, std::process::id().to_string()).unwrap();

        let msg = match FileBackend::open(&temp_path) {
            Ok(_) => panic!("a legacy bare-PID lock naming our PID must be refused"),
            Err(e) => e.to_string(),
        };
        assert!(
            msg.contains("already open in this process"),
            "error should name the same-process case, got: {msg}"
        );
    }

    #[test]
    fn test_file_backend_create() {
        let dir = tempfile::tempdir().unwrap();
        let temp_path = dir.path().join("test_minigraf_create.graph");

        let backend = FileBackend::open(&temp_path).unwrap();
        assert_eq!(backend.backend_name(), "file");
        assert_eq!(backend.page_count().unwrap(), 1); // Header page
        assert!(backend.is_new(), "newly created file should be new");
    }

    #[test]
    fn test_file_backend_existing_file_not_new() {
        let dir = tempfile::tempdir().unwrap();
        let temp_path = dir.path().join("test_minigraf_existing.graph");

        {
            let backend = FileBackend::open(&temp_path).unwrap();
            assert!(backend.is_new(), "first open should be new");
        }

        {
            let backend = FileBackend::open(&temp_path).unwrap();
            assert!(
                !backend.is_new(),
                "reopening existing file should not be new"
            );
        }

        {
            let backend = FileBackend::open(&temp_path).unwrap();
            assert!(!backend.is_new(), "third open should still not be new");
        }
    }

    #[test]
    fn test_file_backend_write_read() {
        let dir = tempfile::tempdir().unwrap();
        let temp_path = dir.path().join("test_minigraf_write_read.graph");

        let mut backend = FileBackend::open(&temp_path).unwrap();

        let data = vec![42u8; PAGE_SIZE];
        backend.write_page(1, &data).unwrap(); // Page 0 is header

        let read_data = backend.read_page(1).unwrap();
        assert_eq!(data, read_data);
    }

    #[test]
    fn test_file_backend_persistence() {
        let dir = tempfile::tempdir().unwrap();
        let temp_path = dir.path().join("test_file_backend_persistence.graph");

        // Write data
        {
            let mut backend = FileBackend::open(&temp_path).unwrap();
            let data = vec![99u8; PAGE_SIZE];
            backend.write_page(1, &data).unwrap();
            backend.close().unwrap();
        }

        // Read data after reopening
        {
            let backend = FileBackend::open(&temp_path).unwrap();
            let read_data = backend.read_page(1).unwrap();
            assert_eq!(read_data[0], 99);
        }
    }

    #[test]
    fn test_file_backend_page_count() {
        let dir = tempfile::tempdir().unwrap();
        let temp_path = dir.path().join("test_minigraf_page_count.graph");

        let mut backend = FileBackend::open(&temp_path).unwrap();
        assert_eq!(backend.page_count().unwrap(), 1);

        backend.write_page(1, &vec![0u8; PAGE_SIZE]).unwrap();
        assert_eq!(backend.page_count().unwrap(), 2);

        backend.write_page(2, &vec![0u8; PAGE_SIZE]).unwrap();
        assert_eq!(backend.page_count().unwrap(), 3);
    }
}
