use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context as _, Result};
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, KeyCode};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Stylize};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, List, ListState, Paragraph};

use crate::recordings::{
    Recording, ReplayContext, record_using_tui, replay_recording, write_mismatch_diff_to_disk,
};
use crate::{ReplayResult, ui};

#[derive(Clone, Default)]
pub struct Collection {
    pub name: String,
    pub collections: HashMap<String, Collection>,
    pub recordings: HashMap<String, Recording>,
    pub replay_context: ReplayContext,
}

impl Collection {
    pub fn read_from_dir<P: AsRef<std::path::Path>>(dir: P) -> Result<Collection> {
        let dir: &Path = dir.as_ref();
        Self::read_from_dir_(dir, "".to_string(), &ReplayContext::default())
    }
    fn read_from_dir_(
        dir: &Path,
        name: String,
        replay_context: &ReplayContext,
    ) -> Result<Collection> {
        let entries =
            std::fs::read_dir(dir).context("Failed to read Manuel recordings directory")?;
        let mut collections = HashMap::new();
        let mut recordings = HashMap::new();
        let mut replay_context = replay_context.clone();
        for entry in entries {
            let entry = entry.context("Failed to read entry in Manuel recordings directory")?;
            let path = entry.path();
            let entry_name = path
                .file_name()
                .map(|str| str.to_string_lossy().to_string())
                .unwrap_or("<unnamed>".to_string());
            if path.is_dir() {
                let collection = Collection::read_from_dir_(
                    &path,
                    format!("{}/{}", name, entry_name),
                    &replay_context,
                )
                .context("Failed to read Manuel collection")?;
                collections.insert(entry_name, collection);
            } else if path.extension().is_some_and(|ext| ext == "yaml") {
                let recording =
                    Recording::read_from_file(&path).context("Failed to read Manuel recording")?;
                recordings.insert(entry_name, recording);
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
            name,
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
                    " · ".bold(),
                    "[+] open mismatch diff".green(),
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
                                    ReplayResult::Mismatch { .. } => "❌ ",
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
                            } else {
                                ui::alert(
                                    terminal,
                                    "Run in VS Code integrated terminal to open files.",
                                )?;
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
                            ui::prompt_for_text(terminal, "Name the new recording:")?
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
                                ui::alert(terminal, "Cannot delete non-empty collection.")?;
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
                            replay_results.clear();
                        }
                    }
                }
                KeyCode::Char('+') => {
                    if let Some(selected) = list_state.selected() {
                        if selected < collection.collections.len() {
                            ui::alert(
                                terminal,
                                "Cannot open diff for a collection. Please select a recording.",
                            )?;
                        } else {
                            let selected_recording_name = collection
                                .recordings
                                .keys()
                                .nth(selected - collection.collections.len())
                                .context("Selected recording not found")?
                                .clone();
                            if let Some(ReplayResult::Mismatch {
                                expected_output,
                                actual_output,
                            }) = replay_results.get(&selected_recording_name)
                            {
                                let diff_file_path = write_mismatch_diff_to_disk(
                                    &std::env::temp_dir(),
                                    &format!(".{}/{}", collection.name, selected_recording_name),
                                    expected_output,
                                    actual_output,
                                )?;
                                open_in_vscode(terminal, &diff_file_path)?;
                            } else {
                                ui::alert(
                                    terminal,
                                    "No mismatch found for the selected recording.",
                                )?;
                            }
                        }
                    }
                }
                KeyCode::Char('c') => {
                    let new_collection_name =
                        ui::prompt_for_text(terminal, "Name the new collection:")?
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

fn open_in_vscode(terminal: &mut DefaultTerminal, path: &Path) -> Result<()> {
    if std::env::var("TERM_PROGRAM").unwrap_or_default() == "vscode" {
        std::process::Command::new("code")
            .arg(path.display().to_string())
            .spawn()
            .context("Failed to open recording in VSCode")?;
    } else {
        ui::alert(
            terminal,
            "Run in VS Code integrated terminal to open files.",
        )?;
    }
    Ok(())
}
