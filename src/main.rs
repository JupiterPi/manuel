use anyhow::{Context as _, Result};
use clap::Parser;

#[derive(clap::Parser)]
#[command(version, long_about = None)]
struct CliArgs {
    /// Write logs to a file (otherwise, no logs can be shown)
    #[arg(long)]
    log_file: Option<String>,

    /// The root dir of Manuel recordings (default: tests/manuel_recordings)
    root_dir: Option<String>,
}

fn main() -> Result<()> {
    manuel::assert_bash_available();

    let cli_args = CliArgs::parse();

    if let Some(log_file) = cli_args.log_file {
        simplelog::WriteLogger::init(
            log::LevelFilter::Info,
            simplelog::Config::default(),
            std::fs::File::create(log_file)?,
        )
        .context("Failed to initialize file logger")?;
    }
    log::info!("Manuel v{}", env!("CARGO_PKG_VERSION"));

    manuel::open_manuel_explorer(
        cli_args
            .root_dir
            .unwrap_or("tests/manuel_recordings".to_string()),
    )?;

    Ok(())
}
