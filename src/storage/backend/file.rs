/// File-based storage backend for native platforms.
use crate::storage::{FileHeader, PAGE_SIZE, StorageBackend};
use anyhow::Result;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// What `FileBackend::open` should do with the result of `File::try_lock`.
///
/// Split out from the I/O so the unsupported-filesystem branch is testable
/// without an exotic filesystem: no CI runner can be relied on to provide a
/// mount whose locks fail.
enum LockOutcome {
    /// We hold the lock.
    Acquired,
    /// Another open file description holds it. May be in this process.
    Held,
    /// The filesystem cannot lock, and the caller did not opt in to running
    /// without one.
    Unsupported(std::io::Error),
    /// The filesystem cannot lock and the caller accepted the risk.
    ProceedUnlocked,
}

/// Decide what a `try_lock` result means.
///
/// `allow_unlocked` is consulted ONLY for `TryLockError::Error`, never for
/// `WouldBlock`. Letting it bypass a live lock would reintroduce the
/// two-writers-one-file corruption the lock exists to prevent.
fn classify(result: Result<(), std::fs::TryLockError>, allow_unlocked: bool) -> LockOutcome {
    match result {
        Ok(()) => LockOutcome::Acquired,
        Err(std::fs::TryLockError::WouldBlock) => LockOutcome::Held,
        Err(std::fs::TryLockError::Error(_)) if allow_unlocked => LockOutcome::ProceedUnlocked,
        Err(std::fs::TryLockError::Error(e)) => LockOutcome::Unsupported(e),
    }
}

/// Canonicalised paths this process currently has open.
///
/// Purely diagnostic. Correctness comes from the kernel lock; this only picks
/// between two error messages. The distinction matters because #304 was
/// misdiagnosed for a long time when the generic "another process" wording
/// sent the investigation looking for a second process that did not exist.
static OPEN_PATHS: std::sync::LazyLock<std::sync::Mutex<std::collections::HashSet<PathBuf>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashSet::new()));

/// Removes this backend's path from `OPEN_PATHS` when the backend drops.
struct PathGuard(Option<PathBuf>);

impl Drop for PathGuard {
    fn drop(&mut self) {
        if let Some(path) = self.0.take()
            && let Ok(mut open) = OPEN_PATHS.lock()
        {
            open.remove(&path);
        }
    }
}

/// True if this process already has `path` open. Diagnostic only.
fn already_open_here(path: &Path) -> bool {
    OPEN_PATHS
        .lock()
        .map(|open| open.contains(path))
        .unwrap_or(false)
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
    // `_path_guard` must drop before `file`: dropping `file` releases the
    // kernel lock, and if the registry entry were still present after that,
    // another thread in this process could win the lock and insert its own
    // entry between the two drops -- which this guard would then delete,
    // leaving the registry claiming "not open" while a live handle exists.
    // Rust drops struct fields in declaration order, so this field must be
    // declared before `file`.
    _path_guard: PathGuard,
    file: File,
    header: FileHeader,
    is_new: bool,
}

impl FileBackend {
    /// Open or create a .graph file at the given path.
    ///
    /// If the file doesn't exist, creates it with an initial header.
    /// If it exists, validates and loads the header.
    ///
    /// Takes a kernel file lock on the `.graph` file itself, which prevents
    /// both multi-process corruption and a second handle within this process.
    /// The kernel releases the lock when the process exits, however it exits,
    /// so a crashed holder never leaves the database unopenable (#317).
    ///
    /// Production code calls `open_with` directly. This wrapper survives as a
    /// convenience for the ~40 in-crate test call sites.
    #[cfg(test)]
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::open_with(path, false)
    }

    /// As [`FileBackend::open`], but `allow_unlocked` permits opening on a
    /// filesystem that cannot lock at all.
    ///
    /// `allow_unlocked` does NOT override a lock held by someone else. It
    /// applies only when the filesystem rejects locking outright.
    pub fn open_with<P: AsRef<Path>>(path: P, allow_unlocked: bool) -> Result<Self> {
        let path = path.as_ref().to_path_buf();

        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;

        // Canonicalise only after the file exists. Used for the diagnostic
        // registry; if it fails we fall back to the generic message.
        let canonical = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());

        let path_guard = match classify(file.try_lock(), allow_unlocked) {
            LockOutcome::Acquired => {
                if let Ok(mut open) = OPEN_PATHS.lock() {
                    open.insert(canonical.clone());
                }
                PathGuard(Some(canonical))
            }
            LockOutcome::Held if already_open_here(&canonical) => {
                anyhow::bail!(
                    "Database is already open in this process ({}). A second handle \
                     on one file would give each its own page table and corrupt both \
                     — reuse the existing handle instead. `Minigraf` is cheap to clone \
                     and all clones share the same database.",
                    path.display()
                );
            }
            LockOutcome::Held => {
                anyhow::bail!(
                    "Database is locked by another process ({}). The lock is held on \
                     the file itself and is released automatically when the holding \
                     process exits, so there is no lock file to clean up.",
                    path.display()
                );
            }
            LockOutcome::Unsupported(e) => {
                anyhow::bail!(
                    "Failed to lock database at {}: {}. This filesystem does not \
                     support file locking (common on NFSv3 without lockd, and on some \
                     FUSE mounts). Set `allow_unlocked` in `OpenOptions` to open \
                     anyway — that accepts the risk that concurrent writers corrupt \
                     the file.",
                    path.display(),
                    e
                );
            }
            LockOutcome::ProceedUnlocked => {
                // There is no kernel lock in this arm, so the registry is not
                // merely diagnostic here: it is the only thing standing
                // between us and the #304 two-page-tables corruption if a
                // second handle in this process opens the same unlockable
                // path.
                if already_open_here(&canonical) {
                    anyhow::bail!(
                        "Database is already open in this process ({}). A second handle \
                         on one file would give each its own page table and corrupt both \
                         — reuse the existing handle instead. `Minigraf` is cheap to clone \
                         and all clones share the same database.",
                        path.display()
                    );
                }
                if let Ok(mut open) = OPEN_PATHS.lock() {
                    open.insert(canonical.clone());
                }
                PathGuard(Some(canonical))
            }
        };

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
            _path_guard: path_guard,
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
    /// refused. `flock` and OFD locks attach to the open file description, not
    /// the process, so the kernel denies this without any bookkeeping from us.
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
            "error should name the same-process case"
        );

        drop(first);
        // Once the only handle is dropped, the kernel releases the lock.
        FileBackend::open(&temp_path).unwrap();
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

    /// A leftover sidecar from v1.2.x must not block an open. We neither read
    /// nor delete it: a still-running old process may rely on it.
    #[test]
    fn test_leftover_sidecar_is_ignored_and_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let temp_path = dir.path().join("test_minigraf_leftover.graph");
        let legacy = temp_path.with_extension("graph.lock");
        std::fs::write(&legacy, "1").unwrap();

        let backend = FileBackend::open(&temp_path).unwrap();
        drop(backend);

        assert!(
            legacy.exists(),
            "a leftover sidecar must be left alone, not deleted"
        );
    }

    /// The path registry must not leak entries: a refused open must leave the
    /// set exactly as it found it, so a later refusal still reports the
    /// same-process case rather than degrading to the generic
    /// "locked by another process" wording.
    #[test]
    fn test_refused_open_does_not_corrupt_path_registry() {
        let dir = tempfile::tempdir().unwrap();
        let temp_path = dir.path().join("test_minigraf_registry.graph");

        let first = FileBackend::open(&temp_path).unwrap();
        assert!(FileBackend::open(&temp_path).is_err()); // first refusal

        // A refused open must not have removed first's registry entry: a
        // second refusal must still see it and report the same-process case.
        let msg = match FileBackend::open(&temp_path) {
            Ok(_) => panic!("must be refused"),
            Err(e) => e.to_string(),
        };
        assert!(
            msg.contains("already open in this process"),
            "registry entry must survive a refused open"
        );

        drop(first);
        FileBackend::open(&temp_path).unwrap();
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

    #[test]
    fn test_classify_ok_is_acquired() {
        assert!(matches!(classify(Ok(()), false), LockOutcome::Acquired));
        assert!(matches!(classify(Ok(()), true), LockOutcome::Acquired));
    }

    #[test]
    fn test_classify_would_block_is_held_regardless_of_allow_unlocked() {
        // allow_unlocked must NOT override a live lock: doing so reintroduces
        // exactly the cross-process corruption the lock exists to prevent.
        assert!(matches!(
            classify(Err(std::fs::TryLockError::WouldBlock), false),
            LockOutcome::Held
        ));
        assert!(matches!(
            classify(Err(std::fs::TryLockError::WouldBlock), true),
            LockOutcome::Held
        ));
    }

    #[test]
    fn test_classify_unsupported_fails_closed_by_default() {
        let e = std::io::Error::from_raw_os_error(95); // ENOTSUP
        assert!(matches!(
            classify(Err(std::fs::TryLockError::Error(e)), false),
            LockOutcome::Unsupported(_)
        ));
    }

    #[test]
    fn test_classify_unsupported_proceeds_when_allowed() {
        let e = std::io::Error::from_raw_os_error(37); // ENOLCK
        assert!(matches!(
            classify(Err(std::fs::TryLockError::Error(e)), true),
            LockOutcome::ProceedUnlocked
        ));
    }
}
