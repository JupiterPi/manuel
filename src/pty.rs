use std::{fs, path::PathBuf};

use anyhow::{Context as _, Result};

use crate::terminal_output::TerminalOutput;

pub(crate) struct Pty {
    pty_in_tx: std::sync::mpsc::Sender<Vec<u8>>,
    pty_out_rx: std::sync::mpsc::Receiver<Vec<u8>>,
    pty_is_alive: bool,
    output: TerminalOutput,
}

impl Pty {
    pub(crate) fn new_in_thread(width: u16, bashrc_files: &[PathBuf]) -> Result<Self> {
        let (pty_out_tx, pty_out_rx) = std::sync::mpsc::channel();
        let (pty_in_tx, pty_in_rx) = std::sync::mpsc::channel::<Vec<u8>>();
        let tempdir =
            tempfile::tempdir().context("Failed to create temporary directory for PTY")?;
        let bashrc_files = bashrc_files
            .iter()
            .map(fs::canonicalize)
            .collect::<Result<Vec<_>, _>>()?;
        std::thread::spawn(move || {
            let pty_system = portable_pty::native_pty_system();
            let pty = pty_system
                .openpty(portable_pty::PtySize {
                    rows: 10,
                    cols: width,
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .unwrap();
            let mut cmd = portable_pty::CommandBuilder::new("bash");
            cmd.args([
                "--noprofile",
                "--norc",
                "-c",
                &format!(
                    "cd {} && bash --noprofile --rcfile <({}{})",
                    tempdir.path().display(),
                    format!(
                        "export PS1=\"\u{1b}[1m\u{1b}[34m(manuel)\u{1b}[0m \"\n\
                            export MANUEL_CWD={}",
                        std::env::current_dir()
                            .unwrap_or(
                                "err\necho '(!) Failed to get current working directory'".into()
                            )
                            .display()
                    )
                    .split("\n")
                    .map(|line| format!("echo '{}'", line))
                    .collect::<Vec<_>>()
                    .join("; "),
                    if bashrc_files.is_empty() {
                        "".to_string()
                    } else {
                        format!(
                            "; cat {}",
                            bashrc_files
                                .iter()
                                .map(|p| p.display().to_string())
                                .collect::<Vec<_>>()
                                .join(" ")
                        )
                    }
                ),
            ]);
            cmd.env("PS1", "\u{1b}[1m\u{1b}[34m(manuel)\u{1b}[0m "); // in blue, bold, with reset afterwards
            cmd.env("TERM", "xterm-256color"); // make sure ansi control sequences are the same in every environment including CI
            cmd.env("COLORTERM", "truecolor"); // make sure ansi control sequences are the same in every environment including CI
            cmd.cwd(tempdir.path()); // todo
            let _child = pty.slave.spawn_command(cmd).unwrap();
            let mut pty_reader = pty.master.try_clone_reader().unwrap();
            let mut pty_writer = pty.master.take_writer().unwrap();

            std::thread::spawn(move || {
                while let Ok(input) = pty_in_rx.recv() {
                    if let Err(e) = pty_writer.write_all(&input) {
                        log::error!("Error writing to PTY: {:?}", e);
                    }
                }
            });

            loop {
                let mut buffer = [0u8; 1024];
                match pty_reader.read(&mut buffer) {
                    Ok(n) => {
                        if n > 0 {
                            match pty_out_tx.send(buffer[..n].to_vec()) {
                                Ok(_) => {}
                                Err(e) => {
                                    log::warn!("Error sending PTY output: {:?}", e);
                                }
                            }
                        } else {
                            log::info!("PTY closed");
                            break;
                        }
                    }
                    Err(e) => {
                        log::error!("Error reading from PTY: {:?}", e);
                    }
                }
            }
        });

        Ok(Self {
            pty_in_tx,
            pty_out_rx,
            pty_is_alive: true,
            output: TerminalOutput::new(width),
        })
    }

    /// Reads new output from the PTY and appends it to the internal buffer
    pub(crate) fn get_new_output(&mut self) -> Option<Vec<u8>> {
        let mut new_output = None::<Vec<u8>>;
        loop {
            match self.pty_out_rx.try_recv() {
                Ok(content) => match new_output {
                    Some(ref mut new_output) => new_output.extend(content),
                    None => new_output = Some(content),
                },
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.pty_is_alive = false;
                    break;
                }
            }
        }
        if let Some(ref new_output) = new_output {
            self.output.append_output(new_output);
        }
        new_output
    }

    /// Get the total output from the PTY. Call `get_new_output` first to update it with new output.
    pub(crate) fn get_total_output(&self) -> &TerminalOutput {
        &self.output
    }

    pub(crate) fn send_input(&self, input: Vec<u8>) -> Result<()> {
        self.pty_in_tx.send(input)?;
        Ok(())
    }

    pub(crate) fn is_alive(&self) -> bool {
        self.pty_is_alive
    }
}
