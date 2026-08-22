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
        "--kill-child",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();

    let privileged: Vec<String> = [
        "sudo",
        "-n",
        "unshare",
        "--pid",
        "--fork",
        "--mount-proc",
        "--kill-child",
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
fn spawn_in_namespace(
    prefix: &[String],
    helper: &Path,
    db: &Path,
    mode: &str,
) -> std::process::Child {
    use std::os::unix::process::CommandExt;

    Command::new(&prefix[0])
        .args(&prefix[1..])
        .arg(helper)
        .arg(db)
        .arg(mode)
        // Give the child its own process group, so the whole tree beneath it
        // -- privilege wrapper, `unshare`, and the process that becomes PID 1
        // in the new namespace -- can be killed as a unit. See `kill_holder`.
        .process_group(0)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn in namespace")
}

/// Kills a namespaced holder and everything beneath it, then reaps it.
///
/// Killing the spawned process alone is not enough when a privilege wrapper
/// sits between us and `unshare`: SIGKILL runs no handler, so the wrapper
/// forwards nothing and the namespaced holder survives, still holding the
/// flock. That produces a spectacularly misleading failure — the #317
/// regression test fails identically whether or not the bug is present,
/// because the lock genuinely is still held.
///
/// The child leads its own process group (see `spawn_in_namespace`), so
/// signalling the negated PID reaches every descendant. When the prefix
/// escalates privilege, the descendants are root-owned and the signal has to
/// be sent with the same escalation, or it fails with EPERM.
fn kill_holder(prefix: &[String], child: &mut KillOnDrop) {
    let pgid = child.id().to_string();
    let target = format!("-{pgid}");

    let mut kill = if prefix[0] == "sudo" {
        let mut c = Command::new("sudo");
        c.arg("-n").arg("kill");
        c
    } else {
        Command::new("kill")
    };

    let _ = kill
        .arg("-9")
        .arg(&target)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    // Belt and braces: also signal the direct child, in case the process
    // group could not be signalled at all.
    let _ = child.kill();
    let _ = child.wait();
}

/// Kills and reaps the wrapped child on drop.
///
/// `std::process::Child` does not do this itself -- dropping a `Child`
/// leaves the process running if it still is. Both tests below hold a
/// "container A" child across an `assert!` before they get around to an
/// explicit `kill`/`wait`; if that assert (or anything else) panics first,
/// the panic unwinds straight past the explicit cleanup and orphans
/// container A: PID 1 in its own namespace, part-way through a 300-second
/// sleep, still holding the flock. `Deref`/`DerefMut` to `Child` let call
/// sites keep using `.kill()`/`.wait()` exactly as before; this only adds a
/// safety net for the unwind path.
struct KillOnDrop(std::process::Child);

impl std::ops::Deref for KillOnDrop {
    type Target = std::process::Child;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for KillOnDrop {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
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

/// Fails loudly if the helper binary this environment could have run is
/// missing, instead of silently skipping.
///
/// Call this only after `unshare_prefix()` has already returned `Some`: at
/// that point this environment has proven it can create PID namespaces, so
/// the one remaining reason `helper_binary()` could be absent is that
/// `cargo build --examples` was not run -- a build problem the developer
/// should see immediately, not a reason to assert nothing and report green.
/// A skip is reserved for `unshare_prefix()` returning `None`, i.e. this
/// environment genuinely cannot create PID namespaces at all.
///
/// This also guards against the assumption (verified by hand, not encoded
/// anywhere else) that plain `cargo test` builds `examples/` before running
/// integration tests: if a future cargo stops doing that, this test turns
/// red and visible in CI instead of green and vacuous.
fn require_helper_built(helper: &Path) {
    assert!(
        helper.exists(),
        "helper binary not found at {}; run `cargo build --examples` first",
        helper.display()
    );
}

#[test]
fn test_lock_survives_holder_death_in_another_pid_namespace() {
    let Some(prefix) = unshare_prefix() else {
        eprintln!("SKIP: this environment cannot create PID namespaces");
        return;
    };
    let helper = helper_binary();
    require_helper_built(&helper);

    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("shared.graph");

    // Container A: opens the database and holds it. Wrapped so a panic
    // unwinding past the explicit `kill`/`wait` below (e.g. the `assert!`
    // right after this, on a namespace flake or a slow runner) still kills
    // and reaps it instead of orphaning a namespaced holder sitting on the
    // flock for its 300-second sleep.
    let mut a = KillOnDrop(spawn_in_namespace(&prefix, &helper, &db, "hold"));

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
    //
    // `kill()` reaches only the process we spawned. Where `unshare` is the
    // direct child that is enough, because `--kill-child` sets
    // PR_SET_PDEATHSIG on the process that becomes PID 1. Where a privilege
    // wrapper sits in between, killing the wrapper cannot propagate — a
    // SIGKILLed process runs no handler and forwards nothing — so the
    // namespaced holder would survive and keep the lock. Kill the whole
    // process group instead, which reaches every descendant regardless.
    kill_holder(&prefix, &mut a);

    // Container B: a different container, also PID 1 in its own namespace.
    let b = spawn_in_namespace(&prefix, &helper, &db, "open")
        .wait_with_output()
        .expect("container B ran");

    let out = String::from_utf8_lossy(&b.stdout);
    let err = String::from_utf8_lossy(&b.stderr);
    assert!(
        out.contains("OPEN_OK"),
        "a replacement container must be able to open the database after its \
         predecessor was killed; this is #317.\n\
         prefix used: {prefix:?}\n\
         container B stdout: {out}\n\
         container B stderr: {err}"
    );
}

#[test]
fn test_two_live_namespaced_holders_are_still_refused() {
    let Some(prefix) = unshare_prefix() else {
        eprintln!("SKIP: this environment cannot create PID namespaces");
        return;
    };
    let helper = helper_binary();
    require_helper_built(&helper);

    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("contended.graph");

    // See the comment on `KillOnDrop`: `a` must survive the `.expect` below
    // panicking (B fails to spawn/run) without leaking a namespaced holder.
    let mut a = KillOnDrop(spawn_in_namespace(&prefix, &helper, &db, "hold"));
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
    let err = String::from_utf8_lossy(&b.stderr);

    // Cleanup only — B's result is already captured, so an orphan here cannot
    // fail this test. It would still leave a namespaced process sitting on the
    // flock for its full 300-second sleep, which on a shared CI runner is a
    // fine way to break somebody else's test.
    kill_holder(&prefix, &mut a);

    assert!(
        out.contains("OPEN_ERR"),
        "a second live holder in another PID namespace must still be refused.\n\
         prefix used: {prefix:?}\n\
         container B stdout: {out}\n\
         container B stderr: {err}"
    );
}
