use anyhow::Result;
use ratatui::{
    DefaultTerminal, crossterm,
    layout::{Constraint, Direction, Layout},
    style::Stylize,
    text::Line,
    widgets::{Block, Borders, Paragraph},
};

fn main() -> Result<()> {
    ratatui::run(app)?;
    Ok(())
}

fn app(terminal: &mut DefaultTerminal) -> Result<()> {
    loop {
        terminal.draw(|frame| {
            let layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints(vec![
                    Constraint::Length(1),
                    Constraint::Fill(1),
                    Constraint::Length(3),
                ])
                .split(frame.area());

            let title = Paragraph::new(Line::from(vec![
                "Manuel".bold().green(),
                " - Capture Snapshot \"Name\"".green(),
            ]))
            .centered();
            frame.render_widget(title, layout[0]);

            let pty_block = Block::default()
                .borders(Borders::ALL)
                .title(Line::from("PTY").bold());
            frame.render_widget(pty_block, layout[1]);

            let input_block = Block::default()
                .borders(Borders::ALL)
                .title(Line::from("Input to PTY").bold());
            let input_status = Paragraph::new(Line::from("Press any key to exit").green())
                .centered()
                .block(input_block);
            frame.render_widget(input_status, layout[2]);
        })?;
        if crossterm::event::read()?.is_key_press() {
            break Ok(());
        }
    }
}
