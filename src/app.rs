use crate::core::{AppEnv, ShortcutLauncherInputSourceGuard, Workspace};
use anyhow::{Context, Result};
use crossterm::{
    cursor::{Hide, Show},
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};
use std::{
    env,
    io::{self, Stdout},
    path::Path,
    time::{Duration, Instant},
};

const DOUBLE_CLICK_MS: u64 = 350;
const MIN_WIDTH: u16 = 52;
const MIN_HEIGHT: u16 = 15;
const MAIN_LIST_WIDTH: u16 = 24;

pub fn run_tui(env: &mut AppEnv) -> Result<()> {
    let mut app = App::new(env.list_workspaces()?);
    let (_shortcut_input_source_guard, input_source_warning) =
        match ShortcutLauncherInputSourceGuard::activate_for_tui() {
            Ok(guard) => (Some(guard), None),
            Err(error) => (
                None,
                Some(format!(
                    "ASCII input source switch failed; letter shortcuts may not work: {error}"
                )),
            ),
        };
    let mut terminal = TerminalSession::start()?;

    if let Some(warning) = input_source_warning {
        app.set_error(warning);
    }

    loop {
        terminal.draw(|frame| draw(frame, &mut app, env))?;

        if let Some(expiry) = app.status_expiry
            && Instant::now() >= expiry
        {
            app.clear_status();
        }

        if !event::poll(Duration::from_millis(60)).context("failed to poll terminal events")? {
            continue;
        }

        match event::read().context("failed to read terminal event")? {
            Event::Key(key) => {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                match app.handle_key(key, env)? {
                    Action::None => {}
                    Action::Quit => break,
                    Action::Refresh => match env.list_workspaces() {
                        Ok(workspaces) => {
                            app.reload(workspaces);
                            app.set_success("Reloaded workspace list");
                        }
                        Err(error) => app.set_error(error.to_string()),
                    },
                    Action::Launch(name) => {
                        let warning = launch_workspace_from_tui(&mut terminal, env, &name)?;
                        drop(terminal);
                        if let Some(warning) = warning {
                            eprintln!("warning: {warning}");
                        }
                        return Ok(());
                    }
                    Action::Save(name) => {
                        terminal.suspend()?;
                        let result = env.save_current_window(&name);
                        terminal.resume()?;

                        match result {
                            Ok(path) => {
                                app.reset_dialogs();
                                app.reload(env.list_workspaces()?);
                                app.select_name(&name);
                                app.set_success(format!(
                                    "Saved workspace \"{name}\" to {}",
                                    display_path(&path)
                                ));
                            }
                            Err(error) => app.set_error(error.to_string()),
                        }
                    }
                    Action::Rename(old_name, new_name) => {
                        match env.rename_workspace(&old_name, &new_name) {
                            Ok(_) => {
                                app.reset_dialogs();
                                app.reload(env.list_workspaces()?);
                                app.select_name(&new_name);
                                app.set_success(format!(
                                    "Renamed workspace \"{old_name}\" to \"{new_name}\""
                                ));
                            }
                            Err(error) => app.set_error(error.to_string()),
                        }
                    }
                    Action::Edit(name) => {
                        terminal.suspend()?;
                        let result = env.open_in_editor(&name);
                        terminal.resume()?;

                        match result {
                            Ok(()) => {
                                app.reload(env.list_workspaces()?);
                                app.select_name(&name);
                                app.set_success(format!("Closed editor for \"{name}\""));
                            }
                            Err(error) => app.set_error(error.to_string()),
                        }
                    }
                    Action::Delete(name) => match env.remove_workspace(&name) {
                        Ok(_) => {
                            app.reset_dialogs();
                            app.reload(env.list_workspaces()?);
                            app.set_success(format!("Removed workspace \"{name}\""));
                        }
                        Err(error) => app.set_error(error.to_string()),
                    },
                    Action::ToggleCloseTab => match env.set_close_tab(!env.config.close_tab) {
                        Ok(()) => {
                            app.set_success(format!("close_tab = {}", env.close_tab_display()))
                        }
                        Err(error) => app.set_error(error.to_string()),
                    },
                    Action::SetGhosttyShortcut(shortcut) => {
                        match env.set_ghostty_shortcut(&shortcut) {
                            Ok(result) => {
                                app.dialog = app.shortcut_return_dialog.clone();
                                app.shortcut_input.clear();
                                if result.sync.shortcut == "off" {
                                    if result.status
                                        == crate::core::GhosttyShortcutApplyStatus::ManualConfigRemovalRequired
                                    {
                                        app.set_success(
                                            "Shortcut file removed. Also remove the include from your Ghostty config source.",
                                        );
                                    } else {
                                        app.set_success(
                                            "Ghostty-local shortcut disabled. Run `gtab init` to restore Cmd+G.",
                                        );
                                    }
                                } else if result.status
                                    == crate::core::GhosttyShortcutApplyStatus::ManualConfigRequired
                                {
                                    app.set_success(
                                        "Shortcut file updated. Add the include to your Ghostty config source, then rebuild.",
                                    );
                                } else {
                                    app.set_success(format!(
                                    "Ghostty-local shortcut saved as {}. Reload Ghostty config to apply it.",
                                    result.sync.shortcut
                                ));
                                }
                            }
                            Err(error) => app.set_error(error.to_string()),
                        }
                    }
                }
            }
            Event::Mouse(mouse) => match app.handle_mouse(mouse, env)? {
                Action::None => {}
                Action::Launch(name) => {
                    let warning = launch_workspace_from_tui(&mut terminal, env, &name)?;
                    drop(terminal);
                    if let Some(warning) = warning {
                        eprintln!("warning: {warning}");
                    }
                    return Ok(());
                }
                _ => {}
            },
            _ => continue,
        };
    }

    Ok(())
}

fn launch_workspace_from_tui(
    terminal: &mut TerminalSession,
    env: &AppEnv,
    name: &str,
) -> Result<Option<String>> {
    let (frame, pending_warning) = match env.capture_frontmost_ghostty_window_frame() {
        Ok(frame) => (Some(frame), None),
        Err(error) => (
            None,
            Some(format!(
                "workspace launched without frame sync; gtab could not read the current Ghostty window frame: {error}"
            )),
        ),
    };

    terminal.suspend()?;

    let result = match frame {
        Some(frame) => env.launch_workspace_from_tui_with_frame(name, &frame),
        None => env.launch_workspace(name).map(|()| None),
    };

    match result {
        Ok(warning) => Ok(warning.or(pending_warning)),
        Err(error) => {
            terminal.resume()?;
            Err(error)
        }
    }
}

struct TerminalSession {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    suspended: bool,
}

impl TerminalSession {
    fn start() -> Result<Self> {
        enable_raw_mode().context("failed to enable raw mode")?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture, Hide)
            .context("failed to enter alternate screen")?;
        let terminal = Terminal::new(CrosstermBackend::new(stdout))
            .context("failed to initialize terminal backend")?;
        Ok(Self {
            terminal,
            suspended: false,
        })
    }

    fn draw(&mut self, f: impl FnOnce(&mut Frame<'_>)) -> Result<()> {
        self.terminal.draw(f).context("failed to draw frame")?;
        Ok(())
    }

    fn suspend(&mut self) -> Result<()> {
        // Drain any buffered terminal events (e.g. mouse button release) before
        // disabling raw mode. Without this, the release bytes can leak into the
        // shell's input buffer and appear as visible text (e.g. "0;9;4m").
        while event::poll(Duration::ZERO).unwrap_or(false) {
            let _ = event::read();
        }

        disable_raw_mode().context("failed to disable raw mode")?;
        execute!(
            self.terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture,
            Show
        )
        .context("failed to leave alternate screen")?;
        self.terminal.show_cursor().ok();
        self.suspended = true;
        Ok(())
    }

    fn resume(&mut self) -> Result<()> {
        execute!(
            self.terminal.backend_mut(),
            EnterAlternateScreen,
            EnableMouseCapture,
            Hide
        )
        .context("failed to re-enter alternate screen")?;
        enable_raw_mode().context("failed to re-enable raw mode")?;
        self.terminal.clear().ok();
        self.suspended = false;
        Ok(())
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        if self.suspended {
            return;
        }
        let _ = disable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture,
            Show
        );
        let _ = self.terminal.show_cursor();
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Dialog {
    None,
    Save,
    Rename,
    ConfirmDelete,
    Settings,
    EditGhosttyShortcut,
    Help,
}

#[derive(Clone, Debug)]
enum StatusKind {
    Info,
    Success,
    Error,
}

#[derive(Clone, Debug)]
struct StatusLine {
    kind: StatusKind,
    text: String,
}

#[derive(Clone, Debug)]
struct ClickState {
    index: usize,
    at: Instant,
}

#[derive(Clone, Debug)]
struct App {
    workspaces: Vec<Workspace>,
    selected: usize,
    list_offset: usize,
    list_area: Rect,
    shortcut_area: Rect,
    last_click: Option<ClickState>,
    filter: String,
    search_before_edit: Option<String>,
    dialog: Dialog,
    save_input: String,
    rename_input: String,
    rename_original: Option<String>,
    shortcut_input: String,
    shortcut_return_dialog: Dialog,
    status: Option<StatusLine>,
    status_expiry: Option<Instant>,
}

impl App {
    fn new(workspaces: Vec<Workspace>) -> Self {
        Self {
            workspaces,
            selected: 0,
            list_offset: 0,
            list_area: Rect::default(),
            shortcut_area: Rect::default(),
            last_click: None,
            filter: String::new(),
            search_before_edit: None,
            dialog: Dialog::None,
            save_input: String::new(),
            rename_input: String::new(),
            rename_original: None,
            shortcut_input: String::new(),
            shortcut_return_dialog: Dialog::None,
            status: Some(StatusLine {
                kind: StatusKind::Info,
                text: "Enter launch  / filter  ? help".to_string(),
            }),
            status_expiry: None,
        }
    }

    fn reload(&mut self, workspaces: Vec<Workspace>) {
        self.workspaces = workspaces;
        self.clear_pending_click();
        self.clamp_selection();
    }

    fn reset_dialogs(&mut self) {
        self.dialog = Dialog::None;
        self.save_input.clear();
        self.rename_input.clear();
        self.rename_original = None;
        self.shortcut_input.clear();
        self.shortcut_return_dialog = Dialog::None;
    }

    fn open_settings(&mut self, _env: &AppEnv) {
        self.dialog = Dialog::Settings;
    }

    fn open_shortcut_editor(&mut self, env: &AppEnv, return_dialog: Dialog) {
        self.shortcut_return_dialog = return_dialog;
        self.dialog = Dialog::EditGhosttyShortcut;
        self.shortcut_input = env.ghostty_shortcut_display().to_string();
    }

    fn open_rename(&mut self, name: String) {
        self.dialog = Dialog::Rename;
        self.rename_input = name.clone();
        self.rename_original = Some(name);
    }

    fn visible_indices(&self) -> Vec<usize> {
        if self.filter.is_empty() {
            return (0..self.workspaces.len()).collect();
        }

        let needle = self.filter.to_lowercase();
        self.workspaces
            .iter()
            .enumerate()
            .filter_map(|(index, workspace)| {
                workspace
                    .name
                    .to_lowercase()
                    .contains(&needle)
                    .then_some(index)
            })
            .collect()
    }

    fn visible_workspaces(&self) -> Vec<&Workspace> {
        self.visible_indices()
            .iter()
            .map(|index| &self.workspaces[*index])
            .collect()
    }

    fn selected_workspace(&self) -> Option<&Workspace> {
        let indices = self.visible_indices();
        indices
            .get(self.selected)
            .and_then(|index| self.workspaces.get(*index))
    }

    fn select_name(&mut self, name: &str) {
        let Some(position) = self
            .visible_workspaces()
            .iter()
            .position(|workspace| workspace.name == name)
        else {
            self.selected = 0;
            self.clear_pending_click();
            return;
        };

        self.selected = position;
        self.clear_pending_click();
    }

    fn clamp_selection(&mut self) {
        let len = self.visible_indices().len();
        if len == 0 {
            self.selected = 0;
        } else {
            self.selected = self.selected.min(len.saturating_sub(1));
        }
    }

    fn reset_visible_selection(&mut self) {
        self.selected = 0;
        self.clear_pending_click();
        self.clamp_selection();
    }

    fn move_selection(&mut self, delta: isize) {
        let len = self.visible_indices().len();
        if len == 0 {
            self.selected = 0;
            self.clear_pending_click();
            return;
        }

        let max = len.saturating_sub(1) as isize;
        let next = (self.selected as isize + delta).clamp(0, max);
        self.selected = next as usize;
        self.clear_pending_click();
    }

    fn move_to_start(&mut self) {
        self.selected = 0;
        self.clear_pending_click();
    }

    fn move_to_end(&mut self) {
        let len = self.visible_indices().len();
        if len > 0 {
            self.selected = len - 1;
        }
        self.clear_pending_click();
    }

    fn page_step(&self) -> isize {
        self.list_area.height.saturating_sub(1).max(5) as isize
    }

    fn clear_pending_click(&mut self) {
        self.last_click = None;
    }

    fn is_double_click(&self, index: usize, clicked_at: Instant) -> bool {
        self.last_click.as_ref().is_some_and(|last_click| {
            last_click.index == index
                && clicked_at.duration_since(last_click.at)
                    <= Duration::from_millis(DOUBLE_CLICK_MS)
        })
    }

    fn search_active(&self) -> bool {
        self.search_before_edit.is_some()
    }

    fn begin_search(&mut self, initial: Option<char>) {
        if self.search_before_edit.is_none() {
            self.search_before_edit = Some(self.filter.clone());
        }

        if let Some(ch) = initial {
            self.filter.push(ch);
            self.reset_visible_selection();
        }
    }

    fn commit_search(&mut self) {
        self.search_before_edit = None;
        self.clear_pending_click();
    }

    fn cancel_search(&mut self) {
        if let Some(previous) = self.search_before_edit.take() {
            self.filter = previous;
            self.reset_visible_selection();
        }
    }

    fn update_filter_after_edit(&mut self) {
        self.reset_visible_selection();
    }

    fn set_status(&mut self, kind: StatusKind, text: impl Into<String>) {
        self.status = Some(StatusLine {
            kind,
            text: text.into(),
        });
        self.status_expiry = Some(Instant::now() + Duration::from_secs(4));
    }

    fn set_success(&mut self, text: impl Into<String>) {
        self.set_status(StatusKind::Success, text);
    }

    fn set_info(&mut self, text: impl Into<String>) {
        self.set_status(StatusKind::Info, text);
    }

    fn set_error(&mut self, text: impl Into<String>) {
        self.set_status(StatusKind::Error, text);
    }

    fn clear_status(&mut self) {
        self.status = None;
        self.status_expiry = None;
    }

    fn handle_key(&mut self, key: KeyEvent, env: &AppEnv) -> Result<Action> {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Ok(Action::Quit);
        }

        match self.dialog {
            Dialog::Save => self.handle_save_key(key),
            Dialog::Rename => self.handle_rename_key(key),
            Dialog::ConfirmDelete => self.handle_delete_key(key),
            Dialog::Settings => self.handle_settings_key(key, env),
            Dialog::EditGhosttyShortcut => self.handle_shortcut_key(key),
            Dialog::Help => self.handle_help_key(key),
            Dialog::None if self.search_active() => self.handle_search_key(key),
            Dialog::None => self.handle_main_key(key, env),
        }
    }

    fn handle_save_key(&mut self, key: KeyEvent) -> Result<Action> {
        match key.code {
            KeyCode::Esc => {
                self.reset_dialogs();
                Ok(Action::None)
            }
            KeyCode::Enter => {
                let name = self.save_input.trim().to_string();
                if name.is_empty() {
                    self.set_error("Workspace name cannot be empty");
                    return Ok(Action::None);
                }

                Ok(Action::Save(name))
            }
            KeyCode::Backspace => {
                self.save_input.pop();
                Ok(Action::None)
            }
            KeyCode::Char(c) if is_text_input(key.modifiers) => {
                self.save_input.push(c);
                Ok(Action::None)
            }
            _ => Ok(Action::None),
        }
    }

    fn handle_rename_key(&mut self, key: KeyEvent) -> Result<Action> {
        match key.code {
            KeyCode::Esc => {
                self.reset_dialogs();
                Ok(Action::None)
            }
            KeyCode::Enter => {
                let Some(original) = self.rename_original.clone() else {
                    self.reset_dialogs();
                    return Ok(Action::None);
                };

                let name = self.rename_input.trim().to_string();
                if name.is_empty() {
                    self.set_error("Workspace name cannot be empty");
                    return Ok(Action::None);
                }

                if name == original {
                    self.reset_dialogs();
                    self.set_info("Workspace name unchanged");
                    return Ok(Action::None);
                }

                Ok(Action::Rename(original, name))
            }
            KeyCode::Backspace => {
                self.rename_input.pop();
                Ok(Action::None)
            }
            KeyCode::Char(c) if is_text_input(key.modifiers) => {
                self.rename_input.push(c);
                Ok(Action::None)
            }
            _ => Ok(Action::None),
        }
    }

    fn handle_delete_key(&mut self, key: KeyEvent) -> Result<Action> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('n') => {
                self.reset_dialogs();
                Ok(Action::None)
            }
            KeyCode::Enter | KeyCode::Char('y') => {
                let Some(workspace) = self.selected_workspace() else {
                    self.reset_dialogs();
                    return Ok(Action::None);
                };

                Ok(Action::Delete(workspace.name.clone()))
            }
            _ => Ok(Action::None),
        }
    }

    fn handle_settings_key(&mut self, key: KeyEvent, env: &AppEnv) -> Result<Action> {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => {
                self.reset_dialogs();
                Ok(Action::None)
            }
            KeyCode::Char('c') | KeyCode::Char(' ') => Ok(Action::ToggleCloseTab),
            KeyCode::Char('g') => {
                self.open_shortcut_editor(env, Dialog::Settings);
                Ok(Action::None)
            }
            _ => Ok(Action::None),
        }
    }

    fn handle_shortcut_key(&mut self, key: KeyEvent) -> Result<Action> {
        match key.code {
            KeyCode::Esc => {
                self.dialog = self.shortcut_return_dialog.clone();
                self.shortcut_input.clear();
                return Ok(Action::None);
            }
            KeyCode::Enter => {
                let shortcut = self.shortcut_input.trim().to_string();
                if shortcut.is_empty() {
                    self.set_error("Ghostty shortcut cannot be empty");
                    return Ok(Action::None);
                }

                return Ok(Action::SetGhosttyShortcut(shortcut));
            }
            KeyCode::Backspace => {
                self.shortcut_input.pop();
                return Ok(Action::None);
            }
            _ => {}
        }

        if let Some(shortcut) = shortcut_string_for_key_event(key) {
            self.shortcut_input = shortcut;
            return Ok(Action::None);
        }

        match key.code {
            KeyCode::Char(c) if is_text_input(key.modifiers) => {
                self.shortcut_input.push(c);
                Ok(Action::None)
            }
            _ => Ok(Action::None),
        }
    }

    fn handle_help_key(&mut self, key: KeyEvent) -> Result<Action> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') => {
                self.dialog = Dialog::None;
                Ok(Action::None)
            }
            _ => Ok(Action::None),
        }
    }

    fn handle_search_key(&mut self, key: KeyEvent) -> Result<Action> {
        match key.code {
            KeyCode::Esc => {
                self.cancel_search();
                Ok(Action::None)
            }
            KeyCode::Enter => {
                self.commit_search();
                self.set_info(format!(
                    "Showing {} of {} workspaces",
                    self.visible_indices().len(),
                    self.workspaces.len()
                ));
                Ok(Action::None)
            }
            KeyCode::Backspace => {
                self.filter.pop();
                self.update_filter_after_edit();
                Ok(Action::None)
            }
            KeyCode::Up => {
                self.move_selection(-1);
                Ok(Action::None)
            }
            KeyCode::Down => {
                self.move_selection(1);
                Ok(Action::None)
            }
            KeyCode::PageUp => {
                self.move_selection(-self.page_step());
                Ok(Action::None)
            }
            KeyCode::PageDown => {
                self.move_selection(self.page_step());
                Ok(Action::None)
            }
            KeyCode::Char(c) if is_text_input(key.modifiers) => {
                self.filter.push(c);
                self.update_filter_after_edit();
                Ok(Action::None)
            }
            _ => Ok(Action::None),
        }
    }

    fn handle_main_key(&mut self, key: KeyEvent, env: &AppEnv) -> Result<Action> {
        if let KeyCode::Char(c) = key.code
            && should_start_quick_search(c, key.modifiers)
        {
            self.begin_search(Some(c));
            return Ok(Action::None);
        }

        match key.code {
            KeyCode::Char('q') => Ok(Action::Quit),
            KeyCode::Char('?') => {
                self.dialog = Dialog::Help;
                Ok(Action::None)
            }
            KeyCode::Char('/') => {
                self.begin_search(None);
                Ok(Action::None)
            }
            KeyCode::Esc => {
                if !self.filter.is_empty() {
                    self.filter.clear();
                    self.reset_visible_selection();
                    self.set_info("Cleared workspace filter");
                    return Ok(Action::None);
                }

                Ok(Action::Quit)
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('s') => {
                self.move_selection(1);
                Ok(Action::None)
            }
            KeyCode::Up | KeyCode::Char('k') | KeyCode::Char('w') => {
                self.move_selection(-1);
                Ok(Action::None)
            }
            KeyCode::Home => {
                self.move_to_start();
                Ok(Action::None)
            }
            KeyCode::End | KeyCode::Char('G') => {
                self.move_to_end();
                Ok(Action::None)
            }
            KeyCode::PageUp => {
                self.move_selection(-self.page_step());
                Ok(Action::None)
            }
            KeyCode::PageDown => {
                self.move_selection(self.page_step());
                Ok(Action::None)
            }
            KeyCode::Enter => {
                let Some(workspace) = self.selected_workspace() else {
                    self.set_error("No workspace selected");
                    return Ok(Action::None);
                };
                Ok(Action::Launch(workspace.name.clone()))
            }
            KeyCode::Char('a') => {
                self.dialog = Dialog::Save;
                self.save_input.clear();
                Ok(Action::None)
            }
            KeyCode::Char('e') => {
                let Some(workspace) = self.selected_workspace() else {
                    self.set_error("No workspace selected");
                    return Ok(Action::None);
                };
                Ok(Action::Edit(workspace.name.clone()))
            }
            KeyCode::Char('n') => {
                let Some(name) = self
                    .selected_workspace()
                    .map(|workspace| workspace.name.clone())
                else {
                    self.set_error("No workspace selected");
                    return Ok(Action::None);
                };
                self.open_rename(name);
                Ok(Action::None)
            }
            KeyCode::Char('d') => {
                if self.selected_workspace().is_some() {
                    self.dialog = Dialog::ConfirmDelete;
                } else {
                    self.set_error("No workspace selected");
                }
                Ok(Action::None)
            }
            KeyCode::Char('g') => {
                self.open_shortcut_editor(env, Dialog::None);
                Ok(Action::None)
            }
            KeyCode::Char('t') => {
                self.open_settings(env);
                Ok(Action::None)
            }
            KeyCode::Char('r') => Ok(Action::Refresh),
            _ => Ok(Action::None),
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent, env: &AppEnv) -> Result<Action> {
        if !matches!(self.dialog, Dialog::None) {
            return Ok(Action::None);
        }

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if self.shortcut_contains(mouse.column, mouse.row) {
                    self.clear_pending_click();
                    self.open_shortcut_editor(env, Dialog::None);
                    return Ok(Action::None);
                }

                let Some(index) = self.list_index_at(mouse.column, mouse.row) else {
                    self.clear_pending_click();
                    return Ok(Action::None);
                };

                self.selected = index;
                let clicked_at = Instant::now();
                if self.is_double_click(index, clicked_at) {
                    self.clear_pending_click();
                    let Some(workspace) = self.selected_workspace() else {
                        return Ok(Action::None);
                    };

                    return Ok(Action::Launch(workspace.name.clone()));
                }

                self.last_click = Some(ClickState {
                    index,
                    at: clicked_at,
                });
                let Some(workspace) = self.selected_workspace() else {
                    return Ok(Action::None);
                };

                self.set_info(format!("Selected \"{}\"", workspace.name));
                Ok(Action::None)
            }
            MouseEventKind::ScrollDown if self.list_contains(mouse.column, mouse.row) => {
                self.move_selection(1);
                Ok(Action::None)
            }
            MouseEventKind::ScrollUp if self.list_contains(mouse.column, mouse.row) => {
                self.move_selection(-1);
                Ok(Action::None)
            }
            _ => Ok(Action::None),
        }
    }

    fn list_index_at(&self, column: u16, row: u16) -> Option<usize> {
        if !self.list_contains(column, row) {
            return None;
        }

        let relative_row = row.saturating_sub(self.list_area.y) as usize;
        let index = self.list_offset + relative_row;
        (index < self.visible_indices().len()).then_some(index)
    }

    fn list_contains(&self, column: u16, row: u16) -> bool {
        column >= self.list_area.x
            && column < self.list_area.x.saturating_add(self.list_area.width)
            && row >= self.list_area.y
            && row < self.list_area.y.saturating_add(self.list_area.height)
    }

    fn shortcut_contains(&self, column: u16, row: u16) -> bool {
        column >= self.shortcut_area.x
            && column
                < self
                    .shortcut_area
                    .x
                    .saturating_add(self.shortcut_area.width)
            && row >= self.shortcut_area.y
            && row
                < self
                    .shortcut_area
                    .y
                    .saturating_add(self.shortcut_area.height)
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Action {
    None,
    Quit,
    Refresh,
    Launch(String),
    Save(String),
    Rename(String, String),
    Edit(String),
    Delete(String),
    ToggleCloseTab,
    SetGhosttyShortcut(String),
}

#[derive(Clone, Copy)]
struct Theme {
    accent: Style,
    emphasis: Style,
    muted: Style,
    dim: Style,
    success: Style,
    error: Style,
    warning: Style,
    selection: Style,
    border: Style,
    border_active: Style,
    titlebar: Style,
    titlebar_dim: Style,
    section: Style,
}

impl Theme {
    fn detect() -> Self {
        if env::var_os("NO_COLOR").is_some() {
            return Self {
                accent: Style::default().add_modifier(Modifier::BOLD),
                emphasis: Style::default().add_modifier(Modifier::BOLD),
                muted: Style::default().add_modifier(Modifier::DIM),
                dim: Style::default().add_modifier(Modifier::DIM),
                success: Style::default().add_modifier(Modifier::BOLD),
                error: Style::default().add_modifier(Modifier::BOLD),
                warning: Style::default().add_modifier(Modifier::BOLD),
                selection: Style::default().add_modifier(Modifier::BOLD),
                border: Style::default(),
                border_active: Style::default().add_modifier(Modifier::BOLD),
                titlebar: Style::default().add_modifier(Modifier::BOLD),
                titlebar_dim: Style::default().add_modifier(Modifier::DIM),
                section: Style::default().add_modifier(Modifier::BOLD),
            };
        }

        Self {
            accent: Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            emphasis: Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
            muted: Style::default().fg(Color::Gray),
            dim: Style::default().fg(Color::DarkGray),
            success: Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
            error: Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            warning: Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
            selection: Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            border: Style::default().fg(Color::DarkGray),
            border_active: Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            titlebar: Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            titlebar_dim: Style::default().fg(Color::Black).bg(Color::Cyan),
            section: Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        }
    }
}

fn draw(frame: &mut Frame<'_>, app: &mut App, env: &AppEnv) {
    let theme = Theme::detect();
    let area = frame.area();
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        draw_too_small(frame, area, &theme);
        return;
    }

    let shell_area = shell_rect(area);
    let shell = Block::default()
        .borders(Borders::ALL)
        .border_style(theme.border_active);
    let inner = shell.inner(shell_area);
    frame.render_widget(shell, shell_area);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(10), Constraint::Length(3)])
        .split(inner);

    draw_body(frame, layout[0], app, env, &theme);
    draw_footer(frame, layout[1], app, &theme);

    match app.dialog {
        Dialog::None => {}
        Dialog::Save => draw_save_dialog(frame, app, &theme),
        Dialog::Rename => draw_rename_dialog(frame, app, &theme),
        Dialog::ConfirmDelete => draw_delete_dialog(frame, app, &theme),
        Dialog::Settings => draw_settings_dialog(frame, app, env, &theme),
        Dialog::EditGhosttyShortcut => draw_shortcut_dialog(frame, app, env, &theme),
        Dialog::Help => draw_help_dialog(frame, &theme),
    }
}

fn draw_too_small(frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
    let text = Text::from(vec![
        Line::from(vec![
            Span::styled("gtab", theme.accent),
            Span::raw(" needs more room"),
        ]),
        Line::default(),
        Line::from(format!("Current terminal: {}x{}", area.width, area.height)),
        Line::from(format!("Recommended minimum: {}x{}", MIN_WIDTH, MIN_HEIGHT)),
        Line::default(),
        Line::from("Resize the terminal to show the dialog layout."),
    ]);

    frame.render_widget(
        Paragraph::new(text).alignment(Alignment::Center).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme.border_active)
                .title("Resize Required"),
        ),
        centered_rect(58, 40, area),
    );
}

fn draw_body(frame: &mut Frame<'_>, area: Rect, app: &mut App, env: &AppEnv, theme: &Theme) {
    let content = Block::default()
        .borders(Borders::ALL)
        .border_style(theme.border);
    let inner = content.inner(area);
    frame.render_widget(content, area);

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(MAIN_LIST_WIDTH), Constraint::Min(24)])
        .split(inner);

    draw_workspace_list(frame, chunks[0], app, theme);
    draw_workspace_detail(frame, chunks[1], app, env, theme);
}

fn draw_workspace_list(frame: &mut Frame<'_>, area: Rect, app: &mut App, theme: &Theme) {
    let panel = Block::default()
        .borders(Borders::RIGHT)
        .border_style(theme.border);
    let inner = panel.inner(area);
    frame.render_widget(panel, area);
    app.list_area = inner;
    let visible = app.visible_workspaces();
    let items: Vec<ListItem<'_>> = if visible.is_empty() {
        vec![ListItem::new(Line::from(vec![Span::styled(
            "no matches",
            theme.muted,
        )]))]
    } else {
        visible
            .iter()
            .map(|workspace| {
                ListItem::new(Span::styled(
                    format!("[{}]", workspace.name),
                    theme.emphasis,
                ))
            })
            .collect()
    };

    let mut state = ListState::default()
        .with_selected((!visible.is_empty()).then_some(app.selected))
        .with_offset(app.list_offset);

    let list = List::new(items).highlight_style(theme.selection);

    frame.render_stateful_widget(list, inner, &mut state);
    app.list_offset = state.offset();
}

fn draw_workspace_detail(
    frame: &mut Frame<'_>,
    area: Rect,
    app: &mut App,
    env: &AppEnv,
    theme: &Theme,
) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    draw_workspace_tabs(frame, chunks[0], app, theme);
    draw_quick_settings(frame, chunks[1], app, env, theme);
}

fn draw_workspace_tabs(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme) {
    let panel = Block::default()
        .borders(Borders::RIGHT)
        .border_style(theme.border);
    let inner = Rect::new(
        area.x.saturating_add(1),
        area.y,
        area.width.saturating_sub(2),
        area.height,
    );

    frame.render_widget(panel, area);
    frame.render_widget(
        Paragraph::new(workspace_tabs_text(app, theme)).wrap(Wrap { trim: false }),
        inner,
    );
}

fn draw_quick_settings(
    frame: &mut Frame<'_>,
    area: Rect,
    app: &mut App,
    env: &AppEnv,
    theme: &Theme,
) {
    let inner = Rect::new(
        area.x.saturating_add(1),
        area.y,
        area.width.saturating_sub(2),
        area.height,
    );
    app.shortcut_area = Rect::new(inner.x, inner.y.saturating_add(1), inner.width, 1);

    frame.render_widget(
        Paragraph::new(quick_settings_text(app, env, inner.width, theme))
            .wrap(Wrap { trim: false }),
        inner,
    );
}

fn workspace_tabs_text(app: &App, theme: &Theme) -> Text<'static> {
    let Some(workspace) = app.selected_workspace() else {
        return Text::default();
    };

    if workspace.tabs.is_empty() {
        return Text::default();
    }

    let mut spans = Vec::with_capacity(workspace.tabs.len().saturating_mul(2));
    for tab in &workspace.tabs {
        spans.push(Span::styled(format!("「{}」", tab.title), theme.accent));
        spans.push(Span::raw(" "));
    }

    Text::from(Line::from(spans))
}

fn quick_settings_text(_app: &App, env: &AppEnv, width: u16, theme: &Theme) -> Text<'static> {
    let shortcut = env.ghostty_shortcut_display().to_string();

    Text::from(vec![
        section_line(width, "Quick Settings", theme),
        joined_line(
            Rect::new(0, 0, width, 1),
            &format!("shortcut {shortcut}"),
            "click / g",
            theme.warning,
            theme.dim,
        ),
        Line::default(),
        section_line(width, "Status", theme),
        Line::from(vec![Span::styled("same-shell in Ghostty", theme.warning)]),
        Line::from("Ghostty-local only"),
        Line::default(),
        Line::from(vec![
            Span::styled("t", theme.accent),
            Span::raw(" full settings"),
        ]),
    ])
}

fn draw_footer(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme) {
    let status = app
        .status
        .as_ref()
        .map(|status| {
            let (label, style) = match status.kind {
                StatusKind::Info => ("[i]", theme.muted),
                StatusKind::Success => ("[ok]", theme.success),
                StatusKind::Error => ("[!!]", theme.error),
            };

            Line::from(vec![
                Span::styled(format!("{label} "), style),
                Span::raw(status.text.clone()),
            ])
        })
        .unwrap_or_else(|| Line::from(vec![Span::styled("[ ] ready", theme.muted)]));

    let keys = if app.dialog == Dialog::Help {
        Line::from(vec![
            Span::styled("Esc", theme.accent),
            Span::raw(" close  "),
            Span::styled("q", theme.accent),
            Span::raw(" close"),
        ])
    } else if matches!(app.dialog, Dialog::Save) {
        Line::from(vec![
            Span::styled("Enter", theme.accent),
            Span::raw(" save  "),
            Span::styled("Esc", theme.accent),
            Span::raw(" cancel"),
        ])
    } else if matches!(app.dialog, Dialog::Rename) {
        Line::from(vec![
            Span::styled("Enter", theme.accent),
            Span::raw(" rename  "),
            Span::styled("Esc", theme.accent),
            Span::raw(" cancel"),
        ])
    } else if matches!(app.dialog, Dialog::ConfirmDelete) {
        Line::from(vec![
            Span::styled("y", theme.accent),
            Span::raw(" confirm  "),
            Span::styled("n", theme.accent),
            Span::raw(" cancel"),
        ])
    } else if matches!(app.dialog, Dialog::Settings) {
        Line::from(vec![
            Span::styled("c", theme.accent),
            Span::raw(" toggle  "),
            Span::styled("g", theme.accent),
            Span::raw(" ghostty shortcut  "),
            Span::styled("Esc", theme.accent),
            Span::raw(" close"),
        ])
    } else if matches!(app.dialog, Dialog::EditGhosttyShortcut) {
        Line::from(vec![
            Span::styled("Enter", theme.accent),
            Span::raw(" save  "),
            Span::styled("Esc", theme.accent),
            Span::raw(" back"),
        ])
    } else if app.search_active() {
        Line::from(vec![
            Span::styled("type", theme.accent),
            Span::raw(" filter  "),
            Span::styled("Enter", theme.accent),
            Span::raw(" keep  "),
            Span::styled("Esc", theme.accent),
            Span::raw(" revert"),
        ])
    } else {
        Line::from(vec![
            Span::styled("Enter", theme.accent),
            Span::raw(" launch  "),
            Span::styled("/", theme.accent),
            Span::raw(" filter  "),
            Span::styled("a", theme.accent),
            Span::raw(" save  "),
            Span::styled("n", theme.accent),
            Span::raw(" rename  "),
            Span::styled("d", theme.accent),
            Span::raw(" remove  "),
            Span::styled("t", theme.accent),
            Span::raw(" settings  "),
            Span::styled("g", theme.accent),
            Span::raw(" ghostty shortcut  "),
            Span::styled("?", theme.accent),
            Span::raw(" help  "),
            Span::styled("q", theme.accent),
            Span::raw(" quit"),
        ])
    };

    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(theme.border);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let footer_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(inner);

    frame.render_widget(Paragraph::new(status), footer_layout[0]);
    frame.render_widget(Paragraph::new(keys), footer_layout[1]);
}

fn draw_save_dialog(frame: &mut Frame<'_>, app: &App, theme: &Theme) {
    let area = centered_rect(58, 34, frame.area());
    let inner = draw_dialog_shell(
        frame,
        area,
        "Save Workspace",
        "Enter save | Esc cancel",
        theme,
    );
    frame.render_widget(
        Paragraph::new(Text::from(vec![
            section_line(inner.width, "Current Window", theme),
            Line::from("Save the frontmost Ghostty window as a workspace."),
            Line::default(),
            section_line(inner.width, "Name", theme),
            Line::from(Span::styled(
                if app.save_input.is_empty() {
                    "..."
                } else {
                    app.save_input.as_str()
                },
                theme.accent,
            )),
        ]))
        .wrap(Wrap { trim: true }),
        inner,
    );
}

fn draw_rename_dialog(frame: &mut Frame<'_>, app: &App, theme: &Theme) {
    let workspace_name = app.rename_original.as_deref().unwrap_or("this workspace");
    let area = centered_rect(58, 36, frame.area());
    let inner = draw_dialog_shell(
        frame,
        area,
        "Rename Workspace",
        "Enter rename | Esc cancel",
        theme,
    );
    frame.render_widget(
        Paragraph::new(Text::from(vec![
            section_line(inner.width, "Selection", theme),
            Line::from(Span::styled(workspace_name, theme.emphasis)),
            Line::default(),
            section_line(inner.width, "New Name", theme),
            Line::from(Span::styled(
                if app.rename_input.is_empty() {
                    "..."
                } else {
                    app.rename_input.as_str()
                },
                theme.accent,
            )),
        ]))
        .wrap(Wrap { trim: true }),
        inner,
    );
}

fn draw_delete_dialog(frame: &mut Frame<'_>, app: &App, theme: &Theme) {
    let workspace_name = app
        .selected_workspace()
        .map(|workspace| workspace.name.as_str())
        .unwrap_or("this workspace");

    let area = centered_rect(56, 34, frame.area());
    let inner = draw_dialog_shell(
        frame,
        area,
        "Delete Workspace",
        "y confirm | n cancel",
        theme,
    );
    frame.render_widget(
        Paragraph::new(Text::from(vec![
            section_line(inner.width, "Selection", theme),
            Line::from(vec![
                Span::styled("Delete ", theme.error),
                Span::styled(format!("\"{workspace_name}\""), theme.emphasis),
                Span::raw("?"),
            ]),
            Line::default(),
            section_line(inner.width, "Effect", theme),
            Line::from("This removes the saved AppleScript file."),
            Line::from("The action cannot be undone from gtab."),
        ]))
        .wrap(Wrap { trim: true }),
        inner,
    );
}

fn draw_settings_dialog(frame: &mut Frame<'_>, _app: &App, env: &AppEnv, theme: &Theme) {
    let area = centered_rect(68, 52, frame.area());
    let inner = draw_dialog_shell(
        frame,
        area,
        "Settings",
        "c toggle | g ghostty shortcut | Esc close",
        theme,
    );
    frame.render_widget(
        Paragraph::new(Text::from(vec![
            section_line(inner.width, "Workspace", theme),
            Line::from(vec![
                Span::styled("close_tab ", theme.dim),
                Span::styled(env.close_tab_display(), theme.warning),
            ]),
            Line::default(),
            section_line(inner.width, "Shortcut", theme),
            Line::from(vec![
                Span::styled("ghostty   ", theme.dim),
                Span::styled(env.ghostty_shortcut_display(), theme.warning),
            ]),
            Line::from(vec![
                Span::styled("mode      ", theme.dim),
                Span::styled("same-shell launch", theme.warning),
            ]),
            Line::from(vec![
                Span::styled("scope     ", theme.dim),
                Span::styled("Ghostty current shell only", theme.warning),
            ]),
        ]))
        .wrap(Wrap { trim: true }),
        inner,
    );
}

fn draw_shortcut_dialog(frame: &mut Frame<'_>, app: &App, env: &AppEnv, theme: &Theme) {
    let area = centered_rect(62, 38, frame.area());
    let inner = draw_dialog_shell(
        frame,
        area,
        "Edit Ghostty Shortcut",
        "Enter save | Esc back",
        theme,
    );
    let current_input = if app.shortcut_input.is_empty() {
        env.ghostty_shortcut_display()
    } else {
        app.shortcut_input.as_str()
    };

    frame.render_widget(
        Paragraph::new(Text::from(vec![
            section_line(inner.width, "Shortcut", theme),
            Line::from(vec![Span::styled(current_input, theme.accent)]),
            Line::default(),
            section_line(inner.width, "Input", theme),
            Line::from("Press the shortcut directly, or type it manually."),
            Line::from("This types `gtab` into the focused Ghostty shell."),
            Line::default(),
            section_line(inner.width, "Examples", theme),
            Line::from("cmd+g"),
            Line::from("cmd+shift+g"),
            Line::from("off"),
        ]))
        .wrap(Wrap { trim: true }),
        inner,
    );
}

fn draw_help_dialog(frame: &mut Frame<'_>, theme: &Theme) {
    let area = centered_rect(68, 58, frame.area());
    let inner = draw_dialog_shell(frame, area, "Help", "Esc close | q close", theme);
    frame.render_widget(
        Paragraph::new(Text::from(vec![
            section_line(inner.width, "Move", theme),
            Line::from("j / k / arrows  PgUp / PgDn  Home / End / G"),
            Line::default(),
            section_line(inner.width, "Search", theme),
            Line::from("/ starts filter"),
            Line::from("Enter keep  Esc revert"),
            Line::default(),
            section_line(inner.width, "Actions", theme),
            Line::from("Enter launch  a save  n rename  e edit  d remove"),
            Line::from("g edit Ghostty shortcut  r reload  t settings"),
            Line::from("q quit"),
            Line::default(),
            section_line(inner.width, "Layout", theme),
            Line::from("Left pane lists saved workspaces."),
            Line::from("Middle pane shows saved tabs in order."),
            Line::from("Right pane shows the Ghostty shortcut first."),
            Line::default(),
            section_line(inner.width, "Mouse", theme),
            Line::from("click select  double-click launch"),
            Line::from("click shortcut to edit  wheel move"),
        ]))
        .wrap(Wrap { trim: true }),
        inner,
    );
}

fn draw_dialog_shell(
    frame: &mut Frame<'_>,
    area: Rect,
    title: &str,
    footer: &str,
    theme: &Theme,
) -> Rect {
    frame.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme.border_active);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(2),
        ])
        .split(inner);

    frame.render_widget(
        Paragraph::new(joined_line(
            layout[0],
            title,
            "x",
            theme.titlebar,
            theme.titlebar_dim,
        ))
        .style(theme.titlebar),
        layout[0],
    );
    let footer_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(layout[2]);
    frame.render_widget(
        Block::default()
            .borders(Borders::TOP)
            .border_style(theme.border),
        footer_layout[0],
    );
    frame.render_widget(
        Paragraph::new(Line::from(footer)).style(theme.dim),
        footer_layout[1],
    );

    layout[1]
}

fn shell_rect(area: Rect) -> Rect {
    let horizontal_margin = if area.width >= MIN_WIDTH + 10 { 2 } else { 0 };
    let vertical_margin = if area.height >= MIN_HEIGHT + 6 { 1 } else { 0 };

    Rect::new(
        area.x.saturating_add(horizontal_margin),
        area.y.saturating_add(vertical_margin),
        area.width
            .saturating_sub(horizontal_margin.saturating_mul(2)),
        area.height
            .saturating_sub(vertical_margin.saturating_mul(2)),
    )
}

fn joined_line(
    area: Rect,
    left: &str,
    right: &str,
    left_style: Style,
    right_style: Style,
) -> Line<'static> {
    let width = area.width.max(1) as usize;
    let mut left = left.to_string();
    let mut right = right.to_string();

    if left.chars().count() + right.chars().count() + 1 > width {
        let right_cap = (width / 2).max(12);
        right = fit_text(&right, right_cap.min(width.saturating_sub(1)));
        let remaining = width.saturating_sub(right.chars().count() + 1);
        left = fit_text(&left, remaining.max(1));
    }

    let left_width = left.chars().count();
    let right_width = right.chars().count();
    let gap = width.saturating_sub(left_width + right_width).max(1);

    Line::from(vec![
        Span::styled(left, left_style),
        Span::raw(" ".repeat(gap)),
        Span::styled(right, right_style),
    ])
}

fn section_line(width: u16, label: &str, theme: &Theme) -> Line<'static> {
    let label = label.to_ascii_uppercase();
    let fill_width = (width as usize)
        .saturating_sub(label.chars().count() + 1)
        .clamp(6, 40);

    Line::from(vec![
        Span::styled(label, theme.section),
        Span::raw(" "),
        Span::styled("─".repeat(fill_width), theme.dim),
    ])
}

fn fit_text(text: &str, max_width: usize) -> String {
    if text.chars().count() <= max_width {
        return text.to_string();
    }

    if max_width <= 3 {
        return ".".repeat(max_width);
    }

    let keep = max_width - 3;
    let prefix: String = text.chars().take(keep).collect();
    format!("{prefix}...")
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn display_path(path: &Path) -> String {
    let raw = path.display().to_string();
    let home = env::var("HOME").ok();
    match home {
        Some(home) if raw == home => "~".to_string(),
        Some(home) if raw.starts_with(&(home.clone() + "/")) => raw.replacen(&home, "~", 1),
        _ => raw,
    }
}

fn is_text_input(modifiers: KeyModifiers) -> bool {
    !modifiers.intersects(
        KeyModifiers::CONTROL
            | KeyModifiers::ALT
            | KeyModifiers::SUPER
            | KeyModifiers::HYPER
            | KeyModifiers::META,
    )
}

fn should_start_quick_search(c: char, modifiers: KeyModifiers) -> bool {
    if !is_text_input(modifiers) || c.is_control() || c.is_whitespace() {
        return false;
    }

    !matches!(
        c.to_ascii_lowercase(),
        '/' | '?' | 'a' | 'd' | 'e' | 'g' | 'j' | 'k' | 'n' | 'q' | 'r' | 's' | 't' | 'w'
    )
}

fn shortcut_string_for_key_event(key: KeyEvent) -> Option<String> {
    let captures_modified_key = key.modifiers.intersects(
        KeyModifiers::SUPER
            | KeyModifiers::CONTROL
            | KeyModifiers::ALT
            | KeyModifiers::HYPER
            | KeyModifiers::META,
    );
    let captures_named_key = !matches!(key.code, KeyCode::Char(_));
    if !captures_modified_key && !captures_named_key {
        return None;
    }

    let key_name = shortcut_key_name(key.code)?;
    let mut parts = Vec::new();
    if key.modifiers.contains(KeyModifiers::SUPER) {
        parts.push("cmd".to_string());
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        parts.push("ctrl".to_string());
    }
    if key.modifiers.contains(KeyModifiers::ALT) {
        parts.push("alt".to_string());
    }
    if key.modifiers.contains(KeyModifiers::SHIFT) {
        parts.push("shift".to_string());
    }
    parts.push(key_name);
    Some(parts.join("+"))
}

fn shortcut_key_name(code: KeyCode) -> Option<String> {
    match code {
        KeyCode::Char(c) => shortcut_key_name_for_char(c).map(str::to_string),
        KeyCode::Enter => Some("enter".to_string()),
        KeyCode::Tab | KeyCode::BackTab => Some("tab".to_string()),
        KeyCode::Esc => Some("esc".to_string()),
        KeyCode::Backspace => Some("backspace".to_string()),
        KeyCode::Delete => Some("delete".to_string()),
        KeyCode::Left => Some("left".to_string()),
        KeyCode::Right => Some("right".to_string()),
        KeyCode::Up => Some("up".to_string()),
        KeyCode::Down => Some("down".to_string()),
        _ => None,
    }
}

fn shortcut_key_name_for_char(c: char) -> Option<&'static str> {
    Some(match c {
        'a'..='z' => match c {
            'a' => "a",
            'b' => "b",
            'c' => "c",
            'd' => "d",
            'e' => "e",
            'f' => "f",
            'g' => "g",
            'h' => "h",
            'i' => "i",
            'j' => "j",
            'k' => "k",
            'l' => "l",
            'm' => "m",
            'n' => "n",
            'o' => "o",
            'p' => "p",
            'q' => "q",
            'r' => "r",
            's' => "s",
            't' => "t",
            'u' => "u",
            'v' => "v",
            'w' => "w",
            'x' => "x",
            'y' => "y",
            'z' => "z",
            _ => unreachable!(),
        },
        'A'..='Z' => return shortcut_key_name_for_char(c.to_ascii_lowercase()),
        '0' => "0",
        '1' => "1",
        '2' => "2",
        '3' => "3",
        '4' => "4",
        '5' => "5",
        '6' => "6",
        '7' => "7",
        '8' => "8",
        '9' => "9",
        '`' | '~' => "`",
        '-' | '_' => "-",
        '=' | '+' => "=",
        '[' | '{' => "[",
        ']' | '}' => "]",
        '\\' | '|' => "\\",
        ';' | ':' => ";",
        '\'' | '"' => "'",
        ',' | '<' => ",",
        '.' | '>' => ".",
        '/' | '?' => "/",
        ' ' => "space",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{Config, WorkspaceTab};
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    use std::path::PathBuf;

    fn workspace(name: &str) -> Workspace {
        Workspace {
            name: name.to_string(),
            path: PathBuf::from(format!("/tmp/{name}.applescript")),
            tabs: vec![WorkspaceTab {
                title: "tab".to_string(),
                working_dir: Some("/tmp/project".to_string()),
            }],
        }
    }

    fn left_click(column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    fn env() -> AppEnv {
        AppEnv {
            base_dir: PathBuf::from("/tmp/gtab"),
            config_file: PathBuf::from("/tmp/gtab/config"),
            config: Config {
                close_tab: true,
                ghostty_shortcut: "cmd+g".to_string(),
            },
        }
    }

    fn text_lines(text: Text<'static>) -> Vec<String> {
        text.lines
            .into_iter()
            .map(|line| {
                line.spans
                    .into_iter()
                    .map(|span| span.content.to_string())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn single_click_selects_and_double_click_launches() {
        let mut app = App::new(vec![workspace("alpha"), workspace("beta")]);
        app.list_area = Rect::new(0, 0, 40, 6);

        assert_eq!(
            app.handle_mouse(left_click(1, 1), &env()).unwrap(),
            Action::None
        );
        assert_eq!(app.selected, 1);

        assert_eq!(
            app.handle_mouse(left_click(1, 1), &env()).unwrap(),
            Action::Launch("beta".to_string())
        );
    }

    #[test]
    fn search_escape_restores_previous_filter() {
        let mut app = App::new(vec![workspace("alpha"), workspace("beta")]);
        app.filter = "al".to_string();
        app.begin_search(Some('p'));

        assert_eq!(app.filter, "alp");
        app.cancel_search();
        assert_eq!(app.filter, "al");
        assert!(!app.search_active());
    }

    #[test]
    fn quick_search_skips_reserved_action_keys() {
        assert!(should_start_quick_search('x', KeyModifiers::NONE));
        assert!(should_start_quick_search('1', KeyModifiers::NONE));
        assert!(should_start_quick_search('p', KeyModifiers::NONE));
        assert!(!should_start_quick_search('a', KeyModifiers::NONE));
        assert!(should_start_quick_search('c', KeyModifiers::NONE));
        assert!(!should_start_quick_search('g', KeyModifiers::NONE));
        assert!(!should_start_quick_search('n', KeyModifiers::NONE));
        assert!(!should_start_quick_search('q', KeyModifiers::NONE));
    }

    #[test]
    fn quick_settings_show_shortcut_status() {
        let theme = Theme::detect();
        let app = App::new(vec![workspace("alpha")]);

        let lines = text_lines(quick_settings_text(&app, &env(), 28, &theme));

        assert!(lines.iter().any(|line| line.contains("shortcut cmd+g")));
        assert!(lines.iter().any(|line| line.contains("click / g")));
        assert!(
            lines
                .iter()
                .any(|line| line.contains("same-shell in Ghostty"))
        );
        assert!(lines.iter().any(|line| line.contains("Ghostty-local only")));
        assert!(!lines.iter().any(|line| line.contains("close_tab")));
    }

    #[test]
    fn main_screen_g_opens_shortcut_editor() {
        let mut app = App::new(vec![workspace("alpha")]);

        assert_eq!(
            app.handle_main_key(KeyEvent::from(KeyCode::Char('g')), &env())
                .unwrap(),
            Action::None
        );
        assert_eq!(app.dialog, Dialog::EditGhosttyShortcut);
        assert_eq!(app.shortcut_return_dialog, Dialog::None);
        assert_eq!(app.shortcut_input, "cmd+g");
    }

    #[test]
    fn main_screen_n_opens_rename_dialog_with_existing_name() {
        let mut app = App::new(vec![workspace("alpha")]);

        assert_eq!(
            app.handle_main_key(KeyEvent::from(KeyCode::Char('n')), &env())
                .unwrap(),
            Action::None
        );
        assert_eq!(app.dialog, Dialog::Rename);
        assert_eq!(app.rename_original.as_deref(), Some("alpha"));
        assert_eq!(app.rename_input, "alpha");
    }

    #[test]
    fn rename_dialog_returns_rename_action() {
        let mut app = App::new(vec![workspace("alpha")]);
        app.open_rename("alpha".to_string());
        app.rename_input = "beta".to_string();

        assert_eq!(
            app.handle_rename_key(KeyEvent::from(KeyCode::Enter))
                .unwrap(),
            Action::Rename("alpha".to_string(), "beta".to_string())
        );
    }

    #[test]
    fn rename_dialog_closes_without_action_when_name_is_unchanged() {
        let mut app = App::new(vec![workspace("alpha")]);
        app.open_rename("alpha".to_string());

        assert_eq!(
            app.handle_rename_key(KeyEvent::from(KeyCode::Enter))
                .unwrap(),
            Action::None
        );
        assert_eq!(app.dialog, Dialog::None);
        assert!(app.rename_original.is_none());
        assert_eq!(
            app.status.as_ref().map(|status| status.text.as_str()),
            Some("Workspace name unchanged")
        );
    }

    #[test]
    fn save_dialog_rejects_empty_name() {
        let mut app = App::new(vec![workspace("alpha")]);
        app.dialog = Dialog::Save;
        app.save_input = "   ".to_string();

        assert_eq!(
            app.handle_save_key(KeyEvent::from(KeyCode::Enter)).unwrap(),
            Action::None
        );
        assert_eq!(app.dialog, Dialog::Save);
        assert_eq!(
            app.status.as_ref().map(|status| status.text.as_str()),
            Some("Workspace name cannot be empty")
        );
    }

    #[test]
    fn main_screen_q_returns_quit_action() {
        let mut app = App::new(vec![workspace("alpha")]);

        assert_eq!(
            app.handle_main_key(KeyEvent::from(KeyCode::Char('q')), &env())
                .unwrap(),
            Action::Quit
        );
    }

    #[test]
    fn main_screen_enter_without_selection_sets_error() {
        let mut app = App::new(vec![workspace("alpha")]);
        app.filter = "zzz".to_string();

        assert_eq!(
            app.handle_main_key(KeyEvent::from(KeyCode::Enter), &env())
                .unwrap(),
            Action::None
        );
        assert_eq!(
            app.status.as_ref().map(|status| status.text.as_str()),
            Some("No workspace selected")
        );
    }

    #[test]
    fn main_screen_delete_without_selection_sets_error() {
        let mut app = App::new(vec![workspace("alpha")]);
        app.filter = "zzz".to_string();

        assert_eq!(
            app.handle_main_key(KeyEvent::from(KeyCode::Char('d')), &env())
                .unwrap(),
            Action::None
        );
        assert_eq!(app.dialog, Dialog::None);
        assert_eq!(
            app.status.as_ref().map(|status| status.text.as_str()),
            Some("No workspace selected")
        );
    }

    #[test]
    fn main_screen_edit_without_selection_sets_error() {
        let mut app = App::new(vec![workspace("alpha")]);
        app.filter = "zzz".to_string();

        assert_eq!(
            app.handle_main_key(KeyEvent::from(KeyCode::Char('e')), &env())
                .unwrap(),
            Action::None
        );
        assert_eq!(
            app.status.as_ref().map(|status| status.text.as_str()),
            Some("No workspace selected")
        );
    }

    #[test]
    fn main_screen_escape_clears_filter_before_quitting() {
        let mut app = App::new(vec![workspace("alpha"), workspace("beta")]);
        app.filter = "al".to_string();

        assert_eq!(
            app.handle_main_key(KeyEvent::from(KeyCode::Esc), &env())
                .unwrap(),
            Action::None
        );
        assert!(app.filter.is_empty());
        assert_eq!(
            app.status.as_ref().map(|status| status.text.as_str()),
            Some("Cleared workspace filter")
        );
    }

    #[test]
    fn shortcut_capture_formats_modified_char_keys() {
        let shortcut = shortcut_string_for_key_event(KeyEvent::new(
            KeyCode::Char('?'),
            KeyModifiers::SUPER | KeyModifiers::SHIFT,
        ));

        assert_eq!(shortcut.as_deref(), Some("cmd+shift+/"));
    }

    #[test]
    fn shortcut_capture_formats_named_keys() {
        let shortcut =
            shortcut_string_for_key_event(KeyEvent::new(KeyCode::Left, KeyModifiers::SUPER));

        assert_eq!(shortcut.as_deref(), Some("cmd+left"));
    }

    #[test]
    fn clicking_shortcut_opens_shortcut_editor() {
        let mut app = App::new(vec![workspace("alpha")]);
        app.shortcut_area = Rect::new(30, 2, 20, 1);

        assert_eq!(
            app.handle_mouse(left_click(31, 2), &env()).unwrap(),
            Action::None
        );
        assert_eq!(app.dialog, Dialog::EditGhosttyShortcut);
        assert_eq!(app.shortcut_return_dialog, Dialog::None);
        assert_eq!(app.shortcut_input, "cmd+g");
    }

    #[test]
    fn shortcut_dialog_records_modified_keys() {
        let mut app = App::new(vec![workspace("alpha")]);

        assert_eq!(
            app.handle_shortcut_key(KeyEvent::new(
                KeyCode::Char('G'),
                KeyModifiers::SUPER | KeyModifiers::SHIFT,
            ))
            .unwrap(),
            Action::None
        );
        assert_eq!(app.shortcut_input, "cmd+shift+g");
    }

    #[test]
    fn settings_shortcut_escape_returns_to_settings_dialog() {
        let mut app = App::new(vec![workspace("alpha")]);
        app.open_shortcut_editor(&env(), Dialog::Settings);

        assert_eq!(
            app.handle_shortcut_key(KeyEvent::from(KeyCode::Esc))
                .unwrap(),
            Action::None
        );
        assert_eq!(app.dialog, Dialog::Settings);
        assert!(app.shortcut_input.is_empty());
    }

    #[test]
    fn settings_dialog_space_toggles_close_tab() {
        let mut app = App::new(vec![workspace("alpha")]);
        app.dialog = Dialog::Settings;

        assert_eq!(
            app.handle_settings_key(KeyEvent::from(KeyCode::Char(' ')), &env())
                .unwrap(),
            Action::ToggleCloseTab
        );
    }

    #[test]
    fn search_enter_commits_filter_and_sets_status() {
        let mut app = App::new(vec![workspace("alpha"), workspace("beta")]);
        app.begin_search(Some('l'));

        assert_eq!(
            app.handle_search_key(KeyEvent::from(KeyCode::Enter))
                .unwrap(),
            Action::None
        );
        assert!(!app.search_active());
        assert_eq!(
            app.status.as_ref().map(|status| status.text.as_str()),
            Some("Showing 1 of 2 workspaces")
        );
    }

    #[test]
    fn workspace_tabs_follow_applescript_order() {
        let theme = Theme::detect();
        let app = App::new(vec![Workspace {
            name: "alpha".to_string(),
            path: PathBuf::from("/tmp/alpha.applescript"),
            tabs: vec![
                WorkspaceTab {
                    title: "api".to_string(),
                    working_dir: Some("/tmp/project/api".to_string()),
                },
                WorkspaceTab {
                    title: "worker".to_string(),
                    working_dir: Some("/tmp/project/worker".to_string()),
                },
            ],
        }]);

        let lines = text_lines(workspace_tabs_text(&app, &theme));

        assert_eq!(lines, vec!["「api」 「worker」 ".to_string()]);
    }

    #[test]
    fn workspace_tabs_are_empty_without_visible_selection() {
        let theme = Theme::detect();
        let mut app = App::new(vec![workspace("alpha"), workspace("beta")]);
        app.filter = "zzz".to_string();

        let lines = text_lines(workspace_tabs_text(&app, &theme));

        assert!(lines.is_empty());
    }

    #[test]
    fn workspace_tabs_are_empty_when_workspace_has_no_tabs() {
        let theme = Theme::detect();
        let app = App::new(vec![Workspace {
            name: "empty".to_string(),
            path: PathBuf::from("/tmp/empty.applescript"),
            tabs: vec![],
        }]);

        let lines = text_lines(workspace_tabs_text(&app, &theme));

        assert!(lines.is_empty());
    }
}
