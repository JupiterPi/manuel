use std::time::Duration;

use ansi_to_tui::IntoText;
use anyhow::Result;
use portable_pty::CommandBuilder;
use ratatui::{
    DefaultTerminal, crossterm,
    layout::{Constraint, Direction, Layout, Size},
    style::Stylize,
    text::{Line, Text},
    widgets::{Block, Borders, Paragraph},
};

fn main() -> Result<()> {
    /* simplelog::WriteLogger::init(
        log::LevelFilter::Info,
        simplelog::Config::default(),
        std::fs::File::create("manuel.log")?,
    )?; */

    ratatui::run(app)?;

    Ok(())
}

fn app(terminal: &mut DefaultTerminal) -> Result<()> {
    let (initialize_pty_width_tx, initialize_pty_width_rx) = std::sync::mpsc::channel::<u16>();
    let (pty_out_tx, pty_out_rx) = std::sync::mpsc::channel();
    let (pty_in_tx, pty_in_rx) = std::sync::mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let width = match initialize_pty_width_rx.recv() {
            Ok(width) => width,
            Err(e) => {
                log::error!("Error receiving PTY width: {:?}", e);
                return;
            }
        };
        let pty_system = portable_pty::native_pty_system();
        let pty = pty_system
            .openpty(portable_pty::PtySize {
                rows: 10,
                cols: width - 1,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let cmd = CommandBuilder::new("bash");
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
                        match pty_out_tx.send(String::from_utf8_lossy(&buffer[..n]).into_owned()) {
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

    let mut pty_initialized = false;
    let mut pty_output = String::new();
    let mut scroll_view_state = tui_scrollview::ScrollViewState::default();
    loop {
        while let Ok(content) = pty_out_rx.try_recv() {
            pty_output.push_str(&content);
            scroll_view_state.scroll_to_bottom();
            log::info!(
                "Scroll view state after receiving PTY output: {:?}",
                scroll_view_state
            );
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
                if !pty_initialized {
                    initialize_pty_width_tx.send(layout.width).unwrap();
                    pty_initialized = true;
                }
                let pty_output_text = match pty_output.into_text() {
                    Ok(text) => text,
                    Err(e) => {
                        log::error!("Error converting PTY output to text: {:?}", e);
                        Text::from("Error converting PTY output to text")
                    }
                };
                /* let pty_output_text = (1..100)
                .map(|i| format!("Line {}", i))
                .collect::<Vec<_>>()
                .join("\n"); */
                /* let pty_block = Block::default()
                    .borders(Borders::ALL)
                    .title(Line::from("PTY").bold());
                let pty_output_paragraph = Paragraph::new(pty_output_text).block(pty_block);
                frame.render_widget(pty_output_paragraph, layout[1]); */
                log::info!("Scroll view state: {:?}", scroll_view_state);
                let mut scroll_view = tui_scrollview::ScrollView::new(Size::new(
                    layout.width - 1,
                    pty_output_text.height() as u16,
                ));
                scroll_view.render_widget(Paragraph::new(pty_output_text), scroll_view.area());
                frame.render_stateful_widget(scroll_view, layout, &mut scroll_view_state);
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
                    "Press Ctrl+C to exit".green(),
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
                && key_event.code == crossterm::event::KeyCode::Char('c')
            {
                break Ok(());
            }
            let terminput_event = terminput_crossterm::to_terminput(crossterm_event)?;
            let mut buf = [0; 16];
            if let Ok(written) = terminput_event.encode(
                &mut buf,
                /* terminput::Encoding::Kitty(terminput::KittyFlags::all()), */
                terminput::Encoding::Xterm,
            ) {
                pty_in_tx.send(buf[..written].to_vec())?;
            }
        }
    }
}
