use minigraf::{ErrorCategory, Minigraf};

#[test]
fn in_memory_open_returns_ok() {
    let db = Minigraf::in_memory();
    assert!(db.is_ok(), "in_memory() should succeed");
}

#[test]
fn open_nonexistent_directory_returns_coded_error() {
    let bad_path = "/nonexistent-dir-for-minigraf-test-277/db.graph";
    let result = Minigraf::open(bad_path);
    // `Result::expect_err`/`unwrap_err` require the `Ok` type (`Minigraf`) to
    // implement `Debug`, which it deliberately does not (its fields include
    // non-`Debug` function-pointer registries). Extract the error manually.
    let err = match result {
        Err(e) => e,
        Ok(_) => panic!("opening a file in a nonexistent directory should fail"),
    };
    assert_eq!(err.category(), ErrorCategory::Internal);
    assert_eq!(err.code(), "INT-000");
}
