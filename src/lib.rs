//! Manuel is a tool to create, replay and verify terminal sessions.
//! It can be used to create tests for CLI applications the same way you would manually test them.
//!
//! # Examples
//!
//! See main.rs for a example CLI that wraps Manuel.

use std::io::Write;
use std::time::Duration;

use ansi_to_tui::IntoText;
use anyhow::{Context as _, Result};
use ratatui::{
    crossterm,
    layout::{Constraint, Direction, Layout, Size},
    style::Stylize,
    text::{Line, Text},
    widgets::{Block, Borders, Paragraph},
};
use serde::{Deserialize, Serialize};

/// Contains all information necessary to replay a terminal session that was previously recorded.
#[derive(Serialize, Deserialize)]
pub struct Recording {
    terminal_width: u16,
    recording_items: Vec<RecordingItem>,
}

#[derive(Serialize, Deserialize)]
pub enum RecordingItem {
    Output(String),
    Input(Vec<u8>),
}

impl Recording {
    pub(crate) fn new(terminal_width: u16) -> Self {
        Self {
            terminal_width,
            recording_items: Vec::new(),
        }
    }

    pub(crate) fn append_output(&mut self, output: String) {
        if let Some(RecordingItem::Output(last_output)) = self.recording_items.last_mut() {
            last_output.push_str(&output);
        } else {
            self.recording_items.push(RecordingItem::Output(output));
        }
    }
}

impl Recording {
    pub fn read_from_file<P: AsRef<std::path::Path>>(path: P) -> Result<Self> {
        let file = std::fs::File::open(path)?;
        let reader = std::io::BufReader::new(file);
        let recording = serde_yaml::from_reader(reader)?;
        Ok(recording)
    }

    pub fn write_to_file(&self, path: &str) -> Result<()> {
        let file = std::fs::File::create(path)?;
        let mut writer = std::io::BufWriter::new(file);
        writeln!(writer, "# Manuel recording file")?;
        writeln!(writer)?;
        writeln!(
            writer,
            "{}",
            strip_ansi_escapes::strip_str(
                self.recording_items
                    .iter()
                    .filter_map(|item| match item {
                        RecordingItem::Output(output) => Some(output.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("")
            )
            .split("\n")
            .map(|line| format!("# {}", line))
            .collect::<Vec<_>>()
            .join("\n")
        )?;
        writeln!(writer)?;
        serde_yaml::to_writer(writer, self)?;
        Ok(())
    }
}

mod pty {
    use anyhow::Result;

    pub(crate) struct Pty {
        pty_in_tx: std::sync::mpsc::Sender<Vec<u8>>,
        pty_out_rx: std::sync::mpsc::Receiver<String>,
        pty_output: String,
    }

    impl Pty {
        pub(crate) fn new_in_thread(width: u16) -> Self {
            let (pty_out_tx, pty_out_rx) = std::sync::mpsc::channel();
            let (pty_in_tx, pty_in_rx) = std::sync::mpsc::channel::<Vec<u8>>();
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
                cmd.args(["--noprofile", "--norc"]);
                cmd.env("PS1", "\u{1b}[1m\u{1b}[34m(manuel)\u{1b}[0m "); // in blue, bold, with reset afterwards
                cmd.cwd(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string())); // todo
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
                                log::info!("Read {} bytes from PTY", n);
                                match pty_out_tx
                                    .send(String::from_utf8_lossy(&buffer[..n]).into_owned())
                                {
                                    Ok(_) => {}
                                    Err(e) => {
                                        log::error!("Error sending PTY output: {:?}", e);
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            log::error!("Error reading from PTY: {:?}", e);
                        }
                    }
                }
            });

            Self {
                pty_in_tx,
                pty_out_rx,
                pty_output: String::new(),
            }
        }

        /// Reads new output from the PTY and appends it to the internal buffer
        pub(crate) fn get_new_output(&mut self) -> Option<String> {
            let mut new_output = None::<String>;
            while let Ok(content) = self.pty_out_rx.try_recv() {
                match new_output {
                    Some(ref mut new_output) => new_output.push_str(&content),
                    None => new_output = Some(content.clone()),
                };
            }
            if let Some(ref new_output) = new_output {
                self.pty_output.push_str(new_output);
            }
            new_output
        }

        /// Get the total output from the PTY from the internal buffer. Call `get_new_output` first to update the buffer with new output.
        pub(crate) fn get_total_output(&self) -> &str {
            &self.pty_output
        }

        pub(crate) fn send_input(&self, input: Vec<u8>) -> Result<()> {
            self.pty_in_tx.send(input)?;
            Ok(())
        }
    }
}

/// Opens a TUI to record a terminal session.
/// Returns the recording if the user pressed Ctrl+S, or None if the user pressed Ctrl+C.
pub fn record_using_tui() -> Result<Option<Recording>> {
    ratatui::run(|terminal| {
        let terminal_width = terminal
            .size()
            .context("Failed to get terminal size")?
            .width;
        let mut recording = Recording::new(terminal_width);
        let mut pty = pty::Pty::new_in_thread(terminal_width);
        loop {
            if let Some(new_output) = pty.get_new_output() {
                recording.append_output(new_output);
            }

            terminal.draw(|frame| {
                let [layout_title, layout_pty, layout_input] = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints(vec![
                        Constraint::Length(1),
                        Constraint::Fill(1),
                        Constraint::Length(3),
                    ])
                    .areas(frame.area());

                // title
                let layout = layout_title;
                {
                    let title = Paragraph::new(Line::from(vec![
                        "Manuel".bold().green(),
                        " - Capture Snapshot \"Name\"".green(),
                    ]))
                    .centered();
                    frame.render_widget(title, layout);
                }

                // pty
                let layout = layout_pty;
                {
                    let pty_output = match pty.get_total_output().into_text() {
                        Ok(text) => text,
                        Err(e) => {
                            log::error!("Error converting PTY output to text: {:?}", e);
                            Text::from("Error converting PTY output to text")
                        }
                    };
                    let mut scroll_view = tui_scrollview::ScrollView::new(Size::new(
                        layout.width - 1,
                        pty_output.height() as u16,
                    ));
                    scroll_view.render_widget(Paragraph::new(pty_output), scroll_view.area());
                    frame.render_stateful_widget(scroll_view, layout, &mut {
                        let mut scroll_view_state = tui_scrollview::ScrollViewState::default();
                        scroll_view_state.scroll_to_bottom();
                        scroll_view_state
                    });
                }

                // input
                let layout = layout_input;
                {
                    let input_block = Block::default().borders(Borders::ALL);
                    let input_status = Paragraph::new(Line::from(vec![
                        "Type into TTY".green(),
                        " · ".bold(),
                        "Scroll up/down".green(),
                        " · ".bold(),
                        "Ctrl+C to exit".green(),
                        " · ".bold(),
                        "Ctrl+S to save".green(),
                    ]))
                    .centered()
                    .block(input_block);
                    frame.render_widget(input_status, layout);
                }
            })?;
            if crossterm::event::poll(Duration::from_millis(10))? {
                let crossterm_event = crossterm::event::read()?;
                if let crossterm::event::Event::Key(key_event) = crossterm_event
                    && key_event
                        .modifiers
                        .contains(crossterm::event::KeyModifiers::CONTROL)
                {
                    if key_event.code == crossterm::event::KeyCode::Char('c') {
                        break Ok(None);
                    } else if key_event.code == crossterm::event::KeyCode::Char('s') {
                        return Ok(Some(recording));
                    }
                }
                let terminput_event = terminput_crossterm::to_terminput(crossterm_event)?;
                let mut buf = [0; 16];
                if let Ok(written) = terminput_event.encode(&mut buf, terminput::Encoding::Xterm) {
                    recording
                        .recording_items
                        .push(RecordingItem::Input(buf[..written].to_vec()));
                    pty.send_input(buf[..written].to_vec())?;
                }
            }
        }
    })
}

pub enum ReplayResult {
    Match,
    Mismatch(String),
}

pub const REPLAY_TIMEOUT: Duration = Duration::from_secs(5); // todo: make higher, configurable, and/or auto-detectable based on recording timestamps

/// Replay a previously recorded terminal session and assert that the output matches the recording.
pub fn replay_recording(recording: Recording) -> Result<ReplayResult> {
    let mut pty = pty::Pty::new_in_thread(recording.terminal_width);
    let mut unmatched_output = String::new();
    let mut recording_items = recording.recording_items;
    if recording_items.is_empty() {
        return Err(anyhow::anyhow!("Recording has no items"));
    }
    let mut current_item = recording_items.remove(0);
    let mut last_output_time = std::time::Instant::now();
    loop {
        match current_item {
            RecordingItem::Input(_) => {
                return Ok(ReplayResult::Mismatch(
                    "Replay has output where recording expected input".to_string(),
                ));
            }
            RecordingItem::Output(ref expected_output) => {
                if unmatched_output.starts_with(expected_output) {
                    unmatched_output = unmatched_output[expected_output.len()..].to_string();
                    if recording_items.is_empty() {
                        return Ok(ReplayResult::Match);
                    } else {
                        current_item = recording_items.remove(0);
                    }
                    while let RecordingItem::Input(input) = current_item {
                        pty.send_input(input)?;
                        if recording_items.is_empty() {
                            return Ok(ReplayResult::Match);
                        } else {
                            current_item = recording_items.remove(0);
                        }
                    }
                }
            }
        }
        if let Some(new_output) = pty.get_new_output() {
            unmatched_output.push_str(&new_output);
            last_output_time = std::time::Instant::now();
        }
        if last_output_time.elapsed() > REPLAY_TIMEOUT {
            return Ok(ReplayResult::Mismatch(format!(
                "Replay timed out after {:?} with unmatched output: {}",
                REPLAY_TIMEOUT, unmatched_output
            )));
        }
    }
}
