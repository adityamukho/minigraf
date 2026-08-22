# Kernel File Locking Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the PID-sidecar `FileLock` with kernel file locking via `std::fs::File::try_lock`, closing #317 (a `.graph` file permanently unopenable after a container restart) and #304 (two handles on one file in one process).

**Architecture:** `FileBackend` already holds an open `File` on the `.graph` file. The lock moves onto that file descriptor, so the kernel owns liveness: a dead holder's lock is released by the OS, and a second open in the same process is denied because `flock`/OFD locks attach to the open file description rather than the process. The `.graph.lock` sidecar, `is_process_alive`, and the recursion in `acquire` are all deleted. A process-global path set survives only to choose between two error messages.

**Tech Stack:** Rust 2024 edition, `std::fs::File::try_lock` / `TryLockError` (stable 1.89), `std::sync::LazyLock`, `tempfile` for tests, `unshare(1)` for the PID-namespace test.

**Spec:** `docs/superpowers/specs/2026-08-22-kernel-file-locking-design.md`

## Global Constraints

- **MSRV is Rust 1.89.** `Cargo.toml` must declare `rust-version = "1.89"`. `File::try_lock` is `#[stable(feature = "file_lock", since = "1.89.0")]`.
- **No new dependencies.** CI's binary-size gate is 1 MiB and the last measured build was 1006 KiB — 18 KiB of headroom. Do not add `fs4`, `rustix`, `libc`, or `windows-sys`.
- **Never use `{:?}` on `Result`, `Fact`, `Value`, `EdnValue`, or anything transitively containing a `Uuid` in assert messages.** CodeQL flags it as `rust/cleartext-logging` and it blocks CI. Use plain string messages, or `unwrap()`/`expect()`. See CLAUDE.md.
- **`allow_unlocked` never overrides a live lock.** It applies only when the filesystem cannot lock. Enforce in code, not by convention.
- **Do not delete a leftover `.graph.lock`.** A still-running v1.2.x process may depend on it. Ignore it.
- **Scope excludes the nightly NFS and Docker CI work**, tracked separately in issue #324.

---

### Task 1: `classify` — the lock decision, as a pure function

Isolating the decision from the I/O makes the unsupported-filesystem branch testable without an exotic filesystem, which is otherwise untestable in CI.

**Files:**
- Modify: `Cargo.toml` (add `rust-version`)
- Modify: `src/storage/backend/file.rs` (add `LockOutcome`, `classify`, and unit tests)

**Interfaces:**
- Consumes: nothing.
- Produces: `enum LockOutcome { Acquired, Held, Unsupported(std::io::Error), ProceedUnlocked }` and `fn classify(result: Result<(), std::fs::TryLockError>, allow_unlocked: bool) -> LockOutcome`, both private to the module. Task 2 calls `classify`.

- [ ] **Step 1: Declare the MSRV**

In `Cargo.toml`, in the `[package]` section, directly after the `edition = "2024"` line:

```toml
rust-version = "1.89"
```

- [ ] **Step 2: Write the failing tests**

Append inside the existing `mod tests` block in `src/storage/backend/file.rs` (the block opens at `#[cfg(all(test, not(target_arch = "wasm32")))]`):

```rust
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
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --lib classify`
Expected: FAIL to compile — `cannot find function 'classify' in this scope`.

- [ ] **Step 4: Write the implementation**

Add near the top of `src/storage/backend/file.rs`, after the `use` statements and before `pub struct FileBackend`:

```rust
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
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --lib classify`
Expected: PASS, 4 tests.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml src/storage/backend/file.rs
git commit -m "feat(storage): add lock-decision classifier and declare MSRV 1.89

Isolates the try_lock decision from the I/O so the unsupported-filesystem
branch is testable without a filesystem that cannot lock.

allow_unlocked deliberately does not apply to WouldBlock."
```

---

### Task 2: Replace `FileLock` with the kernel lock

**Files:**
- Modify: `src/storage/backend/file.rs` — delete `FileLock` (lines 8-114 in the current file: the struct, `impl FileLock`, `is_process_alive`, and `impl Drop for FileLock`), rewrite `FileBackend::open`, replace the three existing lock unit tests
- Test: `src/storage/backend/file.rs` `mod tests`

**Interfaces:**
- Consumes: `classify`, `LockOutcome` from Task 1.
- Produces: `FileBackend::open<P: AsRef<Path>>(path: P) -> Result<Self>` (unchanged signature, `allow_unlocked = false`) and `FileBackend::open_with<P: AsRef<Path>>(path: P, allow_unlocked: bool) -> Result<Self>`. Task 3 calls `open_with`. The `FileBackend` struct field `_lock: FileLock` becomes `_path_guard: PathGuard`.

**Why `open_with` rather than adding a parameter:** `FileBackend::open` has roughly 35 call sites, nearly all in tests. Keeping `open` and adding `open_with` leaves them untouched.

- [ ] **Step 1: Write the failing tests**

In `src/storage/backend/file.rs`, replace the three existing tests — `test_second_open_in_same_process_is_refused`, `test_reopen_after_drop_still_works`, and `test_dead_other_process_lock_is_still_reclaimed` — with these. Note the third is deleted outright, not rewritten: reclaiming a dead process's lock is now the kernel's job and there is nothing left in our code to test.

```rust
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
    /// set exactly as it found it, or a later legitimate open reports the
    /// wrong error.
    #[test]
    fn test_refused_open_does_not_corrupt_path_registry() {
        let dir = tempfile::tempdir().unwrap();
        let temp_path = dir.path().join("test_minigraf_registry.graph");

        let first = FileBackend::open(&temp_path).unwrap();
        assert!(FileBackend::open(&temp_path).is_err());
        drop(first);

        // If the refused open had removed the path, or the guard had failed to,
        // this would report the same-process case instead of succeeding.
        FileBackend::open(&temp_path).unwrap();
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib storage::backend::file`
Expected: `test_leftover_sidecar_is_ignored_and_preserved` FAILS — the current code reads the sidecar, sees PID 1 (or whatever was written), and either refuses or reclaims it. The other three may pass against the old code; that is expected and fine.

- [ ] **Step 3: Delete `FileLock` entirely**

Remove from `src/storage/backend/file.rs`: the `struct FileLock` declaration and its doc comment, the whole `impl FileLock` block (including `acquire` and `is_process_alive`), and `impl Drop for FileLock`. This is the contiguous region from the `/// Advisory file lock to prevent multi-process corruption.` comment through the closing brace of `impl Drop for FileLock`.

- [ ] **Step 4: Add the path registry**

Add to `src/storage/backend/file.rs`, immediately after the `classify` function from Task 1:

```rust
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
```

- [ ] **Step 5: Rewrite `FileBackend::open`**

Change the struct field in `pub struct FileBackend`, replacing `_lock: FileLock,` with:

```rust
    _path_guard: PathGuard,
```

Replace `FileBackend::open` and its doc comment with the following. Note the inverted order: the file is opened **first**, then locked, because the lock now lives on that file descriptor. `canonicalize` also requires the file to exist.

```rust
    /// Open or create a .graph file at the given path.
    ///
    /// If the file doesn't exist, creates it with an initial header.
    /// If it exists, validates and loads the header.
    ///
    /// Takes a kernel file lock on the `.graph` file itself, which prevents
    /// both multi-process corruption and a second handle within this process.
    /// The kernel releases the lock when the process exits, however it exits,
    /// so a crashed holder never leaves the database unopenable (#317).
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
            LockOutcome::ProceedUnlocked => PathGuard(None),
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
```

- [ ] **Step 6: Run the full lib test suite**

Run: `cargo test --lib`
Expected: PASS. If `use std::io::Write;` is now unused, remove it — `write!` was only used to write the PID into the sidecar.

- [ ] **Step 7: Commit**

```bash
git add src/storage/backend/file.rs
git commit -m "fix(storage): lock the .graph file in the kernel, not a PID sidecar

A PID is not a liveness token. It is not unique across PID namespaces and
is recycled within one, so the sidecar could never distinguish a crashed
holder from a live one. A container killed with SIGKILL left the sidecar
holding pid 1, and the replacement container -- also pid 1 -- refused to
open the database forever (#317).

File::try_lock puts the decision in the kernel, which releases the lock on
process death however it occurs. Because flock and OFD locks attach to the
open file description rather than the process, this also denies a second
open within one process, covering #304 with no bookkeeping.

A leftover sidecar from v1.2.x is ignored, never deleted: a still-running
old process may depend on it."
```

---

### Task 3: `OpenOptions::allow_unlocked`

**Files:**
- Modify: `src/db.rs` — `OpenOptions` struct (around line 63), its `Default` impl (around line 78), its builder impl (near the `page_cache_size` builder at line 98), the `open_with_options` call site (line 293), and the stale sidecar comment (line 610)
- Test: `src/db.rs` `mod tests`

**Interfaces:**
- Consumes: `FileBackend::open_with` from Task 2.
- Produces: `OpenOptions { pub allow_unlocked: bool, .. }` and the builder `OpenOptions::allow_unlocked(self, allow: bool) -> Self`.

`OpenOptions` exposes both public fields and chainable builder methods, so this needs both to match the existing pattern.

- [ ] **Step 1: Write the failing test**

Add to the `mod tests` block in `src/db.rs`:

```rust
    #[test]
    fn test_allow_unlocked_defaults_to_false() {
        assert!(!OpenOptions::default().allow_unlocked);
    }

    #[test]
    fn test_allow_unlocked_builder_sets_the_field() {
        assert!(OpenOptions::default().allow_unlocked(true).allow_unlocked);
    }

    /// `allow_unlocked` must not open a door past a live lock. It applies only
    /// where the filesystem cannot lock at all.
    #[test]
    fn test_allow_unlocked_does_not_bypass_a_live_lock() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("allow_unlocked.graph");

        let first = Minigraf::open(&path).unwrap();
        let opts = OpenOptions::default().allow_unlocked(true);
        assert!(
            Minigraf::open_with_options(&path, opts).is_err(),
            "allow_unlocked must not bypass a lock that is actually held"
        );
        drop(first);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib allow_unlocked`
Expected: FAIL to compile — `no field 'allow_unlocked' on type 'OpenOptions'`.

- [ ] **Step 3: Add the field**

In `src/db.rs`, add to `pub struct OpenOptions`, after `pub max_results: usize,`:

```rust
    /// Open even when the filesystem cannot lock files at all.
    ///
    /// Defaults to `false`, which refuses to open rather than run without the
    /// protection the lock provides. Set this only when you know the database
    /// has a single writer by other means — for example an NFSv3 mount with no
    /// `lockd`, where locking is unavailable rather than merely contended.
    ///
    /// This does **not** override a lock that another handle actually holds.
    /// Opening a database twice is refused regardless of this setting.
    pub allow_unlocked: bool,
```

In the `Default` impl, after `max_results: DEFAULT_MAX_RESULTS,`:

```rust
            allow_unlocked: false,
```

- [ ] **Step 4: Add the builder method**

In `src/db.rs`, next to the `page_cache_size` builder method:

```rust
    /// Open even when the filesystem cannot lock files. See
    /// [`OpenOptions::allow_unlocked`] for when this is appropriate.
    #[must_use]
    pub fn allow_unlocked(mut self, allow: bool) -> Self {
        self.allow_unlocked = allow;
        self
    }
```

- [ ] **Step 5: Plumb it through**

In `src/db.rs`, in `open_with_options`, replace:

```rust
        let backend = FileBackend::open(&db_path)?;
```

with:

```rust
        let backend = FileBackend::open_with(&db_path, opts.allow_unlocked)?;
```

Then fix the stale comment around line 610, replacing the sentence beginning `// File locking (\`.graph.lock\` sidecar, acquired by FileBackend::open)` and the lines through `// same-process double-opens (same PID bypasses the stale-lock check)` with:

```rust
                // The kernel file lock taken by FileBackend::open covers both a
                // second process and a second handle in this one, since flock
                // and OFD locks attach to the open file description. This guard
                // remains for environments where the lock is unavailable and
                // the caller set `allow_unlocked` (e.g.
```

Keep whatever text followed on the original line so the sentence still closes.

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test --lib allow_unlocked`
Expected: PASS, 3 tests.

- [ ] **Step 7: Commit**

```bash
git add src/db.rs
git commit -m "feat(db): add OpenOptions::allow_unlocked

Fails closed by default when a filesystem cannot lock. The escape hatch is
for single-writer-by-convention deployments on mounts without working
locks; it never bypasses a lock another handle holds."
```

---

### Task 4: Real subprocess crash simulation

`mem::forget` no longer models a crash. It leaks the `File`, so the fd stays open and the kernel lock is held for the rest of the test process — and no external cleanup can release it. A real child process that aborts is both the correct model and the only one that needs no test-only API in the shipping crate.

**Files:**
- Modify: `tests/wal_test.rs` — add the helper and the child entry point, convert 13 sites (`std::mem::forget(` at lines 89, 132, 192, 280, 325, 374, 524, 559, 614, 641, 665, 684, 705), delete `simulate_crashed_holder` (line 840) and its 13 call sites
- Modify: `tests/migration_matrix_test.rs` — convert 1 site (line 101) and delete the manual `remove_file` of the sidecar that follows it

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: `fn run_crashing_child(db_path: &std::path::Path, wal_checkpoint_threshold: usize, statements: &[&str])`, duplicated in both test files (integration test binaries do not share code without a `tests/common/` module; duplicating one small helper is simpler than adding one).

- [ ] **Step 1: Write the helper and its child entry point**

Add near the top of `tests/wal_test.rs`, after the existing `use` statements:

```rust
/// Runs a child process that opens `db_path`, executes `statements`, then dies
/// by `abort()` without running any `Drop`.
///
/// This is a real crash, not a simulated one: the child's file descriptors are
/// closed by the kernel, which releases the file lock exactly as it would for a
/// SIGKILLed container. `mem::forget` cannot model this — it leaks the `File`,
/// so the lock stays held for the life of the test process.
fn run_crashing_child(
    db_path: &std::path::Path,
    wal_checkpoint_threshold: usize,
    statements: &[&str],
) {
    let exe = std::env::current_exe().expect("test binary path");
    let status = std::process::Command::new(exe)
        .args(["crash_child_entrypoint", "--exact", "--nocapture"])
        .env("MINIGRAF_CRASH_DB", db_path)
        .env("MINIGRAF_CRASH_THRESHOLD", wal_checkpoint_threshold.to_string())
        .env("MINIGRAF_CRASH_STMTS", statements.join("\u{1e}"))
        .status()
        .expect("spawn crash child");

    // On Unix `abort()` raises SIGABRT and `code()` is None. On Windows it
    // terminates with a nonzero exit code instead. The property that matters
    // on both is that the child did not exit cleanly.
    assert!(
        status.code().unwrap_or(1) != 0,
        "crash child must not exit cleanly"
    );
}

/// The child half of [`run_crashing_child`]. A no-op in ordinary test runs.
///
/// This is a `#[test]` purely so the harness will dispatch to it by name; when
/// the marker environment variable is absent it returns immediately.
#[test]
fn crash_child_entrypoint() {
    let Ok(db_path) = std::env::var("MINIGRAF_CRASH_DB") else {
        return;
    };
    let threshold: usize = std::env::var("MINIGRAF_CRASH_THRESHOLD")
        .expect("threshold")
        .parse()
        .expect("threshold is a number");
    let stmts = std::env::var("MINIGRAF_CRASH_STMTS").expect("statements");

    let db = Minigraf::open_with_options(
        &db_path,
        OpenOptions {
            wal_checkpoint_threshold: threshold,
            ..Default::default()
        },
    )
    .expect("child opens db");

    for stmt in stmts.split('\u{1e}').filter(|s| !s.is_empty()) {
        db.execute(stmt).expect("child statement");
    }

    // Die without running Drop, so no checkpoint happens and the WAL is left
    // for the next open to replay.
    std::process::abort();
}
```

- [ ] **Step 2: Convert the first site and verify the shape**

In `tests/wal_test.rs`, `test_wal_recovery_after_simulated_crash`, replace the whole block that opens the db, executes, calls `std::mem::forget(db)` and then `simulate_crashed_holder(&db_path)` with:

```rust
    run_crashing_child(
        &db_path,
        1_000_000,
        &[r#"(transact [[:alice :name "Alice"]])"#],
    );
```

Keep the surrounding assertions unchanged.

- [ ] **Step 3: Run that single test**

Run: `cargo test --test wal_test test_wal_recovery_after_simulated_crash -- --nocapture`
Expected: PASS. The WAL exists after the child dies, and the reopen recovers the fact.

- [ ] **Step 4: Convert the remaining 12 sites in `wal_test.rs`**

Apply the same shape at each remaining `std::mem::forget(` site: lines 132, 192, 280, 325, 374, 524, 559, 614, 641, 665, 684, 705. For each, read the block's existing `OpenOptions` to get its `wal_checkpoint_threshold` and pass that value; collect the `db.execute(...)` calls in that block into the `statements` slice in order. Then delete the `fn simulate_crashed_holder` definition at line 840 and every remaining call to it.

- [ ] **Step 5: Convert the site in `migration_matrix_test.rs`**

Copy the `run_crashing_child` helper and `crash_child_entrypoint` from Step 1 into `tests/migration_matrix_test.rs`. Replace the block containing `std::mem::forget(db)` at line 101 with:

```rust
    run_crashing_child(&path, 1000, &[r#"(transact [[:e2 :color "blue"]])"#]);
```

Delete the comment block that follows about `mem::forget` skipping `FileLock::drop`, and the `std::fs::remove_file(path.with_extension("graph.lock")).unwrap();` line beneath it. There is no sidecar to remove, and the `unwrap()` would now panic.

- [ ] **Step 6: Run both suites**

Run: `cargo test --test wal_test --test migration_matrix_test`
Expected: PASS, with no reference to `simulate_crashed_holder` or `mem::forget` remaining. Verify with:

```bash
grep -rn "simulate_crashed_holder\|mem::forget" tests/
```

Expected: no output.

- [ ] **Step 7: Commit**

```bash
git add tests/wal_test.rs tests/migration_matrix_test.rs
git commit -m "test: simulate crashes with real subprocesses

mem::forget no longer models a crash: it leaks the File, so the kernel
lock stays held for the life of the test process and no external cleanup
can release it.

A child that aborts is both the accurate model -- the kernel closes its
descriptors exactly as it would for a SIGKILLed container -- and the one
that needs no test-only hook in the shipping crate. #[cfg(test)] cannot
reach tests/, and #[doc(hidden)] pub would be callable in production."
```

---

### Task 5: The #317 regression test

**Files:**
- Create: `tests/pid_namespace_test.rs`
- Create: `examples/pid_ns_helper.rs`

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: nothing consumed later.

This is the test that reproduces the reported bug. Verified to fail against the pre-fix code: container A is SIGKILLed leaving the sidecar holding `1`, and container B — also PID 1 — is refused permanently.

- [ ] **Step 1: Write the test file**

Create `tests/pid_namespace_test.rs`:

```rust
//! #317: a `.graph` file must stay openable after its holder dies in another
//! PID namespace.
//!
//! Kubernetes is where this was reported, but the bug has nothing to do with
//! Kubernetes: every container's main process is PID 1 in its own namespace, so
//! a lock keyed on a PID cannot tell a crashed holder from a live one. `unshare`
//! reproduces it in a few seconds with no container runtime.
#![cfg(target_os = "linux")]

use std::path::Path;
use std::process::{Command, Stdio};

/// The `unshare` invocation that puts a command at PID 1 in a fresh namespace,
/// or `None` if this environment cannot create one.
///
/// Ubuntu 24.04 restricts unprivileged user namespaces via
/// `kernel.apparmor_restrict_unprivileged_userns`, so fall back to `sudo` where
/// it is available passwordlessly (GitHub runners). If neither works the test
/// skips rather than fails: not every environment permits namespaces.
fn unshare_prefix() -> Option<Vec<String>> {
    let unprivileged: Vec<String> = [
        "unshare",
        "--user",
        "--map-root-user",
        "--pid",
        "--fork",
        "--mount-proc",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();

    let privileged: Vec<String> = [
        "sudo", "-n", "unshare", "--pid", "--fork", "--mount-proc",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();

    for candidate in [unprivileged, privileged] {
        let ok = Command::new(&candidate[0])
            .args(&candidate[1..])
            .args(["sh", "-c", "test \"$$\" = 1"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ok {
            return Some(candidate);
        }
    }
    None
}

/// Runs the helper binary at PID 1 in a fresh PID namespace.
fn spawn_in_namespace(prefix: &[String], helper: &Path, db: &Path, mode: &str) -> std::process::Child {
    Command::new(&prefix[0])
        .args(&prefix[1..])
        .arg(helper)
        .arg(db)
        .arg(mode)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn in namespace")
}

/// Path to the compiled helper binary (see `examples/pid_ns_helper.rs`).
fn helper_binary() -> std::path::PathBuf {
    // target/debug/examples/pid_ns_helper, resolved relative to the test binary
    // at target/debug/deps/pid_namespace_test-<hash>.
    let mut dir = std::env::current_exe().expect("test binary path");
    dir.pop(); // deps/
    dir.pop(); // debug/
    dir.join("examples").join("pid_ns_helper")
}

#[test]
fn test_lock_survives_holder_death_in_another_pid_namespace() {
    let Some(prefix) = unshare_prefix() else {
        eprintln!("SKIP: this environment cannot create PID namespaces");
        return;
    };
    let helper = helper_binary();
    if !helper.exists() {
        eprintln!("SKIP: helper not built; run `cargo build --examples` first");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("shared.graph");

    // Container A: opens the database and holds it.
    let mut a = spawn_in_namespace(&prefix, &helper, &db, "hold");

    // Wait for it to report that it holds the lock.
    let mut held = false;
    for _ in 0..80 {
        if db.exists() && std::fs::metadata(&db).map(|m| m.len() > 0).unwrap_or(false) {
            held = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(held, "container A never opened the database");

    // Container A is OOMKilled. No Drop runs.
    let _ = a.kill();
    let _ = a.wait();

    // Container B: a different container, also PID 1 in its own namespace.
    let b = spawn_in_namespace(&prefix, &helper, &db, "open")
        .wait_with_output()
        .expect("container B ran");

    let out = String::from_utf8_lossy(&b.stdout);
    assert!(
        out.contains("OPEN_OK"),
        "a replacement container must be able to open the database after its \
         predecessor was killed; this is #317"
    );
}

#[test]
fn test_two_live_namespaced_holders_are_still_refused() {
    let Some(prefix) = unshare_prefix() else {
        eprintln!("SKIP: this environment cannot create PID namespaces");
        return;
    };
    let helper = helper_binary();
    if !helper.exists() {
        eprintln!("SKIP: helper not built; run `cargo build --examples` first");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("contended.graph");

    let mut a = spawn_in_namespace(&prefix, &helper, &db, "hold");
    let mut held = false;
    for _ in 0..80 {
        if db.exists() && std::fs::metadata(&db).map(|m| m.len() > 0).unwrap_or(false) {
            held = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(held, "container A never opened the database");

    // A is still alive. B must be refused — this is the guarantee PR #318's
    // start-time comparison would have broken, admitting two live writers.
    let b = spawn_in_namespace(&prefix, &helper, &db, "open")
        .wait_with_output()
        .expect("container B ran");
    let out = String::from_utf8_lossy(&b.stdout);

    let _ = a.kill();
    let _ = a.wait();

    assert!(
        out.contains("OPEN_ERR"),
        "a second live holder in another PID namespace must still be refused"
    );
}
```

- [ ] **Step 2: Write the helper binary**

Create `examples/pid_ns_helper.rs`:

```rust
//! Helper for `tests/pid_namespace_test.rs`. Opens a database and either holds
//! it or reports whether the open succeeded.
//!
//! An example rather than a test so it is a standalone binary the test can run
//! under `unshare`.

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = &args[1];
    let mode = &args[2];

    match minigraf::Minigraf::open(path) {
        Ok(db) => {
            db.execute(r#"(transact [[:a :name "A"]])"#).ok();
            println!("OPEN_OK");
            if mode == "hold" {
                std::thread::sleep(std::time::Duration::from_secs(300));
            }
        }
        Err(e) => {
            println!("OPEN_ERR {e}");
            std::process::exit(3);
        }
    }
}
```

- [ ] **Step 3: Build the helper and run the tests**

Run: `cargo build --examples && cargo test --test pid_namespace_test -- --nocapture`
Expected: PASS, 2 tests (or SKIP with a printed reason if namespaces are unavailable).

- [ ] **Step 4: Verify the test actually catches the bug**

Confirm the test has real catching power by checking it against the old behaviour:

`git stash` cannot help here: Tasks 1 and 2 are already committed, and stash only touches uncommitted work. Use a throwaway worktree at the pre-fix commit instead. The test uses only the public `Minigraf::open` API, so it compiles unchanged against the old code.

```bash
TMP=$(mktemp -d)
git worktree add "$TMP/prefix" e056905
cp tests/pid_namespace_test.rs "$TMP/prefix/tests/"
cp examples/pid_ns_helper.rs "$TMP/prefix/examples/"
(cd "$TMP/prefix" && cargo build --examples && cargo test --test pid_namespace_test -- --nocapture)
git worktree remove --force "$TMP/prefix"
rm -rf "$TMP"
```

Expected: `test_lock_survives_holder_death_in_another_pid_namespace` FAILS against the pre-fix code, reporting that container B could not open the database. If it passes, the test is not reproducing #317 and must be fixed before continuing. Report the observed failure output in your report file.

- [ ] **Step 5: Commit**

```bash
git add tests/pid_namespace_test.rs examples/pid_ns_helper.rs
git commit -m "test: reproduce #317 across PID namespaces

Two processes at PID 1 in separate namespaces sharing one file, via
unshare -- no container runtime needed. Verified to fail against the
pre-fix code: the killed holder's sidecar left pid 1 behind and the
replacement was refused permanently.

The second test guards the other direction: two live holders must still
be refused, which is what PR #318's start-time comparison would have
broken.

Co-Authored-By: Olive Casazza <olive.casazza@schrodinger.com>"
```

---

### Task 6: Documentation

**Files:**
- Modify: `CHANGELOG.md` — under the existing `## Unreleased` heading
- Modify: `docs/ERROR_REFERENCE.md` — new `STG-025`, `STG-026`, `STG-027` entries plus their Quick Reference Table rows
- Modify: `docs/TEST_COVERAGE.md` — lines 8, 48, 50, 509, 510, 557, 705
- Modify: `.wiki/Architecture.md` — lines 205 and 231
- Modify: `CLAUDE.md` — the stale test count on the "Test Coverage" line

**Interfaces:**
- Consumes: the error text written in Task 2.
- Produces: nothing consumed later.

- [ ] **Step 1: Add the CHANGELOG entry**

In `CHANGELOG.md`, under `## Unreleased`, above the existing `### Bug fixes` heading, add a new section. The version-relevant facts are recorded explicitly because the release this lands in has not been decided:

```markdown
### Breaking-ish changes

- **Minimum supported Rust version is now 1.89** (was effectively 1.85, implied
  by `edition = "2024"`). Required for `std::fs::File::try_lock`, stabilised in
  1.89. Declared as `rust-version` in `Cargo.toml`, so cargo reports a clear
  error rather than a confusing one.
- **On Windows, an open database can no longer be read by other processes.**
  `LockFileEx` locks are mandatory rather than advisory, so copying or backing
  up an open `.graph` file now fails on Windows where it previously succeeded.
  Unix locks remain advisory and are unaffected. Close the database before
  copying it — which also avoids a torn copy.
- **`open()` now fails on filesystems that cannot lock at all** rather than
  proceeding unprotected. Set `OpenOptions::allow_unlocked(true)` to accept the
  risk on single-writer deployments.
```

Then under the existing `### Bug fixes` heading, above the keywords entry:

```markdown
- **A `.graph` file is no longer bricked when its holder dies in a container**
  (#317): `FileLock` recorded the holder's PID in a `.graph.lock` sidecar and
  refused to open when that PID equalled our own. Every container's main process
  is PID 1 in its own namespace, so a container killed with SIGKILL left the
  sidecar holding `1`, and the replacement container — also PID 1 — read its own
  PID back and refused. The refusal was permanent; no code path could clear it,
  and an operator had to delete the file by hand. The `pid == our_pid` refusal
  was itself the fix for #304, so the two defects were entangled.

  A PID cannot serve as a liveness token: it is not unique across PID namespaces
  and is recycled within one. The sidecar is gone, and the lock now lives in the
  kernel via `std::fs::File::try_lock` on the `.graph` file itself. The kernel
  releases it whenever the process exits, however it exits, so a crashed holder
  leaves nothing behind. Because `flock` and OFD locks attach to the open file
  description rather than the process, a second open within one process is also
  denied, so #304 stays fixed with no PID bookkeeping at all.

  A leftover `.graph.lock` from an earlier version is ignored, never deleted — a
  still-running old process may still depend on it. Running mixed versions
  against one file is not supported; upgrade all writers together.
```

- [ ] **Step 2: Add the three error entries**

In `docs/ERROR_REFERENCE.md`, at the end of the `## STG — Storage Errors` section (after the `STG-024` entry, before `## WAL`):

```markdown
### STG-025 Database already open in this process

**Error text**: `Database is already open in this process (/path/to/db.graph). A second handle on one file would give each its own page table and corrupt both — reuse the existing handle instead.`

**Cause**: This process already holds an open handle on this file. Two `FileBackend` instances on one file each cache their own `header.page_count`, allocate new pages from that count, and bounds-check reads against it, so the two page tables diverge and produce intermittent `Page N out of bounds` errors.

**Resolution**:
- Reuse the handle you already have. `Minigraf` is cheap to clone and all clones share one database.
- If you cannot find the other handle, it is usually held by a longer-lived object than you expect — a cache, a registry, or a background task.

**Scenario**: A request handler calls `Minigraf::open` per request while a connection pool already holds the same path open.

### STG-026 Database locked by another process

**Error text**: `Database is locked by another process (/path/to/db.graph). The lock is held on the file itself and is released automatically when the holding process exits.`

**Cause**: Another process holds the kernel file lock on this `.graph` file. Minigraf is single-writer: one process at a time.

**Resolution**:
- Wait for the other process to exit; the kernel releases the lock automatically, including when the process is killed.
- There is no lock file to delete. If you find a stale `.graph.lock`, it is a leftover from a version before 1.3 and has no effect.
- If two services genuinely need the same file, put one in front of the other. Minigraf is embedded, not a server.

**Scenario**: A rolling deployment starts the new pod before the old one has exited, and both mount the same volume.

### STG-027 Filesystem does not support file locking

**Error text**: `Failed to lock database at /path/to/db.graph: Operation not supported (os error 95). This filesystem does not support file locking.`

**Cause**: The underlying filesystem rejected the lock outright. Common on NFSv3 mounts with no `lockd` running, and on some FUSE filesystems.

**Resolution**:
- Prefer moving the database to a filesystem that supports locking — this is the safe fix.
- On NFS, ensure NFSv4, or start `lockd` for NFSv3.
- If you are certain there is exactly one writer, set `OpenOptions::allow_unlocked(true)`. This accepts the risk that concurrent writers corrupt the file; Minigraf cannot detect them without a working lock.

**Scenario**: A `.graph` file placed on an NFSv3 export whose `lockd` is not running.
```

Then add the matching rows to the Quick Reference Table, after the last `STG-024` row:

```markdown
| STG-025 | Database already open in this process | Storage |
| STG-026 | Database locked by another process | Storage |
| STG-027 | Filesystem does not support file locking | Storage |
```

- [ ] **Step 3: Update the test coverage document**

In `docs/TEST_COVERAGE.md`:

- Line 48: replace the `FileLock` unit-test description with: `` - ✅ `src/storage/backend/file.rs` — lock unit tests: same-process second open refused (#304), sequential reopen-after-drop, leftover v1.2.x sidecar ignored and preserved, path registry not corrupted by a refused open, plus 4 `classify` tests covering the unsupported-filesystem branch in both `allow_unlocked` settings ``
- Line 50: replace the crash-simulation-idiom paragraph with: `` - ✅ Crash simulation uses a real child process that `abort()`s (`run_crashing_child`), replacing the former `mem::forget` idiom. `mem::forget` leaks the `File`, so the kernel lock stays held for the life of the test process; only real process death releases it, which is also what production does. ``
- Lines 509 and 510: replace the two `FileLock` bullets with: `` - ✅ **Same-process exclusion** (#304): a second open while a handle is live is refused, enforced by the kernel — `flock`/OFD locks attach to the open file description, not the process `` and `` - ✅ **Holder death across PID namespaces** (#317): `tests/pid_namespace_test.rs` runs two processes at PID 1 in separate namespaces via `unshare`; the survivor opens after the holder is killed, and two live holders are still refused ``
- Lines 557 and 705: replace `mem::forget` crash-simulation references with `real subprocess crash (`run_crashing_child`)`.
- Line 8: update the unit-test count after running the suite.

- [ ] **Step 4: Update the wiki**

In `.wiki/Architecture.md`, replace the File Locking paragraph at line 205:

```markdown
A `.graph` file is guarded by a kernel file lock taken on the file itself
(`std::fs::File::try_lock`: `flock` on Unix, `LockFileEx` on Windows). The lock
is released by the kernel whenever the holding process exits, however it exits,
so a crashed holder never leaves the database unopenable. Because `flock` and
OFD locks attach to the open file description rather than the process, a second
open within one process is refused too.

There is no lock file. A `.graph.lock` sidecar left behind by versions before
1.3 is ignored and never deleted, since a still-running old process may depend
on it. Running mixed versions against one file is not supported.

On a filesystem that cannot lock at all, `open()` fails rather than proceeding
unprotected. `OpenOptions::allow_unlocked(true)` overrides this, and accepts the
corruption risk that comes with it.

On Windows these locks are mandatory rather than advisory, so other processes
cannot read an open `.graph` file.
```

And at line 231, replace `enforced by the sidecar lock (see File Locking above)` with `enforced by the kernel file lock (see File Locking above)`.

- [ ] **Step 5: Fix the stale test count**

Run `cargo test 2>&1 | grep "^test result:"` and total the counts. Update the "Test Coverage" line in `CLAUDE.md` and line 8 of `docs/TEST_COVERAGE.md` to the real figure. Both currently claim "998 tests passing (990 passing, 8 ignored)", which was already stale before this work — the pre-change count was 999 passing plus 8 ignored.

- [ ] **Step 6: Verify docs build**

Run: `cargo test --doc && cargo doc --no-deps`
Expected: PASS, no broken intra-doc links.

- [ ] **Step 7: Commit**

```bash
git add CHANGELOG.md docs/ERROR_REFERENCE.md docs/TEST_COVERAGE.md CLAUDE.md
git commit -m "docs: record the kernel locking change

CHANGELOG carries the semver-relevant facts (new public option, MSRV
1.85 -> 1.89, behaviour change on unlockable filesystems, Windows
mandatory locks) so the release decision can be made later.

Adds STG-025 through STG-027; the lock errors had no reference entries."
```

The `.wiki/` directory is a separate git repository AND it does not exist in this worktree — it is a plain clone that lives only in the main checkout. Make the wiki edit there, and commit it in that repository:

```bash
cd /home/aditya/Work/AMC/Minigraf/minigraf/.wiki
# edit Architecture.md as described above
git add -A && git commit -m "docs: kernel file locking replaces the PID sidecar"
```

Do NOT push. Pushing the wiki is a shared-branch side effect for the human to authorise.

---

### Task 7: Verify the MSRV against the binding repositories

The MSRV bump is the one change that can break downstream without any test here failing. Seven binding repositories consume this crate.

**Files:**
- No source changes. Produces a recorded result for the PR description.

**Interfaces:**
- Consumes: the `rust-version` from Task 1.
- Produces: a finding to include in the PR body.

- [ ] **Step 1: Check each repository's declared toolchain**

```bash
for repo in minigraf-python minigraf-node minigraf-wasm minigraf-java minigraf-android minigraf-swift minigraf-c; do
  echo "=== $repo ==="
  gh api "repos/project-minigraf/$repo/contents/rust-toolchain.toml" --jq '.content' 2>/dev/null | base64 -d 2>/dev/null | grep -i channel || echo "  no rust-toolchain.toml"
  gh api "repos/project-minigraf/$repo/contents/Cargo.toml" --jq '.content' 2>/dev/null | base64 -d 2>/dev/null | grep -i "rust-version" || echo "  no rust-version declared"
done
```

If a repository name differs, list the organisation's repositories first with `gh repo list project-minigraf --limit 50`.

- [ ] **Step 2: Record the result**

Any repository pinning a toolchain below 1.89 needs its pin raised in the cascade release that follows. Note each one by name in the PR description. Do not change those repositories in this PR — the cascade is a separate process.

- [ ] **Step 3: Run the full suite once more**

Run: `cargo test --no-fail-fast && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: PASS on all three. `rust-clippy.yml` and `rustfmt.yml` both gate CI.

- [ ] **Step 4: Check the binary size gate**

```bash
cargo build --release && echo "$(( $(stat -c %s target/release/minigraf) / 1024 )) KiB of 1024 KiB limit"
```

Expected: no growth versus the pre-change baseline; std locking adds no dependency. Local builds run a few KiB above CI's, so compare the delta rather than the absolute number — CI's baseline was 1006 KiB.

---

## Self-Review

**Spec coverage.** Every section of the spec maps to a task: locking and `classify` to Tasks 1-2, same-process diagnostics to Task 2, `allow_unlocked` to Task 3, errors to Tasks 2 and 6, platform differences and compatibility to Task 6, in-process testing to Task 4, tier 1 to Task 5, and the MSRV risk to Task 7. Tiers 2 and 3 are deliberately excluded and tracked in #324.

**Type consistency.** `classify` and `LockOutcome` are defined in Task 1 and used unchanged in Task 2. `open_with` is defined in Task 2 and called in Task 3. `run_crashing_child` has one signature, used in both test files. `PathGuard` is defined and consumed within Task 2.

**Verified against std.** `TryLockError` has exactly the variants `Error(io::Error)` and `WouldBlock`, and `try_lock(&self) -> Result<(), TryLockError>`, both checked against the std source. The `PathGuard::drop` let-chain and `LazyLock` were compile-checked under edition 2024.
