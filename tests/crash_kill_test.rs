//! #308 regression: a `SIGKILL` during an active `transact`/`checkpoint`
//! must never permanently corrupt the database (header checksum mismatch,
//! no recovery path).
//!
//! Two fixes landed together, both closing the same class of bug -- a
//! multi-step on-disk state transition that a kill could land in the
//! middle of:
//! - `src/storage/backend/file.rs`: `FileBackend::write_page` now
//!   recomputes `header_checksum` every time it persists a `page_count`
//!   bump, so the on-disk header is self-consistent at every single-page
//!   write, not just at the deliberate commits in
//!   `PersistentFactStorage::save`.
//! - `src/wal.rs`: a WAL sidecar file shorter than its own header (a crash
//!   between `create_new` and `write_wal_header` completing) is now
//!   treated as an empty WAL instead of a hard read error, since no entry
//!   could ever have been appended at that point.
//!
//! This test reproduces the original report directly: spawn a child that
//! loops `transact` + `checkpoint()` on a fresh `.graph` file, kill it with
//! a real `SIGKILL` at a random point mid-loop, and confirm a fresh process
//! can still open the file afterward.
//!
//! Round count is controlled by `MINIGRAF_CRASH_KILL_ROUNDS` (default: a
//! small smoke count so this runs unconditionally, cheaply, in the per-PR
//! suite). `.github/workflows/crash-kill.yml` runs a much larger nightly
//! count for statistical confidence, matching the ~26% per-round corruption
//! rate observed in the original report before the fix.
#![cfg(not(target_arch = "wasm32"))]

use minigraf::db::Minigraf;
use std::process::Command;
use std::time::{Duration, Instant};

/// Per-PR default: cheap enough to run on every CI build, but with a ~1 -
/// 0.74^5 ≈ 78% chance of catching a reintroduced bug at the original 26%
/// per-round corruption rate.
const DEFAULT_ROUNDS: usize = 5;

fn rounds() -> usize {
    std::env::var("MINIGRAF_CRASH_KILL_ROUNDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_ROUNDS)
}

/// Cheap, dependency-free jitter for the kill delay. No cryptographic
/// requirement here -- just enough spread across the checkpoint loop's
/// write window to land the kill at varying points mid-`save()`.
fn jitter_millis(seed: u64, max: u64) -> u64 {
    let mut x = seed ^ 0x9E37_79B9_7F4A_7C15;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    x % max.max(1)
}

#[test]
fn sigkill_during_checkpoint_never_permanently_corrupts_file() {
    let exe = std::env::current_exe().expect("test binary path");

    for round in 0..rounds() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("kill.graph");

        let mut child = Command::new(&exe)
            .args(["crash_kill_child_entrypoint", "--exact", "--nocapture"])
            .env("MINIGRAF_KILL_DB", &db_path)
            .spawn()
            .expect("spawn crash-kill child");

        let seed = Instant::now().elapsed().as_nanos() as u64
            ^ (round as u64).wrapping_mul(0x2545_F491_4F6C_DD1D);
        let delay = Duration::from_millis(5 + jitter_millis(seed, 150));
        std::thread::sleep(delay);

        child.kill().expect("SIGKILL crash-kill child");
        let status = child.wait().expect("reap crash-kill child");
        assert!(
            !status.success(),
            "round {round}: child must not exit cleanly before being killed"
        );

        // A fresh handle in a fresh process must always be able to open the
        // file after the kill -- this is the exact guarantee #308 broke.
        let db = match Minigraf::open(&db_path) {
            Ok(db) => db,
            Err(e) => panic!("round {round}: reopen after SIGKILL must not fail: {e}"),
        };
        db.execute("(query [:find ?e :where [?e :k ?v]])")
            .unwrap_or_else(|e| panic!("round {round}: query after reopen must not fail: {e}"));
    }
}

/// Child half of the SIGKILL fuzz loop above: opens `MINIGRAF_KILL_DB` and
/// loops transact + checkpoint until killed. A no-op when the marker
/// environment variable is absent -- same convention as
/// `tests/common::crash_child_entrypoint`.
#[test]
fn crash_kill_child_entrypoint() {
    let Ok(db_path) = std::env::var("MINIGRAF_KILL_DB") else {
        return;
    };
    let db = Minigraf::open(&db_path).expect("child opens db");
    let mut i: u64 = 0;
    loop {
        db.execute(&format!("(transact [[:e{i} :k {i}]])"))
            .expect("child transact");
        db.checkpoint().expect("child checkpoint");
        i += 1;
    }
}
