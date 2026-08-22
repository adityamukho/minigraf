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
