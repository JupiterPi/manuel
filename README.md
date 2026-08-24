# Manuel, a replay testing tool for Rust CLI applications

Do the manual testing once, and Manuel does it automatically in the future. Originally developed for the [Filen CLI](https://github.com/FilenCloudDienste/filen-rs/tree/main/filen-cli).

## Installation

```bash
# the manuel CLI tool
cargo install --git https://github.com/JupiterPi/manuel
# and the manuel crate for your tests
cargo add --dev --git https://github.com/JupiterPi/manuel
```

## Usage

Use the `manuel` TUI to create recordings.
You can do anything that can be done in a `bash`, and Manuel will record your keystrokes and the output of your commands.

You can also add `.bashrc` files, which will be sourced at the start of recordings in the same and child directories.

Then, run the replays as integration tests in your Rust project:

```rust
// tests/manuel_tests.rs
#[test]
fn manuel_tests() {
    manuel::run_manuel_tests_in_dir(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/manuel_recordings"),
    );
}
```

When a replay fails, because the output mismatches the recording, you can use the `manuel` TUI to review the diff and update the recording if necessary.

There are an example CLI application with some tests in this crate's `tests` directory.
