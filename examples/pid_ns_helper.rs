//! Helper for `tests/pid_namespace_test.rs`. Opens a database and either dies
//! holding the lock, or reports whether the open succeeded.
//!
//! An example rather than a test so it is a standalone binary the test can run
//! under `unshare`.
//!
//! # Why this kills itself
//!
//! The obvious design is for the test to spawn a holder and kill it. That does
//! not survive contact with CI. GitHub's runners restrict unprivileged user
//! namespaces, so the test falls back to `sudo unshare ...`, and Ubuntu's sudo
//! defaults to `use_pty`, which runs the command in a NEW SESSION. Signalling
//! the process we spawned then reaches only `sudo` itself — SIGKILL runs no
//! handler, so nothing is forwarded — and killing the process group misses the
//! holder too, because it is no longer in that group. The holder survives,
//! still holding the flock, and the #317 regression test fails identically
//! whether or not the bug is present.
//!
//! So the holder ends its own life instead. `abort()` is the right instrument:
//! it terminates without unwinding and without running any `Drop`, which is
//! precisely what a SIGKILLed container leaves behind — no checkpoint, no lock
//! release by the library, nothing but whatever the kernel does on its own.
//! That is the exact state #317 is about.

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = &args[1];
    let mode = &args[2];

    match minigraf::Minigraf::open(path) {
        Ok(db) => {
            db.execute(r#"(transact [[:a :name "A"]])"#).ok();
            println!("OPEN_OK");

            if mode == "hold" {
                // Hold for as long as the test asks, then die WITHOUT running
                // Drop. A zero hold means "die as soon as the lock is held",
                // which is what the crashed-predecessor case wants.
                let millis: u64 = std::env::var("MINIGRAF_NS_HOLD_MILLIS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
                if millis > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(millis));
                }
                // Flush stdout first: abort() will not.
                use std::io::Write;
                let _ = std::io::stdout().flush();
                std::process::abort();
            }
        }
        Err(e) => {
            println!("OPEN_ERR {e}");
            std::process::exit(3);
        }
    }
}
