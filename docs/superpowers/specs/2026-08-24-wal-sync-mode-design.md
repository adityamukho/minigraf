# WAL Sync Mode — Design

**Date:** 2026-08-24
**Status:** Approved, not yet implemented
**Issue:** #302 (WAL fsync-per-write is a full fsync with no batching/relaxed-durability escape hatch)
**Depends on:** none. **Blocks:** #309 (lock fairness) — cheaper/skippable per-write fsync directly
shrinks the lock-hold window #309's starvation/timeout numbers were measured against; see
`docs/superpowers/specs/` roadmap note in project memory `project_issue_roadmap.md`.

## Problem

`WalWriter::append_entry` (`src/wal.rs:153-158`) does a full `sync_all()` (fsync, flushes file
data *and* inode metadata) after every WAL write:

```rust
pub fn append_entry(&mut self, tx_count: u64, facts: &[Fact]) -> Result<()> {
    let entry_bytes = serialize_entry(tx_count, facts)?;
    self.file.write_all(&entry_bytes)?;
    self.file.sync_all()?;
    Ok(())
}
```

For high-write-volume callers (bulk ingestion, migration, seeding) issuing many small
`db.execute()` calls, this is dozens of synchronous disk round-trips per logical unit of work.
Discovered while diagnosing event-loop blocking in `temporal-reasoning`'s git-history ingestion,
which issues ~20-25 separate `execute()`/`checkpoint()` calls per commit.

Three independent improvements, increasing risk/leverage:

1. `sync_all()` → `sync_data()` (fdatasync): skips the inode-metadata flush `sync_all()` also
   does. The WAL is pure-append, so no metadata beyond file length (which `sync_data()` still
   guarantees) matters here. Near-zero risk, no format change.
2. `WriteTransaction::commit()` (`src/db.rs:1050-...`) already collapses every fact staged via
   `begin_write()` + N `execute()` calls into one `tx_count` and one WAL write / one fsync. Callers
   issuing N bare `db.execute()` calls for unrelated facts (not hitting the #287 same-ident-batch
   bug) can switch to the batched form and cut N fsyncs to 1 today, with no library change. This
   is undocumented and easy to miss.
3. An opt-in relaxed-durability mode (SQLite `PRAGMA synchronous=NORMAL` equivalent): skip the
   per-write fsync entirely, relying on the OS page cache and the next checkpoint (auto-threshold,
   explicit, or clean-close) for durability. Biggest lever, real tradeoff — must be explicit
   opt-in since the default is "every transact/retract is fsync-durable immediately."

## Decisions

| # | Decision |
|---|---|
| 1 | Ship all three fixes in this issue (not split #3 into its own issue) |
| 2 | `sync_all()` → `sync_data()` applies unconditionally, in both durability modes |
| 3 | New mode is a two-value enum `SyncMode { Full, Normal }` — no third "never sync" tier |
| 4 | Exposed as `OpenOptions::synchronous: SyncMode`, open-time only, default `Full` |
| 5 | `checkpoint()` (explicit, auto-threshold, or clean-close `Drop`) is unaffected in either mode — it remains the hard durability boundary |
| 6 | Document the `begin_write()`/commit()-batches-N-facts pattern (fix #2) alongside the new mode |

### Why an enum on `OpenOptions`, not a runtime-toggleable method or per-transaction override

`OpenOptions` already holds every other tunable that affects durability/performance trade-offs
(`wal_checkpoint_threshold`, `page_cache_size`), all fixed for the handle's lifetime and set at
`open()`. A `db.set_synchronous(mode)` method or a per-`WriteTransaction` override would add a
mutable-mode or per-call decision surface with no requester and no precedent in this codebase.
YAGNI: nothing in #302 or the ingestion use case that motivated it needs mid-session switching —
a bulk-loader that wants relaxed durability wants it for the whole load, opened once for that
purpose.

### Why two levels, not three

SQLite's `PRAGMA synchronous` has `OFF`/`NORMAL`/`FULL`. `OFF` never fsyncs, not even at
checkpoints — durable only on clean shutdown or explicit flush, with a much larger and
harder-to-document crash-loss window. Nothing in #302 asks for this, and Minigraf's `checkpoint()`
is already positioned as *the* durability boundary (see below) — adding a mode that skips even
that boundary contradicts the "every feature must be crash-safe" default in `PHILOSOPHY.md` §6
without a concrete use case driving it. `Full`/`Normal` covers the actual ask.

### Why `checkpoint()` stays a hard boundary in both modes

`Minigraf::do_checkpoint` → `PersistentFactStorage::save()` already calls `sync_all()` on the main
`.graph` file (`src/storage/backend/file.rs:424`) unconditionally. This is what bounds `Normal`
mode's risk window to "since the last checkpoint" rather than "forever": auto-checkpoint
(`wal_checkpoint_threshold`), an explicit `checkpoint()` call, or the best-effort checkpoint in
`Inner::drop` (`src/db.rs:234-247`, skipped only by the `usize::MAX` sentinel) all still fsync the
main file. A crash between two checkpoints in `Normal` mode loses only the WAL-buffered writes
since the last one — an OS crash or power loss, not an ordinary process crash, since the WAL bytes
are still `write_all()`'d to the OS page cache regardless of mode and become durable on the next
fsync by *any* process, not just the writer.

## Design

### `SyncMode`

```rust
/// Controls how aggressively WAL writes are flushed to disk.
///
/// Independent of `checkpoint()`, which always fsyncs the main file regardless
/// of this setting — see `OpenOptions::synchronous`.
pub enum SyncMode {
    /// fsync (via `fdatasync`) after every WAL write. Default; matches
    /// Minigraf's behavior before this option existed.
    Full,
    /// No per-write fsync. The WAL entry is still `write_all()`'d — safe across
    /// an ordinary process crash — but not flushed to disk until the next
    /// checkpoint (explicit, threshold-triggered, or on clean close). Data
    /// written since the last checkpoint is lost only on OS crash or power
    /// loss, not on process death. Intended for bulk loaders/migrations that
    /// can safely re-run from a checkpoint watermark on failure.
    Normal,
}
```

Added to `OpenOptions`:

```rust
pub struct OpenOptions {
    // ...existing fields...
    /// See [`SyncMode`]. Defaults to `Full`.
    pub synchronous: SyncMode,
}
```

`Default for OpenOptions` sets `synchronous: SyncMode::Full` — unchanged behavior for every
existing caller, including those using `..Default::default()` or the chainable builders.

### Plumbing

- `WalWriter` gains a `sync_mode: SyncMode` field, set in `open_or_create` (both call sites in
  `db.rs`: initial `open()` at `db.rs:345` and the lazy re-create after checkpoint at
  `db.rs:1126` need the mode threaded through — both currently call
  `WalWriter::open_or_create(&wal_path)` with just a path).
- `append_entry` branches on `self.sync_mode`: `Full` → `self.file.sync_data()`, `Normal` → no
  sync call.
- `SyncMode` is defined in `src/db.rs` next to `OpenOptions` (public API surface), used by
  `src/wal.rs`.

### Semver

`OpenOptions` already has all-public fields and is not `#[non_exhaustive]`; a field addition here
rides the same pending semver bump as `allow_unlocked` (project memory:
`constraint_openoptions_semver_break`). No new decision needed — the version-bump session already
tracks this class of change; this issue adds one more field to that same bucket.

### Docs (fix #2)

Add a "Durability tuning" section (README + rustdoc on `OpenOptions::synchronous`) covering both
levers together:

- Batch unrelated writes into one `begin_write()` + N `execute()` + one `commit()` to collapse N
  fsyncs into 1, at the default `Full` durability — no tradeoff, just an underused existing
  feature.
- Set `synchronous: SyncMode::Normal` for bulk-loaders that can tolerate re-running from a
  checkpoint watermark on OS-crash/power-loss, in exchange for skipping the per-write fsync
  entirely.

## Testing

- `wal.rs` unit tests: `Normal`-mode entries are still written and correctly replayed on reopen.
  In-process tests can't observe whether `sync_data()`/`sync_all()` was actually called or skipped
  — the durability *trade-off* is not unit-testable without OS-crash injection — so these tests
  cover correctness of the write/replay path under both modes, not the fsync behavior itself.
- `db.rs` integration test: open with `synchronous: Normal`, write several transactions,
  `checkpoint()`, reopen — facts present. Confirms the code path doesn't break correctness.
- Regression: existing WAL/crash-safety tests (the #308-era SIGKILL corruption tests) must keep
  passing unmodified, since they exercise the unchanged `Full` default.
- No new test asserts `sync_data()` specifically is called in `Full` mode — no observable
  in-process difference from `sync_all()`; covered by "no behavior change" rather than a direct
  assertion.

## Non-goals

- No `OFF`/never-sync tier (see Decisions above).
- No runtime mode switching after `open()`.
- No per-`WriteTransaction` durability override.
- No change to `checkpoint()`'s own fsync behavior — it stays unconditional in both modes.
