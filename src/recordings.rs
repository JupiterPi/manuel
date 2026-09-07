use crate::{
    ReplayMismatch, ReplayMismatchReason, ReplayResult, terminal_output::TerminalOutput, ui,
};
use ansi_to_tui::IntoText as _;
use anyhow::{Context as _, Result};
use ratatui::{
    crossterm,
    layout::{Constraint, Direction, Layout, Size},
    style::Stylize,
    text::{Line, Text},
    widgets::{Block, Borders, Paragraph},
};
use serde::{Deserialize, Serialize};
use std::{io::Write, path::Path};
use std::{path::PathBuf, time::Duration};
use unicode_width::UnicodeWidthStr;

/// Contains all information necessary to replay a terminal session that was previously recorded.
#[derive(Serialize, Deserialize, Clone)]
pub(crate) struct Recording {
    terminal_width: u16,
    recording_items: Vec<RecordingItem>,
}

#[derive(Serialize, Deserialize, Clone)]
pub(crate) enum RecordingItem {
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

    pub(crate) fn get_terminal_width(&self) -> u16 {
        self.terminal_width
    }

    pub(crate) fn get_recording_items(&self) -> &Vec<RecordingItem> {
        &self.recording_items
    }
}

impl Recording {
    pub(crate) fn read_from_file<P: AsRef<std::path::Path>>(path: P) -> Result<Self> {
        let file = std::fs::File::open(path)?;
        let reader = std::io::BufReader::new(file);
        let recording = serde_yaml::from_reader(reader)?;
        Ok(recording)
    }

    pub(crate) fn write_to_file(&self, path: impl AsRef<std::path::Path>) -> Result<()> {
        let file = std::fs::File::create(path)?;
        let mut writer = std::io::BufWriter::new(file);
        writeln!(writer, "# Manuel recording file")?;
        writeln!(writer)?;
        writeln!(
            writer,
            "{}",
            TerminalOutput::from_recording(self)
                .get_emulator_output_stripped()
                .split("\n")
                .map(|line| format!(
                    "# {}{}#",
                    line,
                    " ".repeat(
                        self.terminal_width as usize
                            - line.width().min(self.terminal_width as usize)
                    )
                ))
                .collect::<Vec<_>>()
                .join("\n")
        )?;
        writeln!(writer)?;
        serde_yaml::to_writer(writer, self)?;
        Ok(())
    }
}

#[derive(Clone, Default)]
pub(crate) struct ReplayContext {
    pub bashrc_files: Vec<PathBuf>,
}

/// Opens a TUI to record a terminal session.
/// Returns the recording if the user pressed Ctrl+S, or None if the user pressed Ctrl+C.
pub(crate) fn record_using_tui(
    terminal: &mut ratatui::DefaultTerminal,
    replay_context: &ReplayContext,
) -> Result<Option<Recording>> {
    let terminal_width = terminal
        .size()
        .context("Failed to get terminal size")?
        .width;
    let mut recording = Recording::new(terminal_width);
    let mut pty = crate::pty::Pty::new_in_thread(terminal_width, &replay_context.bashrc_files)?;
    loop {
        if let Some(new_output) = pty.get_new_output() {
            recording.append_output(String::from_utf8_lossy(&new_output).to_string());
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
                    " - Capture Recording".green(),
                ]))
                .centered();
                frame.render_widget(title, layout);
            }

            // pty
            let layout = layout_pty;
            {
                let pty_output = match pty
                    .get_total_output()
                    .get_emulator_output_formatted()
                    .into_text()
                {
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
                    " · ".bold(),
                    "Ctrl+B for batch input".green(),
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
                } else if key_event.code == crossterm::event::KeyCode::Char('b') {
                    let input = ui::prompt_for_text(terminal, "Enter input to send to the TTY:")?;
                    for char in input.chars() {
                        let mut buf = [0; 16];
                        let terminput_event = terminput::Event::Key(terminput::KeyEvent::new(
                            terminput::KeyCode::Char(char),
                        ));
                        if let Ok(written) =
                            terminput_event.encode(&mut buf, terminput::Encoding::Xterm)
                        {
                            recording
                                .recording_items
                                .push(RecordingItem::Input(buf[..written].to_vec()));
                            pty.send_input(buf[..written].to_vec())?;
                        }
                        // todo refactor with same logic below, possibly can be simplified
                    }
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
}

/// Replay a previously recorded terminal session and assert that the output matches the recording.
pub(crate) fn replay_recording(
    recording: Recording,
    replay_context: &ReplayContext,
    timeout: Duration,
) -> Result<ReplayResult> {
    let mut pty =
        crate::pty::Pty::new_in_thread(recording.terminal_width, &replay_context.bashrc_files)?;

    let mut mismatch = ReplayMismatch {
        expected_output: TerminalOutput::from_recording(&recording),
        actual_output: TerminalOutput::new(recording.terminal_width),
        reason: ReplayMismatchReason::OutputMismatch,
    };
    let mut unmatched_output = String::new();

    let mut recording_items = recording.recording_items.clone();
    if recording_items.is_empty() {
        return Err(anyhow::anyhow!("Recording has no items"));
    }
    let mut current_item = recording_items.remove(0);
    let mut last_output_time = std::time::Instant::now();
    loop {
        // append new output
        if let Some(new_output) = pty.get_new_output() {
            let output = String::from_utf8_lossy(&new_output);
            mismatch = ReplayMismatch {
                actual_output: pty.get_total_output().clone(),
                ..mismatch
            };
            unmatched_output.push_str(&output);
            last_output_time = std::time::Instant::now();
        }

        match current_item {
            RecordingItem::Input(_) => {
                return Ok(ReplayResult::RecordingError(
                    "Recordings must never start with input".to_string(),
                ));
            }
            RecordingItem::Output(ref expected_output) => {
                match unmatched_output.len().cmp(&expected_output.len()) {
                    std::cmp::Ordering::Less => {
                        // still waiting for more output, so check if it matches so far
                        if !expected_output.starts_with(&unmatched_output) {
                            return Ok(ReplayResult::Mismatch(mismatch));
                        }
                        // and check for timeout
                        if last_output_time.elapsed() > timeout {
                            return Ok(ReplayResult::Mismatch(ReplayMismatch {
                                reason: ReplayMismatchReason::Timeout,
                                ..mismatch
                            }));
                        }
                    }
                    std::cmp::Ordering::Equal => {
                        // compare output and execute all next recorded input items
                        unmatched_output.clear();
                        if recording_items.is_empty() {
                            return Ok(ReplayResult::Match);
                        } else {
                            current_item = recording_items.remove(0);
                        }
                        let mut input_buffer = Vec::new();
                        while let RecordingItem::Input(input) = current_item {
                            input_buffer.extend_from_slice(&input);
                            if recording_items.is_empty() {
                                return Ok(ReplayResult::Match);
                            } else {
                                current_item = recording_items.remove(0);
                            }
                        }
                        pty.send_input(input_buffer)?;
                    }
                    std::cmp::Ordering::Greater => {
                        // err at extra unexpected output
                        return Ok(ReplayResult::Mismatch(mismatch));
                    }
                }
            }
        }

        // mismatch if the process exits unexpectedly
        if !pty.is_alive() {
            return Ok(ReplayResult::Mismatch(ReplayMismatch {
                reason: ReplayMismatchReason::UnexpectedExit,
                ..mismatch
            }));
        }
    }
}

pub(crate) fn write_mismatch_diff_to_disk(
    output_dir: &Path,
    recording_name: &str,
    mismatch: &ReplayMismatch,
) -> Result<PathBuf> {
    fn diff(expected: &str, actual: &str) -> String {
        let diff = similar::TextDiff::from_lines(expected, actual);
        let mut diff_output = String::new();
        for change in diff.iter_all_changes() {
            let sign = match change.tag() {
                similar::ChangeTag::Delete => "- ",
                similar::ChangeTag::Insert => "+ ",
                similar::ChangeTag::Equal => "  ",
            };
            diff_output.push_str(&format!("{}{}", sign, change));
        }
        diff_output
    }

    let diff_file_path = output_dir.join(format!(
        "manuel_mismatch_diff_{}.md",
        recording_name
            .replace("/", "__")
            .rsplit_once(".")
            .unwrap_or((recording_name, ""))
            .0
    ));
    std::fs::write(
        &diff_file_path,
        format!(
            "# Manuel mismatch diff for `{}` ({})\n\
            \n\
            ⚠️ The output listings displayed here first do not contain formatting!\n\
            \n\
            ## Diff (as it appears in the terminal)\n\
            ```\n{}\n```\n\
            \n\
            ## Diff (as it comes in)\n\
            ```\n{}\n```\n\
            \n\
            ## Diff (as it appears in the terminal, with formatting)\n\
            ```\n{}\n```\n\
            \n\
            ## Diff (as it comes in, with formatting)\n\
            ```\n{}\n```\n\
            \n\
            ",
            recording_name,
            mismatch.reason.as_str(),
            diff(
                &mismatch.expected_output.get_emulator_output_stripped(),
                &mismatch.actual_output.get_emulator_output_stripped()
            ),
            diff(
                &mismatch.expected_output.get_output_stripped(),
                &mismatch.actual_output.get_output_stripped()
            ),
            diff(
                &mismatch.expected_output.get_emulator_output_formatted(),
                &mismatch.actual_output.get_emulator_output_formatted()
            ),
            diff(
                &mismatch.expected_output.get_output_formatted(),
                &mismatch.actual_output.get_output_formatted()
            )
        ),
    )
    .context("Failed to write diff to file")?;

    Ok(diff_file_path)
}
