# Kernel File Locking — Design

**Date:** 2026-08-22
**Status:** Approved, not yet implemented
**Issues:** #317 (bricked `.graph` file after pod restart), #304 (two handles, one file, one process)
**Supersedes:** PR #318 (closed unmerged — mechanism unsound across PID namespaces)

## Problem

`FileLock` (`src/storage/backend/file.rs`) implements advisory locking with a PID
sidecar: `acquire` writes `std::process::id()` into `<db>.graph.lock`, created with
`create_new`, and deletes it on `Drop`.

Two defects follow from using a PID as the liveness token.

**#317.** A container killed with SIGKILL never runs `Drop`, so the sidecar survives
holding the value `1`. The replacement container is also PID 1 in its own namespace.
`acquire` reads `1`, compares it against `our_pid`, takes the `pid == our_pid` branch,
and refuses. The refusal is permanent: nothing in the code path can ever clear it. The
database is unopenable until an operator deletes the file by hand.

Reproduced in ~3 seconds with `unshare --user --map-root-user --pid --fork --mount-proc`,
no container runtime required. Container A opens and is SIGKILLed; the sidecar is left
holding `1`; container B is refused with the #304 same-process error text.

**#304.** A second `FileBackend` on one file within one process gives each its own
cached `header.page_count`. The two page tables diverge, producing intermittent
`Page N out of bounds (total pages: M)`. Whichever handle drops first deletes the lock
file belonging to the survivor. The current code addresses this by refusing when
`pid == our_pid` — which is exactly what makes #317 unrecoverable.

## Root cause

A PID is not a liveness token. It is not unique across PID namespaces, and it is
recycled within one. Any scheme that infers "the holder is alive" from a PID is
unsound wherever namespaces exist, which today means anywhere containers run.

PR #318 proposed comparing `/proc/<pid>/stat` start times. This was rejected: across
PID namespaces `/proc/<pid>` does not refer to the holder at all, so a differing start
time proves only "the holder is not the process I can see", never "the holder is dead".
It converts a safe refusal into silent cross-process corruption. See the review on #318.

## Decisions

| # | Decision |
|---|---|
| 1 | Use `std::fs::File::try_lock` — the kernel is the source of truth |
| 2 | Fail closed when locking is unsupported, with an `allow_unlocked` opt-in |
| 3 | Delete the sidecar; lock the `.graph` fd directly; ignore leftovers |
| 4 | Declare `rust-version = "1.89"` |
| 5 | Simulate crashes with real subprocesses; ship no test hook |
| 6 | Merge to main with no version bump; CHANGELOG records the semver facts |

### Why std rather than a crate or hand-rolled FFI

`File::try_lock`/`unlock` stabilised in Rust 1.89 (`#[stable(feature = "file_lock",
since = "1.89.0")]`). It uses `flock` on Unix and `LockFileEx` on Windows, both
maintained by the Rust project.

Measured, not assumed:

- Both `flock` and `F_OFD_SETLK` deny a same-process second lock (`EWOULDBLOCK`),
  because they attach to the open file description rather than the process. Verified
  directly. **The kernel lock therefore fixes #304 as well**, with no bookkeeping.
- CI's binary-size gate stands at 1006 KiB against a 1024 KiB limit — **18 KiB of
  headroom**. `fs4` would add `rustix`, `bitflags` and `linux-raw-sys` on Linux plus
  `windows-sys` on Windows, for an unmeasured delta against that margin. std adds zero.
- `fs4`'s own documentation describes its methods as mirroring the std ones.

SQLite hand-rolls all of its locking across ~8,000 lines of `os_unix.c` with five
selectable styles, but for reasons that do not apply here: it needs byte-range
granularity to encode SHARED/RESERVED/PENDING/EXCLUSIVE at distinct offsets, which
`flock` cannot express. Its `unixInodeInfo` table exists solely to paper over POSIX
`fcntl` locks being per-process. We have one whole-file exclusive lock and a primitive
that is already per-description, so neither complication applies.

`edition = "2024"` already implies Rust 1.85, so the MSRV moves four releases, not from
an ancient baseline.

## Design

### Locking

`FileLock`, `is_process_alive`, the sidecar, and the unbounded recursion in `acquire`
are all deleted. `FileBackend` already holds an open `File` on the `.graph` file; the
lock moves onto that fd. No `Drop` impl is needed — closing the file releases the lock,
and so does process death. That is the entirety of the #317 fix.

`FileBackend::open` classifies `file.try_lock()`:

| Result | Meaning | Action |
|---|---|---|
| `Ok(())` | lock held | proceed |
| `Err(WouldBlock)` | another open file description holds it | refuse |
| `Err(Error(e))` | filesystem cannot lock | refuse unless `allow_unlocked` |

The classification is a pure function so the unsupported-filesystem branch is testable
without an exotic filesystem:

```rust
enum LockOutcome { Acquired, Held, Unsupported(std::io::Error), ProceedUnlocked }

fn classify(r: Result<(), TryLockError>, allow_unlocked: bool) -> LockOutcome
```

### Same-process diagnostics

`WouldBlock` does not identify the holder, but the #304 message is worth preserving —
the code comment records that generic "another process" wording sent that investigation
looking for a second process for a long time.

A process-global `Mutex<HashSet<PathBuf>>` of canonicalised open paths, inserted on
success and removed on drop, selects between the same-process and other-process
message. This is **purely diagnostic**; correctness comes from the kernel. If
`canonicalize` fails, fall back to the generic message.

### `allow_unlocked`

`OpenOptions` exposes both public fields and chainable builder methods (see
`page_cache_size`), so this needs both, to match the existing pattern:

```rust
pub allow_unlocked: bool,                                   // default: false
pub fn allow_unlocked(self, allow: bool) -> Self;           // builder
```

Consulted **only** when locking is unsupported. It never overrides a live `WouldBlock`
— doing so would reintroduce the corruption the lock exists to prevent. Enforce this in
`classify`, not by convention.

### Errors

Three distinct messages, all new to `docs/ERROR_REFERENCE.md`, which currently
documents none:

1. **Same-process** — retains the #304 explanation and the advice to clone the handle.
2. **Another process** — names the path; no PID, since the kernel does not tell us one
   and a guess is what caused #317.
3. **Unsupported filesystem** — names the underlying errno and points at
   `allow_unlocked`, stating the risk it accepts.

### Platform differences

**Windows locks are mandatory**, not advisory. While a database is open, other
processes' reads of the `.graph` file will fail — so copying or backing up an open
database stops working on Windows, where today it succeeds. On Unix the lock stays
advisory and nothing changes. Documented rather than worked around; it also prevents
torn backups.

**NFS.** Linux emulates `flock` via `fcntl` since 2.6.37, which works on NFSv4 but
needs `lockd` on NFSv3. The failure mode that matters is a lock that silently no-ops
rather than erroring. Tier 2 testing is designed specifically to detect this — but it is deferred to
issue #324 and is NOT delivered by this branch, so the risk is unmitigated on
merge. See the delivery note above.

### Compatibility

A leftover `.graph.lock` from v1.2.x is **ignored, not deleted** — deleting it would
break mutual exclusion for a still-running old process. Concurrent access by mixed
versions is unsupported and documented as such: upgrade all writers together.

> **Delivery note.** Tiers 2 and 3 below are design, not delivery. This branch
> ships the core fix and Tier 1 only; the NFS and Docker nightly suites are
> tracked in issue #324. That leaves one risk in this document's own register
> unmitigated on merge: if `flock` silently no-ops on some network filesystem,
> two processes both receive `Ok(())` and both write. Tier 2 exists precisely
> to detect that, and it is not built yet. The trade is still favourable — the
> sidecar it replaces could not evaluate a remote holder at all, and bricked
> the database on any crash — but it should be stated plainly rather than left
> implicit in a linked issue.

## Non-goals

- Shared/read locks. One exclusive lock; the multi-reader model is not in scope.
- Byte-range locking. Nothing here needs sub-file granularity.
- Making `allow_unlocked` bypass a live lock.
- Kubernetes-specific code paths.

## Testing

### In-process

`run_crashing_child()` re-execs the test binary via `current_exe()` with an env marker;
the child opens, writes, and `abort()`s. The kernel releases the lock on death, exactly
as in production. Each of the 14 crash-simulation call sites (13 in `tests/wal_test.rs`, 1 in
`tests/migration_matrix_test.rs`) becomes a one-line call, and `simulate_crashed_holder`
(`tests/wal_test.rs:840`) is deleted.

No test hook ships in the crate. `#[doc(hidden)] pub` would be callable in production,
and `#[cfg(test)]` is invisible to `tests/*.rs` — integration tests link against the
crate built without `cfg(test)`, verified directly. A real subprocess needs neither.

### Tier 1 — PID namespaces, every PR

`tests/pid_namespace_test.rs`, `#[cfg(target_os = "linux")]`, runtime-probing for a
working `unshare` and skipping cleanly when absent. Ubuntu 24.04's
`kernel.apparmor_restrict_unprivileged_userns` may block the unprivileged form; fall
back to `sudo unshare`, which needs no user namespace.

- **#317 regression** — PID-1 child holds, is SIGKILLed, a second PID-1 process opens
  successfully. Fails against today's code, so its catching power is established rather
  than assumed. Carries `Co-Authored-By: Olive Casazza <olive.casazza@schrodinger.com>`.
- **Mutual exclusion holds** — two live PID-1 processes in separate namespaces, the
  second is refused. Guards precisely what PR #318 would have broken.

### Tier 2 — NFS, nightly — NOT DELIVERED BY THIS BRANCH, see issue #324

`nfs-kernel-server` on the runner, loopback export, lock suite over NFSv4 and
NFSv3-without-`lockd`. Asserts a lock is either genuinely exclusive or refused, never a
silent no-op. This is the surface no other tier reaches and the likeliest place for a
real bug in the new code.

### Tier 3 — Docker, nightly — NOT DELIVERED BY THIS BRANCH, see issue #324

Two containers sharing a volume. Mechanically the same path as tier 1, but matches the
reporter's setup literally and yields an artifact worth posting on #317.

Kubernetes (`kind`/`k3s`) was considered and rejected: pod lifecycle reduces to SIGKILL
plus a fresh PID-1 process, which tier 1 already covers, and `kind` provisions
`hostPath` rather than NFS, so it would not exercise the RWX semantics that motivate it.

## Files touched

| File | Change |
|---|---|
| `src/storage/backend/file.rs` | Delete `FileLock`/`is_process_alive`; lock the fd; `classify`; rewrite 3 unit tests |
| `src/db.rs` | `OpenOptions::allow_unlocked` field + docs |
| `Cargo.toml` | `rust-version = "1.89"` |
| `tests/wal_test.rs` | `run_crashing_child`; convert 13 sites; delete `simulate_crashed_holder` |
| `tests/migration_matrix_test.rs` | Convert 1 site |
| `tests/pid_namespace_test.rs` | New — tier 1 |
| `.github/workflows/nfs-lock.yml` | New — tiers 2 and 3. **Deferred to issue #324; not created by this branch.** |
| `CHANGELOG.md` | Unreleased entry recording new API, MSRV bump, behaviour change |
| `docs/ERROR_REFERENCE.md` | Three new lock errors |
| `docs/TEST_COVERAGE.md` | Lines 50, 557, 705 describe the removed idiom |
| `.wiki/Architecture.md` | Lines 205, 231 describe the sidecar lock |

## Risks

| Risk | Mitigation |
|---|---|
| `flock` silently no-ops on some network filesystem | **UNMITIGATED ON MERGE.** Tier 2 is designed for this but is deferred to #324. Fail-closed on error still applies, but a silent no-op returns `Ok(())` and so is not an error we can catch. |
| GitHub runners block unprivileged `unshare` | `sudo unshare` fallback; test skips rather than fails |
| MSRV 1.89 breaks a binding repo | Verify all 7 build on 1.89 before merge |
| Windows mandatory locks break someone's backup flow | Documented as a platform difference in CHANGELOG and wiki |
| Mixed v1.2.x/new writers on one file | Documented unsupported; leftover sidecar left untouched |

## Attribution

Reported and diagnosed by @ocasazza (#317), including the reproduction and the correct
identification of #304 as the reason for the `pid == our_pid` refusal. PR #318 is not
merged, but one of its tests carries over as the tier 1 regression case.
