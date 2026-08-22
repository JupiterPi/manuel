use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context as _, Result};
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, KeyCode};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Stylize};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, List, ListState, Paragraph};

use crate::ReplayResult;
use crate::recordings::{Recording, ReplayContext, record_using_tui, replay_recording};

#[derive(Clone, Default)]
pub struct Collection {
    pub collections: HashMap<String, Collection>,
    pub recordings: HashMap<String, Recording>,
    pub replay_context: ReplayContext,
}

impl Collection {
    pub fn read_from_dir<P: AsRef<std::path::Path>>(dir: P) -> Result<Collection> {
        let dir: &Path = dir.as_ref();
        Self::read_from_dir_(dir, &ReplayContext::default())
    }
    fn read_from_dir_(dir: &Path, replay_context: &ReplayContext) -> Result<Collection> {
        let entries =
            std::fs::read_dir(dir).context("Failed to read Manuel recordings directory")?;
        let mut collections = HashMap::new();
        let mut recordings = HashMap::new();
        let mut replay_context = replay_context.clone();
        for entry in entries {
            let entry = entry.context("Failed to read entry in Manuel recordings directory")?;
            let path = entry.path();
            let name = path
                .file_name()
                .map(|str| str.to_string_lossy().to_string())
                .unwrap_or("<unnamed>".to_string());
            if path.is_dir() {
                let collection = Collection::read_from_dir_(&path, &replay_context)
                    .context("Failed to read Manuel collection")?;
                collections.insert(name, collection);
            } else if path.extension().is_some_and(|ext| ext == "yaml") {
                let recording =
                    Recording::read_from_file(&path).context("Failed to read Manuel recording")?;
                recordings.insert(name, recording);
            } else if path.file_name().is_some_and(|name| name == ".bashrc") {
                replay_context.bashrc_files.push(path);
            } else {
                log::warn!(
                    "Ignoring unknown file in Manuel recordings directory: {}",
                    path.display()
                );
            }
        }
        Ok(Collection {
            collections,
            recordings,
            replay_context,
        })
    }

    pub(crate) fn flat_recordings(&self) -> HashMap<String, (Recording, ReplayContext)> {
        let mut recordings = HashMap::new();
        for (name, recording) in &self.recordings {
            recordings.insert(
                name.clone(),
                (recording.clone(), self.replay_context.clone()),
            );
        }
        for (name, collection) in &self.collections {
            let sub_recordings = collection.flat_recordings();
            for (sub_name, recording) in sub_recordings {
                recordings.insert(format!("{}/{}", name, sub_name), recording);
            }
        }
        recordings
    }
}

/// Opens a TUI where the user can navigate and create collections, preview and create recordings, and run Manuel tests.
pub fn explore_collection_in_tui(
    terminal: &mut ratatui::DefaultTerminal,
    collection: &Collection,
    collection_dir: &Path,
) -> Result<()> {
    let mut collection = collection.clone();
    let mut replay_queue = Vec::<(String, Recording, ReplayContext)>::new();
    let mut replay_results = HashMap::<String, ReplayResult>::new();
    let mut list_state = ListState::default();
    loop {
        while let Some((name, recording, replay_context)) = replay_queue.pop() {
            let result = replay_recording(recording.clone(), &replay_context)
                .context("Failed to replay Manuel recording")?;
            replay_results.insert(name, result);
        }
        // todo: really make this async

        terminal.draw(|frame| {
            let explorer_block = Block::new().borders(Borders::ALL).title(Line::from_iter([
                " Manuel Explorer".into(),
                " · ".bold(),
                collection_dir.to_string_lossy().to_string().italic(),
                " ".into(),
            ]));
            let inner_area = explorer_block.inner(frame.area());
            frame.render_widget(explorer_block, frame.area());

            let [layout_list, layout_help] =
                Layout::vertical([Constraint::Fill(1), Constraint::Length(6)]).areas(inner_area);

            let help_text_block = Block::new().borders(Borders::ALL).title(" Help ");
            let help_text = Paragraph::new(vec![
                Line::from(vec![
                    "[↑/↓] select item".green(),
                    " · ".bold(),
                    "[←/Esc] navigate back, close".green(),
                ]),
                Line::from(vec![
                    "[→/Enter] navigate into collection, open file in VS Code".green(),
                ]),
                Line::from(vec![
                    "[a/d] add/delete recording".green(),
                    " · ".bold(),
                    "[c/d] add/delete empty collection".green(),
                ]),
                Line::from(vec![
                    "[r] reload from disk".green(),
                    " · ".bold(),
                    "[t] run tests".green(),
                ]),
            ])
            .block(help_text_block);
            frame.render_widget(help_text, layout_help);

            let items_list = List::new({
                let collection_names = collection
                    .collections
                    .keys()
                    .map(|name| format!("📁 {}", name))
                    .collect::<Vec<_>>();
                let recording_names = collection
                    .recordings
                    .keys()
                    .map(|name| {
                        format!(
                            "📼 {}{}",
                            replay_results
                                .get(name)
                                .map(|r| match r {
                                    ReplayResult::Match => "✅ ",
                                    ReplayResult::Mismatch(_) => "❌ ",
                                    ReplayResult::RecordingError(_) => "⚠️ ",
                                })
                                .unwrap_or_default(),
                            name
                        )
                    })
                    .collect::<Vec<_>>();
                collection_names.into_iter().chain(recording_names)
            })
            .style(Color::White)
            .highlight_style(Modifier::RAPID_BLINK | Modifier::BOLD);
            frame.render_stateful_widget(items_list, layout_list, &mut list_state);
        })?;

        if let Some(key) = event::read()?.as_key_press_event() {
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc | KeyCode::Char('h') | KeyCode::Left => {
                    break Ok(());
                }
                KeyCode::Char('j') | KeyCode::Down => list_state.select_next(),
                KeyCode::Char('k') | KeyCode::Up => list_state.select_previous(),
                KeyCode::Char('l') | KeyCode::Right | KeyCode::Enter => {
                    if let Some(selected) = list_state.selected() {
                        if selected < collection.collections.len() {
                            let (selected_collection_name, selected_collection) = collection
                                .collections
                                .iter()
                                .nth(selected)
                                .context("Selected collection not found")?;
                            explore_collection_in_tui(
                                terminal,
                                selected_collection,
                                &collection_dir.join(selected_collection_name),
                            )?;
                        } else {
                            let selected_recording_name = collection
                                .recordings
                                .keys()
                                .nth(selected - collection.collections.len())
                                .context("Selected recording not found")?;
                            if std::env::var("TERM_PROGRAM").unwrap_or_default() == "vscode" {
                                std::process::Command::new("code")
                                    .arg(collection_dir.join(selected_recording_name))
                                    .spawn()
                                    .context("Failed to open recording in VSCode")?;
                            }
                        }
                    }
                }
                KeyCode::Char('r') => {
                    collection = Collection::read_from_dir(collection_dir)
                        .context("Failed to read Manuel recordings directory")?;
                }
                KeyCode::Char('a') => {
                    if let Ok(Some(new_recording)) =
                        record_using_tui(terminal, &collection.replay_context)
                    {
                        let new_recording_name =
                            prompt_for_text(terminal, "Name the new recording:")?
                                .replace(" ", "_")
                                .to_lowercase();
                        let new_recording_name = format!("{}.yaml", new_recording_name);
                        new_recording
                            .write_to_file(collection_dir.join(&new_recording_name))
                            .context("Failed to write new recording to file")?;
                        collection
                            .recordings
                            .insert(new_recording_name, new_recording);
                    }
                }
                KeyCode::Char('d') => {
                    if let Some(selected) = list_state.selected() {
                        if selected < collection.collections.len() {
                            let (selected_collection_name, selected_collection) = collection
                                .collections
                                .iter()
                                .nth(selected)
                                .context("Selected collection not found")?;
                            if selected_collection.collections.is_empty()
                                && selected_collection.recordings.is_empty()
                            {
                                std::fs::remove_dir_all(
                                    collection_dir.join(selected_collection_name),
                                )
                                .context("Failed to delete collection directory")?;
                                let selected_collection_name = selected_collection_name.clone();
                                collection.collections.remove(&selected_collection_name);
                            } else {
                                alert(terminal, "Cannot delete non-empty collection.")?;
                            }
                        } else {
                            let selected_recording_name = collection
                                .recordings
                                .keys()
                                .nth(selected - collection.collections.len())
                                .context("Selected recording not found")?
                                .clone();
                            std::fs::remove_file(collection_dir.join(&selected_recording_name))
                                .context("Failed to delete recording file")?;
                            collection.recordings.remove(&selected_recording_name);
                        }
                    }
                }
                KeyCode::Char('c') => {
                    let new_collection_name =
                        prompt_for_text(terminal, "Name the new collection:")?
                            .replace(" ", "_")
                            .to_lowercase();
                    std::fs::create_dir(collection_dir.join(&new_collection_name))
                        .context("Failed to create new collection directory")?;
                    collection
                        .collections
                        .insert(new_collection_name, Collection::default());
                }
                KeyCode::Char('t') => {
                    collection.recordings.iter().for_each(|(name, recording)| {
                        replay_queue.push((
                            name.clone(),
                            recording.clone(),
                            collection.replay_context.clone(),
                        ));
                    });
                }
                _ => {}
            }
        }
    }
}

fn prompt_for_text(terminal: &mut DefaultTerminal, message: &str) -> Result<String> {
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

fn alert(terminal: &mut DefaultTerminal, message: &str) -> Result<()> {
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

fn centered_rect(area: Rect, max_height: u16, max_width: u16) -> Rect {
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
