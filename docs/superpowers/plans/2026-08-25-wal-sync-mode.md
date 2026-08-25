# WAL Sync Mode Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace `WalWriter`'s per-write `fsync()` with a cheaper `fdatasync()`, and add an opt-in `OpenOptions::synchronous` mode that skips per-write flushing entirely for bulk loaders, while `checkpoint()` stays a hard durability boundary in both modes.

**Architecture:** A new `SyncMode { Full, Normal }` enum lives in `src/db.rs` next to `OpenOptions` (public API surface) and is re-exported at the crate root. `OpenOptions` gains a `synchronous: SyncMode` field (default `Full`), threaded through to `WalWriter` at construction. `WalWriter` stores the mode and branches on it in `append_entry`: `Full` calls `sync_data()` (fdatasync — replaces the old `sync_all()`), `Normal` skips the flush call entirely. No change to `checkpoint()`'s own fsync behavior.

**Tech Stack:** Rust, `std::fs::File::sync_data()`/`sync_all()`, existing `anyhow::Result` error plumbing.

**Spec:** `docs/superpowers/specs/2026-08-24-wal-sync-mode-design.md`

## Global Constraints

- Default (`SyncMode::Full`) must preserve exactly the current behavior for every existing caller, including those using `..Default::default()` or the chainable builders.
- `checkpoint()` (explicit, auto-threshold, or the clean-close `Drop` in `Inner::drop`, `src/db.rs:234-247`) fsyncs the main file unconditionally in both modes — do not touch that path.
- No third `Off`/never-sync tier, no runtime mode switching after `open()`, no per-`WriteTransaction` override (see spec's Non-goals).
- Testing convention (`CLAUDE.md`): never use `{:?}` debug format of `Result`/`Fact`/`Value`/`EdnValue`/anything transitively containing `Uuid` in `assert!`/`assert_eq!` messages — CodeQL flags it. Use plain string messages, `unwrap()`/`expect()`, or assert on count/bool only.

---

### Task 1: `SyncMode` enum and `OpenOptions::synchronous`

**Files:**
- Modify: `src/db.rs` (enum + struct field + `Default` impl + builder method + one existing test struct literal)
- Test: `src/db.rs` (`#[cfg(test)] mod tests` at the bottom of the same file — this codebase keeps `db.rs` unit tests inline)

**Interfaces:**
- Produces: `pub enum SyncMode { Full, Normal }` (derives `Debug, Clone, Copy, PartialEq, Eq`), `OpenOptions::synchronous: SyncMode`, `OpenOptions::synchronous(self, mode: SyncMode) -> Self` builder method. Tasks 2 and 3 consume this type and field.

- [ ] **Step 1: Write the failing test**

Add near `test_open_with_options_custom_threshold` (around `src/db.rs:1584`):

```rust
    #[test]
    fn test_sync_mode_defaults_to_full() {
        assert_eq!(OpenOptions::default().synchronous, SyncMode::Full, "default sync mode");
    }

    #[test]
    fn test_sync_mode_builder_sets_normal() {
        let opts = OpenOptions::new().synchronous(SyncMode::Normal);
        assert_eq!(opts.synchronous, SyncMode::Normal, "builder should set Normal");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib test_sync_mode_defaults_to_full test_sync_mode_builder_sets_normal`
Expected: FAIL — `SyncMode` and `OpenOptions::synchronous` do not exist yet (compile error).

- [ ] **Step 3: Add the `SyncMode` enum**

In `src/db.rs`, insert immediately before the `// ─── OpenOptions ─────...` comment (currently just above `pub struct OpenOptions`):

```rust
// ─── SyncMode ────────────────────────────────────────────────────────────────

/// Controls how aggressively WAL writes are flushed to disk.
///
/// Independent of [`Minigraf::checkpoint`], which always fsyncs the main
/// `.graph` file regardless of this setting — see [`OpenOptions::synchronous`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncMode {
    /// `fdatasync` after every WAL write. Default; matches Minigraf's
    /// behavior before this option existed — every committed `transact`/
    /// `retract` is durable immediately.
    Full,
    /// No per-write flush. The WAL entry is still `write_all()`'d — safe
    /// across an ordinary process crash — but not forced to disk until the
    /// next checkpoint (auto-threshold, explicit `checkpoint()`, or clean
    /// close). Data written since the last checkpoint is lost only on OS
    /// crash or power loss, not process death. Intended for bulk loaders
    /// that can safely re-run from a checkpoint watermark on failure.
    Normal,
}
```

- [ ] **Step 4: Add the field, `Default`, and builder method**

In `struct OpenOptions`, add after the `allow_unlocked` field (`src/db.rs:100`, right before the closing `}`):

```rust
    /// Controls WAL write durability. See [`SyncMode`]. Defaults to `SyncMode::Full`.
    pub synchronous: SyncMode,
```

In `impl Default for OpenOptions` (`src/db.rs:103-113`), add to the struct literal:

```rust
            allow_unlocked: false,
            synchronous: SyncMode::Full,
```

In `impl OpenOptions`, add a builder method after `allow_unlocked` (`src/db.rs:150-154`):

```rust
    /// Set the WAL durability mode. See [`SyncMode`].
    #[must_use]
    pub fn synchronous(mut self, mode: SyncMode) -> Self {
        self.synchronous = mode;
        self
    }
```

- [ ] **Step 5: Fix the pre-existing struct-literal test that now fails to compile**

`test_open_with_options_custom_threshold` (`src/db.rs:1589`) builds `OpenOptions` as a full struct literal without `..Default::default()`, so it needs the new field added explicitly:

```rust
        let opts = OpenOptions {
            wal_checkpoint_threshold: 5,
            page_cache_size: 256,
            max_derived_facts: 100_000,
            max_results: 1_000_000,
            allow_unlocked: false,
            synchronous: SyncMode::Full,
        };
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test --lib test_sync_mode_defaults_to_full test_sync_mode_builder_sets_normal test_open_with_options_custom_threshold`
Expected: PASS (3 tests)

- [ ] **Step 7: Commit**

```bash
git add src/db.rs
git commit -m "feat: add SyncMode enum and OpenOptions::synchronous field"
```

---

### Task 2: Thread `SyncMode` through `WalWriter`, swap `sync_all()` for `sync_data()`

**Files:**
- Modify: `src/wal.rs`
- Test: `src/wal.rs` (`#[cfg(all(test, not(target_arch = "wasm32")))] mod tests` at the bottom)

**Interfaces:**
- Consumes: `crate::db::SyncMode` (from Task 1).
- Produces: `WalWriter::open_or_create(path: &Path, sync_mode: SyncMode) -> Result<Self>` (signature change — was `open_or_create(path: &Path)`). Task 3 calls this with the new signature.

- [ ] **Step 1: Write the failing test**

Add near the end of the `tests` module in `src/wal.rs` (after `test_wal_single_fact_round_trip`, around line 394):

```rust
    #[test]
    fn test_wal_normal_mode_write_and_replay() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.wal");

        let alice = Uuid::new_v4();
        let fact = make_fact(alice, ":name", Value::String("Alice".to_string()), 1);

        // Normal mode skips the per-write flush; the entry must still be
        // durable-within-process (write_all()'d) and correctly replayable.
        let mut writer = WalWriter::open_or_create(&path, SyncMode::Normal).unwrap();
        writer.append_entry(1, std::slice::from_ref(&fact)).unwrap();

        let mut reader = WalReader::open(&path).unwrap();
        let entries = reader.read_entries().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].tx_count, 1);
        assert_eq!(entries[0].facts.len(), 1);
        assert_eq!(entries[0].facts[0].entity, fact.entity);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib test_wal_normal_mode_write_and_replay`
Expected: FAIL — `open_or_create` doesn't take a second argument yet (compile error).

- [ ] **Step 3: Add the import**

At the top of `src/wal.rs`, add to the `use` block (after `use crate::graph::types::Fact;`):

```rust
use crate::db::SyncMode;
```

- [ ] **Step 4: Add the `sync_mode` field to `WalWriter`**

Change (around `src/wal.rs:143-146`):

```rust
pub struct WalWriter {
    file: File,
}
```

to:

```rust
pub struct WalWriter {
    file: File,
    sync_mode: SyncMode,
}
```

- [ ] **Step 5: Update `open_or_create` to accept and store the mode**

Change the signature and both `Ok(WalWriter { file })` construction sites inside it (`src/wal.rs:152-172`):

```rust
    pub fn open_or_create(path: &Path, sync_mode: SyncMode) -> Result<Self> {
        // Try atomic create-new first (no TOCTOU window)
        match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path)
        {
            Ok(mut file) => {
                write_wal_header(&mut file)?;
                file.seek(SeekFrom::End(0))?;
                return Ok(WalWriter { file, sync_mode });
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.into()),
        }

        // File exists — validate its header and seek to end for appending
        let mut file = OpenOptions::new().read(true).write(true).open(path)?;
        match check_wal_header_length(&mut file)? {
            WalHeaderState::Present => validate_wal_header(&mut file)?,
            // A previous crash landed between this file's creation and its
            // header write completing; no entry could have been appended
            // yet, so re-initialize it as a fresh, empty WAL.
            WalHeaderState::Absent => write_wal_header(&mut file)?,
        }
        file.seek(SeekFrom::End(0))?;
        Ok(WalWriter { file, sync_mode })
    }
```

- [ ] **Step 6: Branch on `sync_mode` in `append_entry`**

Change (`src/wal.rs:186-191`):

```rust
    pub fn append_entry(&mut self, tx_count: u64, facts: &[Fact]) -> Result<()> {
        let entry_bytes = serialize_entry(tx_count, facts)?;
        self.file.write_all(&entry_bytes)?;
        self.file.sync_all()?;
        Ok(())
    }
```

to:

```rust
    pub fn append_entry(&mut self, tx_count: u64, facts: &[Fact]) -> Result<()> {
        let entry_bytes = serialize_entry(tx_count, facts)?;
        self.file.write_all(&entry_bytes)?;
        match self.sync_mode {
            SyncMode::Full => self.file.sync_data()?,
            SyncMode::Normal => {}
        }
        Ok(())
    }
```

Also update the doc comment directly above `append_entry` — it currently reads "Serialize `facts` as a WAL entry and append it to the file, then fsync." Change the second sentence to: "Then flushes to disk, unless `sync_mode` is `SyncMode::Normal` — see [`SyncMode`]."

- [ ] **Step 7: Update all existing test call sites to pass `SyncMode::Full`**

Every existing test in this file constructs a `WalWriter` expecting the pre-change (always-fsync) behavior, so they all take `SyncMode::Full` explicitly. Run this from the worktree root to apply the mechanical substitution:

```bash
sed -i 's/WalWriter::open_or_create(&path)/WalWriter::open_or_create(\&path, SyncMode::Full)/g' src/wal.rs
```

Verify it changed exactly the 10 pre-existing call sites (lines 368, 383, 407, 430, 465, 480, 541, 575, 595, 609 before this edit) and left the function definition (`pub fn open_or_create(path: &Path) -> Result<Self> {`) untouched:

```bash
grep -c "WalWriter::open_or_create(&path, SyncMode::Full)" src/wal.rs
```

Expected: `10`

- [ ] **Step 8: Run tests to verify they pass**

Run: `cargo test --lib wal::`
Expected: PASS (all `wal.rs` unit tests, including the new `test_wal_normal_mode_write_and_replay`)

- [ ] **Step 9: Commit**

```bash
git add src/wal.rs
git commit -m "feat: thread SyncMode through WalWriter, swap sync_all for sync_data"
```

---

### Task 3: Thread `opts.synchronous` through both `db.rs` call sites, integration test

**Files:**
- Modify: `src/db.rs`
- Test: `src/db.rs` (inline `mod tests`)

**Interfaces:**
- Consumes: `WalWriter::open_or_create(path: &Path, sync_mode: SyncMode) -> Result<Self>` (Task 2), `OpenOptions::synchronous: SyncMode` (Task 1).

- [ ] **Step 1: Write the failing test**

Add near `test_open_with_options_custom_threshold` in `src/db.rs`:

```rust
    #[test]
    fn test_normal_sync_mode_survives_checkpoint_and_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.graph");

        {
            let db = OpenOptions::new()
                .synchronous(SyncMode::Normal)
                .path(&path)
                .open()
                .unwrap();
            db.execute(r#"(transact [[:alice :name "Alice"]])"#).unwrap();
            db.execute(r#"(transact [[:bob :name "Bob"]])"#).unwrap();
            db.checkpoint().unwrap();
        }

        let db = Minigraf::open(&path).unwrap();
        let facts = db.inner.fact_storage.get_asserted_facts().unwrap();
        assert_eq!(facts.len(), 2, "both facts should survive checkpoint + reopen under Normal sync mode");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib test_normal_sync_mode_survives_checkpoint_and_reopen`
Expected: FAIL — `WalWriter::open_or_create` call sites in `db.rs` don't pass a `SyncMode` yet (compile error, since Task 2 changed the signature).

- [ ] **Step 3: Update the call site in `open_with_options`**

Change (`src/db.rs:344-348`):

```rust
        let wal = if wal_path.exists() {
            Some(WalWriter::open_or_create(&wal_path)?)
        } else {
            None
        };
```

to:

```rust
        let wal = if wal_path.exists() {
            Some(WalWriter::open_or_create(&wal_path, opts.synchronous)?)
        } else {
            None
        };
```

- [ ] **Step 4: Update the call site in `wal_write_stamped_batch`**

Change (`src/db.rs:1124-1127`):

```rust
                if wal.is_none() {
                    let wal_path = Minigraf::wal_path_for(db_path);
                    *wal = Some(WalWriter::open_or_create(&wal_path)?);
                }
```

to:

```rust
                if wal.is_none() {
                    let wal_path = Minigraf::wal_path_for(db_path);
                    *wal = Some(WalWriter::open_or_create(&wal_path, opts.synchronous)?);
                }
```

(`opts: &OpenOptions` is already a parameter of `wal_write_stamped_batch` — no further plumbing needed.)

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib db::`
Expected: PASS (all `db.rs` unit tests, including the new one)

- [ ] **Step 6: Commit**

```bash
git add src/db.rs
git commit -m "feat: thread OpenOptions::synchronous into WalWriter construction"
```

---

### Task 4: Re-export `SyncMode` at the crate root

**Files:**
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes: `pub enum SyncMode` (Task 1).
- Produces: `minigraf::SyncMode` reachable at the crate root, matching how `OpenOptions` is already exposed.

- [ ] **Step 1: Write the failing test**

There's no existing doc-test file for root re-exports in this codebase, so this is verified by a doc-test embedded directly in `src/lib.rs`'s module doc comment area — add this compile-checked snippet near the existing `pub use db::{Minigraf, OpenOptions, WriteTransaction};` line (`src/lib.rs:92`) as a doc comment on that same line:

```rust
/// Re-exported at the crate root alongside [`OpenOptions`] so
/// `minigraf::SyncMode` is reachable without importing `minigraf::db`.
```

(This is documentation, not a runnable test — the actual verification is the compile step below, since an incorrect re-export is a compile error, not a runtime failure.)

- [ ] **Step 2: Update the re-export**

Change (`src/lib.rs:92`):

```rust
pub use db::{Minigraf, OpenOptions, WriteTransaction};
```

to:

```rust
pub use db::{Minigraf, OpenOptions, SyncMode, WriteTransaction};
```

- [ ] **Step 3: Verify it compiles and is reachable**

Run: `cargo build --lib`
Expected: builds cleanly.

Run: `cargo doc --no-deps 2>&1 | grep -i "error\|warning: unresolved"`
Expected: no output (no broken intra-doc links from the doc comments added in Tasks 1-3, which reference `[\`SyncMode\`]` and `[\`OpenOptions::synchronous\`]`).

- [ ] **Step 4: Commit**

```bash
git add src/lib.rs
git commit -m "feat: re-export SyncMode at the crate root"
```

---

### Task 5: Docs — README pointer and CHANGELOG entries

**Files:**
- Modify: `README.md`
- Modify: `CHANGELOG.md`

**Interfaces:** None — documentation only, no code interfaces produced or consumed. (The wiki's `Architecture.md` and `Performance-Tuning.md` were already updated and pushed in a separate session, ahead of this implementation.)

- [ ] **Step 1: Add a "Durability tuning" pointer to the README's Performance section**

In `README.md`, after the sentence "File-backed databases enforce a maximum fact size of **4 080 serialised bytes** per fact. In-memory databases have no limit." (end of the `## Performance` section, just before `## Contributing`), add:

```markdown

**Durability tuning:** every write is `fsync`'d immediately by default (`SyncMode::Full`). Bulk loaders/migrations that can safely re-run from a checkpoint watermark on failure can trade that for throughput with `OpenOptions::new().synchronous(SyncMode::Normal)` — `checkpoint()` still fsyncs unconditionally in both modes. See the [Performance Tuning wiki page](https://github.com/project-minigraf/minigraf/wiki/Performance-Tuning#configuration-knobs) for the full tradeoff and the write-batching pattern that pairs with it.
```

- [ ] **Step 2: Add a CHANGELOG entry documenting the fix**

In `CHANGELOG.md`, under `## Unreleased` → `### Performance` (the section ending with the `or`/`or-join` short-circuit bullet, immediately before `## v1.2.3 — 2026-08-10`), append:

```markdown

- **WAL writes no longer pay a full `fsync()`, and can skip the per-write flush entirely** (#302): `WalWriter::append_entry` called `File::sync_all()` (fsync) after every write, flushing inode metadata the WAL — a pure-append file — never needed alongside the data. It now calls `File::sync_data()` (fdatasync) instead: same durability guarantee, one fewer metadata flush, for every existing caller, no other change required. A new `OpenOptions::synchronous` field (`SyncMode::Full`, the default, matches this fdatasync behavior; `SyncMode::Normal` skips the per-write flush altogether) lets bulk loaders/migrations that can safely re-run from a checkpoint watermark trade per-write durability for throughput — `checkpoint()` still fsyncs the main file unconditionally in both modes, so it remains the hard durability boundary regardless of `synchronous`. Separately: `begin_write()` + N `execute()` + one `commit()` already collapsed N facts into a single WAL write / single fsync before this change; that existing batching pattern is now documented (README, wiki Performance Tuning page) since it was easy to miss.
```

- [ ] **Step 3: Add a "Breaking-ish changes" bullet for the new `OpenOptions` field**

In `CHANGELOG.md`, under `## Unreleased` → `### Breaking-ish changes`, immediately after the existing `allow_unlocked` bullet (the one starting "**Adding `OpenOptions::allow_unlocked` breaks struct-literal construction.**"), append a new bullet:

```markdown
- **`OpenOptions` gains a second post-1.0 field: `synchronous`** (#302). Same semver consequence as `allow_unlocked` above — a struct-literal `OpenOptions { .. }` built without `..Default::default()` no longer compiles. One in-tree literal (`src/db.rs`, a test) needed updating; every other in-tree construction already used the spread form or the chainable builder.
```

- [ ] **Step 4: Verify markdown renders sensibly**

Run: `grep -c "^## " CHANGELOG.md` before and after — should be unchanged (no accidental heading-level edits). Read back both diffs with `git diff README.md CHANGELOG.md` and confirm the new text lands in the right sections with no stray heading markers.

- [ ] **Step 5: Commit**

```bash
git add README.md CHANGELOG.md
git commit -m "docs: document SyncMode in README and CHANGELOG"
```

---

### Task 6: Full verification

**Files:** None modified — verification only.

**Interfaces:** None.

- [ ] **Step 1: Format check**

Run: `cargo fmt --check`
Expected: no output (clean).

- [ ] **Step 2: Clippy**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: no warnings/errors.

- [ ] **Step 3: Full test suite**

Run: `cargo test`
Expected: all tests pass, including every test added in Tasks 1-3.

- [ ] **Step 4: Confirm the #308 crash-safety regression tests are unaffected**

The default `SyncMode::Full` must preserve exactly the fsync behavior those tests depend on.

Run: `cargo test --test crash_kill_test`
Expected: PASS, unchanged from before this branch.

- [ ] **Step 5: Confirm the doc comment cross-references resolve**

Run: `cargo doc --no-deps 2>&1 | grep -i "warning"`
Expected: no new warnings beyond whatever pre-existing warnings the baseline had (compare against `git stash` output if unsure — but do not actually stash; if in doubt, note any warning text and cross-check it isn't newly introduced by `[\`SyncMode\`]` / `[\`OpenOptions::synchronous\`]` links).

- [ ] **Step 6: Review the full diff**

Run: `git diff origin/main --stat` and `git log --oneline origin/main..HEAD`
Expected: touches only `src/db.rs`, `src/wal.rs`, `src/lib.rs`, `README.md`, `CHANGELOG.md`, plus the spec/plan docs already committed. Five feature/doc commits (Tasks 1-5) plus the earlier spec commit — six total.
