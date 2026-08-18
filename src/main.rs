use anyhow::{Context as _, Result};
use clap::Parser;
use manuel::{Recording, ReplayResult, record_using_tui, replay_recording};

#[derive(clap::Parser)]
#[command(version, long_about = None)]
struct CliArgs {
    /// Write logs to a file (otherwise, no logs can be shown)
    #[arg(long)]
    log_file: Option<String>,

    #[command(subcommand)]
    command: CliCommands,
}

#[derive(clap::Subcommand)]
enum CliCommands {
    /// Record a terminal session to a file
    Record { file: String },
    /// Replay a saved terminal session and assert that the output matches the record
    Replay { file: String },
}

fn main() -> Result<()> {
    let cli_args = CliArgs::parse();
    if let Some(log_file) = cli_args.log_file {
        simplelog::WriteLogger::init(
            log::LevelFilter::Info,
            simplelog::Config::default(),
            std::fs::File::create(log_file)?,
        )
        .context("Failed to initialize file logger")?;
    }
    match &cli_args.command {
        CliCommands::Record { file } => {
            let recording = record_using_tui();
            if let Ok(Some(recording)) = recording
                && let Err(e) = recording.write_to_file(file)
            {
                return Err(e);
            }
        }
        CliCommands::Replay { file } => {
            let recording = Recording::read_from_file(file)?;
            match replay_recording(recording)? {
                ReplayResult::Match => {
                    println!("✅ Replay matched recording");
                }
                ReplayResult::Mismatch(reason) => {
                    println!("❌ Replay did not match recording: {}", reason);
                }
            }
        }
    }

    Ok(())
}
