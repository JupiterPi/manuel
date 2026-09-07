//! These are not tests for Manuel, but rather a sample of how Manuel can be used to test a CLI application.

#[test]
fn manuel_tests() {
    manuel::run_manuel_tests_in_dir(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/manuel_recordings"),
        false,
        std::time::Duration::from_secs(60),
    );
}
