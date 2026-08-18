//! These are not tests for Manuel, but rather a sample of how Manuel can be used to test a CLI application.

#[test]
fn manuel_tests() {
    run_manuel_tests_in_dir("tests/manuel_recordings");
}

fn run_manuel_tests_in_dir(dir: &str) {
    let dir_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(dir);
    let entries = std::fs::read_dir(&dir_path).expect("Failed to read Manuel recordings directory");
    for entry in entries {
        let entry = entry.expect("Failed to read entry in Manuel recordings directory");
        let path = entry.path();
        if path.is_file() && path.extension().is_some_and(|ext| ext == "yaml") {
            let recording =
                manuel::Recording::read_from_file(&path).expect("Failed to read Manuel recording");
            manuel::replay_recording(recording)
                .unwrap_or_else(|_| panic!("Failed to replay Manuel recording: {:?}", path));
            println!(
                "\u{1b}[32m\u{1b}[1m[OK]\u{1b}[0m Successfully replayed Manuel recording: {:?}",
                path
            );
        }
    }
}
