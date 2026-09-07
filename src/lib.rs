//! Manuel is a tool to create, replay and verify terminal sessions.
//! It can be used to create tests for CLI applications the same way you would manually test them.
//!
//! See the [README](https://github.com/JupiterPi/manuel) for more information.

pub(crate) mod collections;
pub(crate) mod pty;
pub(crate) mod recordings;
pub mod terminal_output;
pub(crate) mod ui;

use crate::{
    collections::{Collection, explore_collection_in_tui},
    recordings::replay_recording,
    terminal_output::TerminalOutput,
};
use anyhow::{Context as _, Result};
use std::{
    collections::HashMap,
    path::Path,
    thread::{self, JoinHandle},
    time::Duration,
};

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
    Mismatch(ReplayMismatch),
    /// An error occurred during the replay process.
    RecordingError(String),
}

pub struct ReplayMismatch {
    pub expected_output: TerminalOutput,
    pub actual_output: TerminalOutput,
    pub reason: ReplayMismatchReason,
}

pub enum ReplayMismatchReason {
    /// The replayed output does not match the recorded output.
    OutputMismatch,
    /// The replayed output timed out before the recording was complete.
    Timeout,
    /// The replayed output exited unexpectedly before the recording was complete.
    UnexpectedExit,
}

impl ReplayMismatchReason {
    // Returns a string representation of the reason for the replay mismatch (e.g. "timed out")
    pub fn as_str(&self) -> &'static str {
        match self {
            ReplayMismatchReason::OutputMismatch => "output mismatched",
            ReplayMismatchReason::Timeout => "timed out",
            ReplayMismatchReason::UnexpectedExit => "exited unexpectedly",
        }
    }
}

/// Replays all Manuel recordings recursively in the given directory.
/// **Panics** if any of the recordings mismatch. Use this in your tests.
/// Will write mismatch diffs to disk, which can be inspected after failed test runs.
pub fn run_manuel_tests_in_dir(
    dir: impl AsRef<std::path::Path>,
    run_consecutively: bool,
    timeout: Duration,
) {
    assert_bash_available();

    // discover all recordings
    let root_collection =
        Collection::read_from_dir(dir).expect("Failed to read Manuel recordings directory");
    let recordings = root_collection.flat_recordings();

    // replay all recordings
    let output_dir = std::env::temp_dir().join("manuel_replay_output");
    std::fs::create_dir_all(&output_dir)
        .expect("Failed to create output directory for Manuel replay");
    let mut thread_handles = HashMap::new();
    let mut fail = false;
    fn wait_for_thread(name: String, handle: JoinHandle<bool>) -> bool {
        let start_time = std::time::Instant::now();
        let mut last_alert = start_time;
        while !handle.is_finished() {
            thread::sleep(Duration::from_millis(10));
            if last_alert.elapsed() > Duration::from_secs(10) {
                println!(
                    "\u{1b}[33m\u{1b}[1m[INFO]\u{1b}[0m Recording has been running for {}s: {:?}",
                    start_time.elapsed().as_secs(),
                    name
                );
                last_alert = std::time::Instant::now();
            }
        }
        handle.join().unwrap()
    }
    for (name, (recording, replay_context)) in recordings {
        let recording_name = name.clone();
        let output_dir = output_dir.clone();
        let thread_handle = std::thread::spawn(move || {
            match replay_recording(recording, &replay_context, timeout).unwrap_or_else(|_| {
                panic!("Failed to replay Manuel recording: {:?}", recording_name)
            }) {
                ReplayResult::Match => {
                    println!(
                        "\u{1b}[32m\u{1b}[1m[OK]\u{1b}[0m Successfully replayed recording: {:?}",
                        recording_name
                    );
                    true
                }
                ReplayResult::Mismatch(mismatch) => {
                    let diff_file_path = recordings::write_mismatch_diff_to_disk(
                        &output_dir,
                        &recording_name,
                        &mismatch,
                    )
                    .expect("Failed to write mismatch diff to disk");
                    println!(
                        "\u{1b}[31m\u{1b}[1m[FAIL]\u{1b}[0m Replay {}: {:?} (diff at {})",
                        mismatch.reason.as_str(),
                        recording_name,
                        diff_file_path.display()
                    );
                    false
                }
                ReplayResult::RecordingError(reason) => {
                    println!(
                        "\u{1b}[31m\u{1b}[1m[FAIL]\u{1b}[0m Error during replay: {:?}, reason: {}",
                        recording_name, reason
                    );
                    false
                }
            }
        });
        if run_consecutively {
            if !wait_for_thread(name, thread_handle) {
                fail = true;
            }
        } else {
            thread_handles.insert(name.clone(), thread_handle);
        }
    }
    for (name, handle) in thread_handles {
        if !wait_for_thread(name, handle) {
            fail = true;
        }
    }
    if fail {
        println!(
            "\u{1b}[31m\u{1b}[1mSome tests failed!\u{1b}[0m Use the Manuel TUI to edit recordings. Learn more: https://github.com/JupiterPi/manuel"
        );
        panic!("Some Manuel recordings failed to replay. See output for details.");
    }
}

/// Opens a TUI where the user can navigate and create collections, preview and create recordings,
/// and run Manuel tests. Use this via the Manuel CLI application.
pub fn open_manuel_explorer(dir: impl AsRef<std::path::Path>, timeout: Duration) -> Result<()> {
    assert_bash_available();
    let dir: &Path = dir.as_ref();
    let root_collection =
        Collection::read_from_dir(dir).context("Failed to read Manuel recordings directory")?;
    ratatui::run(|terminal| explore_collection_in_tui(terminal, &root_collection, dir, timeout))?;

    Ok(())
}
