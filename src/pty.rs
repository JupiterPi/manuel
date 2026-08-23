use std::{fs, path::PathBuf};

use anyhow::{Context as _, Result};

pub(crate) struct Pty {
    pty_in_tx: std::sync::mpsc::Sender<Vec<u8>>,
    pty_out_rx: std::sync::mpsc::Receiver<Vec<u8>>,
    pty_is_alive: bool,
    vte: vt100::Parser,
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
                    "cd {} && bash --noprofile --rcfile <({}; cat {})",
                    tempdir.path().display(),
                    format!(
                        "export PS1=\"\u{1b}[1m\u{1b}[34m(manuel)\u{1b}[0m \"\n\
                            export CARGO_MANIFEST_DIR={}",
                        env!("CARGO_MANIFEST_DIR")
                    )
                    .split("\n")
                    .map(|line| format!("echo '{}'", line))
                    .collect::<Vec<_>>()
                    .join("; "),
                    bashrc_files
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect::<Vec<_>>()
                        .join(" "),
                ),
            ]);
            cmd.env("PS1", "\u{1b}[1m\u{1b}[34m(manuel)\u{1b}[0m "); // in blue, bold, with reset afterwards
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
                                    log::error!("Error sending PTY output: {:?}", e);
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
            vte: vt100::Parser::new(
                u16::MAX, // todo?
                width - 1,
                0,
            ),
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
            self.vte.process(new_output);
        }
        new_output
    }

    /// Get the total output from the PTY from the internal buffer. Call `get_new_output` first to update the buffer with new output.
    pub(crate) fn get_total_output(&self) -> Vec<u8> {
        let mut output_buffer: Vec<u8> = Vec::new();
        let screen = self.vte.screen();
        let (_, cols) = screen.size();
        let last_non_blank_row_idx = {
            let mut last_non_blank_row_idx = 0;
            let mut number_of_consecutive_blank_rows = 0;
            for (row_idx, row) in screen.rows_formatted(0, cols).enumerate() {
                if !row.is_empty() {
                    last_non_blank_row_idx = row_idx;
                } else {
                    number_of_consecutive_blank_rows += 1;
                }
                if number_of_consecutive_blank_rows > 100 {
                    break;
                }
            }
            last_non_blank_row_idx
        };
        for (row_idx, row) in screen.rows_formatted(0, cols).enumerate() {
            if row_idx > last_non_blank_row_idx {
                break;
            }
            output_buffer.extend(row);
            // rows_formatted generates each row assuming it starts at default
            // attrs, but never resets at the end, so attrs bleed between rows.
            output_buffer.extend(b"\x1b[m\n");
        }
        output_buffer
    }

    pub(crate) fn send_input(&self, input: Vec<u8>) -> Result<()> {
        self.pty_in_tx.send(input)?;
        Ok(())
    }

    pub(crate) fn is_alive(&self) -> bool {
        self.pty_is_alive
    }
}
