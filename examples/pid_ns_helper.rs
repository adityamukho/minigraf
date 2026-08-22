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
                // Hold, then die WITHOUT running Drop.
                //
                // If the test names a release marker, wait for it to appear.
                // A fixed sleep cannot work here: the test must spawn a second
                // container through `sudo unshare` and let it attempt an open,
                // and how long that takes varies wildly between a laptop and a
                // loaded CI runner. A fixed hold that is comfortable locally
                // expires mid-attempt on a slow runner, the second container
                // then succeeds, and the test fails claiming a live holder was
                // not excluded. Letting the parent say when it is done removes
                // the guesswork entirely.
                //
                // The deadline is a backstop, not the mechanism: if the parent
                // dies without writing the marker, this process must not sit on
                // the lock indefinitely.
                if let Ok(marker) = std::env::var("MINIGRAF_NS_RELEASE_MARKER") {
                    let marker = std::path::PathBuf::from(marker);
                    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
                    while !marker.exists() && std::time::Instant::now() < deadline {
                        std::thread::sleep(std::time::Duration::from_millis(20));
                    }
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
