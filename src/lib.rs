//! Manuel is a tool to create, replay and verify terminal sessions.
//! It can be used to create tests for CLI applications the same way you would manually test them.
//!
//! See the [README](https://github.com/JupiterPi/manuel) for more information.

pub(crate) mod collections;
pub(crate) mod pty;
pub(crate) mod recordings;

use crate::{
    collections::{Collection, explore_collection_in_tui},
    recordings::replay_recording,
};
use anyhow::{Context as _, Result};
use std::path::Path;

pub fn assert_bash_available() {
    if std::process::Command::new("bash")
        .arg("--version")
        .output()
        .is_err()
    {
        panic!("Bash is not available on this system. Manuel requires Bash to run.");
    }
}

pub enum ReplayResult {
    /// The replayed output matches the recorded output.
    Match,
    /// The replayed output does not match the recorded output.
    /// **Keep in mind** that there might also be a mismatch in the formatting,
    /// which is detected but not reported in the unfomratted output here.
    Mismatch {
        expected_output: String,
        actual_output: String,
    },
    /// An error occurred during the replay process.
    RecordingError(String),
}

/// Replays all Manuel recordings recursively in the given directory.
/// **Panics** if any of the recordings mismatch. Use this in your tests.
pub fn run_manuel_tests_in_dir(dir: impl AsRef<std::path::Path>) {
    assert_bash_available();

    // discover all recordings
    let root_collection =
        Collection::read_from_dir(dir).expect("Failed to read Manuel recordings directory");
    let recordings = root_collection.flat_recordings();

    // replay all recordings
    let mut fail = false;
    for (name, (recording, replay_context)) in recordings {
        match replay_recording(recording, &replay_context)
            .unwrap_or_else(|_| panic!("Failed to replay Manuel recording: {:?}", name))
        {
            ReplayResult::Match => {
                println!(
                    "\u{1b}[32m\u{1b}[1m[OK]\u{1b}[0m Successfully replayed recording: {:?}",
                    name
                );
            }
            ReplayResult::Mismatch { .. } => {
                fail = true;
                println!(
                    "\u{1b}[31m\u{1b}[1m[FAIL]\u{1b}[0m Replay did not match recording: {:?} (open in Manuel to see details)",
                    name
                );
            }
            ReplayResult::RecordingError(reason) => {
                fail = true;
                println!(
                    "\u{1b}[31m\u{1b}[1m[FAIL]\u{1b}[0m Error during replay: {:?}, reason: {}",
                    name, reason
                );
            }
        }
    }
    if fail {
        println!(
            "\u{1b}[31m\u{1b}[1mSome tests failed!\u{1b}[0m Use the Manuel TUI to inspect diffs. Learn more: https://github.com/JupiterPi/manuel"
        );
        panic!("Some Manuel recordings failed to replay. See output for details.");
    }
}

/// Opens a TUI where the user can navigate and create collections, preview and create recordings,
/// and run Manuel tests. Use this via the Manuel CLI application.
pub fn open_manuel_explorer(dir: impl AsRef<std::path::Path>) -> Result<()> {
    assert_bash_available();
    let dir: &Path = dir.as_ref();
    let root_collection =
        Collection::read_from_dir(dir).context("Failed to read Manuel recordings directory")?;
    ratatui::run(|terminal| explore_collection_in_tui(terminal, &root_collection, dir))?;

    Ok(())
}
