//! Migration matrix tests (#215).
#![cfg(not(target_arch = "wasm32"))]

use minigraf::QueryResult;
use minigraf::db::{Minigraf, OpenOptions};

const PAGE_SIZE: usize = 4096;
const MAGIC_NUMBER: [u8; 4] = *b"MGRF";

fn count_results(r: QueryResult) -> usize {
    match r {
        QueryResult::QueryResults { results, .. } => results.len(),
        _ => 0,
    }
}

/// Serializes `Command::spawn` calls within this test binary.
///
/// `Command::spawn` forks the whole (multi-threaded) test process before
/// exec'ing the child. Until that child reaches `execve` and its
/// close-on-exec descriptors are closed, it transiently holds a duplicate of
/// every flock any other thread's test currently has open on a `.graph`
/// file — which can make an unrelated, concurrent `try_lock()` elsewhere in
/// this binary observe `WouldBlock` for a few microseconds and fail with
/// "Database is locked by another process", even though no other process
/// was ever really involved. Capping this binary to one fork in flight at a
/// time keeps that window rare enough not to flake the suite.
static CRASH_CHILD_SPAWN_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Runs a child process that opens `db_path`, executes `statements`, then dies
/// by `abort()` without running any `Drop`.
///
/// This is a real crash, not a simulated one: the child's file descriptors are
/// closed by the kernel, which releases the file lock exactly as it would for a
/// SIGKILLed container. Forgetting the handle in-process cannot model this — it
/// leaks the `File`, so the lock stays held for the life of the test process.
fn run_crashing_child(
    db_path: &std::path::Path,
    wal_checkpoint_threshold: usize,
    statements: &[&str],
) {
    let exe = std::env::current_exe().expect("test binary path");
    let status = {
        let _guard = CRASH_CHILD_SPAWN_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        std::process::Command::new(exe)
            .args(["crash_child_entrypoint", "--exact", "--nocapture"])
            .env("MINIGRAF_CRASH_DB", db_path)
            .env("MINIGRAF_CRASH_THRESHOLD", wal_checkpoint_threshold.to_string())
            .env("MINIGRAF_CRASH_STMTS", statements.join("\u{1e}"))
            .status()
            .expect("spawn crash child")
    };

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

#[test]
fn v7_round_trip_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db.graph");
    {
        let db = Minigraf::open(&path).unwrap();
        db.execute(r#"(transact [[:e1 :name "Alice"]])"#).unwrap();
        db.checkpoint().unwrap();
    }
    let db2 = Minigraf::open(&path).unwrap();
    let n = count_results(
        db2.execute("(query [:find ?n :where [?e :name ?n]])")
            .unwrap(),
    );
    assert_eq!(n, 1, "v7 round-trip: Alice must survive close/reopen");
}

#[test]
fn v3_empty_migrates_without_error() {
    use std::io::Write;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v3.graph");
    // Build a minimally valid v3 header: magic + version=3 + page_count=1
    // All other fields zero (roots=0 means empty index, fact_count=0).
    let mut page = vec![0u8; PAGE_SIZE];
    page[0..4].copy_from_slice(&MAGIC_NUMBER);
    page[4..8].copy_from_slice(&3u32.to_le_bytes()); // version
    page[8..16].copy_from_slice(&1u64.to_le_bytes()); // page_count = 1
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(&page).unwrap();
    drop(f);
    assert!(
        Minigraf::open(&path).is_ok(),
        "v3 empty file should open without error"
    );
}

#[test]
fn corrupt_magic_fails_loudly() {
    use std::io::Write;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("corrupt.graph");
    let mut page = vec![0u8; PAGE_SIZE];
    page[0..4].copy_from_slice(b"XXXX");
    page[4..8].copy_from_slice(&7u32.to_le_bytes());
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(&page).unwrap();
    let result = Minigraf::open(&path);
    assert!(result.is_err(), "corrupt magic must produce an error");
    let msg = result.err().unwrap().to_string();
    assert!(
        msg.contains("magic") || msg.contains("invalid") || msg.contains("not a"),
        "error message must describe the corrupt magic"
    );
}

#[test]
fn unsupported_version_fails_loudly() {
    use std::io::Write;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("future.graph");
    let mut page = vec![0u8; PAGE_SIZE];
    page[0..4].copy_from_slice(&MAGIC_NUMBER);
    page[4..8].copy_from_slice(&99u32.to_le_bytes());
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(&page).unwrap();
    let result = Minigraf::open(&path);
    assert!(result.is_err(), "unsupported version must produce an error");
}

#[test]
fn wal_replay_after_migration_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("replay.graph");
    {
        let db = Minigraf::open(&path).unwrap();
        db.execute(r#"(transact [[:e1 :color "red"]])"#).unwrap();
        db.checkpoint().unwrap();
    }
    // Crash before checkpoint: no Drop runs, so the WAL is left for the next
    // open to replay.
    run_crashing_child(&path, 1000, &[r#"(transact [[:e2 :color "blue"]])"#]);
    let db3 = Minigraf::open(&path).unwrap();
    let n = count_results(
        db3.execute("(query [:find ?c :where [?e :color ?c]])")
            .unwrap(),
    );
    assert_eq!(n, 2, "WAL replay after checkpoint must be idempotent");
}
