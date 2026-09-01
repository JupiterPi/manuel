use anyhow::Result;
use ratatui::{
    DefaultTerminal, crossterm::event::{self, KeyCode}, layout::Rect, style::{Color, Stylize}, widgets::{Block, Borders, Paragraph},
};

pub(crate) fn prompt_for_text(terminal: &mut DefaultTerminal, message: &str) -> Result<String> {
    let mut input = String::new();
    loop {
        terminal.draw(|frame| {
            let area = frame.area();
            let area = centered_rect(area, 5, 80);
            let block = Block::new()
                .borders(Borders::ALL)
                .title(message)
                .title_bottom(vec![
                    "[Enter] submit".green(),
                    " · ".bold(),
                    "[Esc] cancel".green(),
                ]);
            let inner_area = block.inner(area);
            frame.render_widget(block, area);
            frame.render_widget(
                ratatui::widgets::Paragraph::new(input.as_str())
                    .style(Color::White)
                    .block(Block::new().borders(Borders::NONE)),
                inner_area,
            );
        })?;

        if let Some(key) = event::read()?.as_key_press_event() {
            match key.code {
                KeyCode::Char(c) => input.push(c),
                KeyCode::Backspace => {
                    input.pop();
                }
                KeyCode::Enter => return Ok(input),
                KeyCode::Esc => return Err(anyhow::anyhow!("Input cancelled")),
                _ => {}
            }
        }
    }
}

pub(crate) fn alert(terminal: &mut DefaultTerminal, message: &str) -> Result<()> {
    loop {
        terminal.draw(|frame| {
            let area = frame.area();
            let area = centered_rect(area, 5, 80);
            let block = Block::new()
                .borders(Borders::ALL)
                .title_bottom(vec!["[Enter]".green()]);
            let text = Paragraph::new(message).style(Color::White).block(block);
            frame.render_widget(text, area);
        })?;

        if let Some(key) = event::read()?.as_key_press_event() {
            match key.code {
                KeyCode::Enter | KeyCode::Esc => return Ok(()),
                _ => {}
            }
        }
    }
}

pub(crate) fn centered_rect(area: Rect, max_height: u16, max_width: u16) -> Rect {
    let width = std::cmp::min(max_width, area.width);
    let height = std::cmp::min(max_height, area.height);
    let x = area.x + (area.width - width) / 2;
    let y = area.y + (area.height - height) / 2;
    Rect {
        x,
        y,
        width,
        height,
    }
}
