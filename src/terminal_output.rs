use serde::{Deserialize, Serialize};

use crate::recordings::Recording;

#[derive(Clone, Serialize, Deserialize)]
pub struct TerminalOutput {
    terminal_width: u16,
    output: Vec<u8>,
}

impl TerminalOutput {
    pub fn new(terminal_width: u16) -> Self {
        Self {
            terminal_width,
            output: Vec::new(),
        }
    }

    pub(crate) fn from_recording(recording: &Recording) -> Self {
        Self {
            terminal_width: recording.get_terminal_width(),
            output: recording
                .get_recording_items()
                .iter()
                .filter_map(|item| match item {
                    crate::recordings::RecordingItem::Output(output) => {
                        Some(output.as_bytes().to_vec())
                    }
                    _ => None,
                })
                .flatten()
                .collect(),
        }
    }

    pub(crate) fn append_output(&mut self, output: &[u8]) {
        self.output.extend_from_slice(output);
    }

    pub fn get_output_formatted(&self) -> String {
        String::from_utf8_lossy(&self.output).to_string()
    }

    pub fn get_output_stripped(&self) -> String {
        String::from_utf8_lossy(&strip_ansi_escapes::strip(&self.output)).to_string()
    }

    pub fn get_emulator_output_formatted(&self) -> String {
        let mut vte = vt100::Parser::new(
            u16::MAX, // todo?
            self.terminal_width - 1,
            0,
        );
        vte.process(&self.output);

        let mut output_buffer: Vec<u8> = Vec::new();
        let screen = vte.screen();
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
        String::from_utf8_lossy(&output_buffer).to_string()
    }

    pub fn get_emulator_output_stripped(&self) -> String {
        String::from_utf8_lossy(&strip_ansi_escapes::strip(
            self.get_emulator_output_formatted(),
        ))
        .to_string()
    }
}
