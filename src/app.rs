use crate::core::{AppEnv, GhosttyShortcutSync, Workspace};
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
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Padding, Paragraph, Wrap},
};
use std::{
    io::{self, Stdout},
    time::{Duration, Instant},
};

const SPLASH_MS: u64 = 850;
const DOUBLE_CLICK_MS: u64 = 350;

pub fn run_tui(env: &mut AppEnv) -> Result<()> {
    let mut terminal = TerminalSession::start()?;
    let sync_status = env.ensure_ghostty_shortcut();
    let mut app = App::new(env.list_workspaces()?);

    match sync_status {
        Ok(true) => app.set_info(format!(
            "Ghostty shortcut {} synced. Reload Ghostty config or restart Ghostty.",
            env.ghostty_shortcut_display()
        )),
        Ok(false) => {}
        Err(error) => app.set_error(format!("Ghostty shortcut sync failed: {error}")),
    }

    loop {
        terminal.draw(|frame| draw(frame, &mut app, env))?;

        if let Some(expiry) = app.status_expiry {
            if Instant::now() >= expiry {
                app.clear_status();
            }
        }

        if !event::poll(Duration::from_millis(60)).context("failed to poll terminal events")? {
            continue;
        }

        match event::read().context("failed to read terminal event")? {
            Event::Key(key) => {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                if app.is_splash_visible() {
                    app.dismiss_splash();
                    continue;
                }

                match app.handle_key(key, env)? {
                    Action::None => {}
                    Action::Quit => break,
                    Action::Launch(name) => {
                        terminal.suspend()?;
                        let result = env.launch_workspace(&name);
                        terminal.resume()?;
                        result?;
                        break;
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
                                    path.display()
                                ));
                            }
                            Err(error) => {
                                app.set_error(error.to_string());
                            }
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
                            Ok(sync) => {
                                app.reset_dialogs();
                                app.set_success(shortcut_sync_message(&sync));
                            }
                            Err(error) => app.set_error(error.to_string()),
                        }
                    }
                }
            }
            Event::Mouse(mouse) => {
                if app.is_splash_visible() {
                    app.dismiss_splash();
                    continue;
                }

                match app.handle_mouse(mouse)? {
                    Action::None => {}
                    Action::Launch(name) => {
                        terminal.suspend()?;
                        let result = env.launch_workspace(&name);
                        terminal.resume()?;
                        result?;
                        break;
                    }
                    action => unreachable!("unexpected mouse action: {action:?}"),
                }
            }
            _ => continue,
        };
    }

    Ok(())
}

struct TerminalSession {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TerminalSession {
    fn start() -> Result<Self> {
        enable_raw_mode().context("failed to enable raw mode")?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture, Hide)
            .context("failed to enter alternate screen")?;
        let terminal = Terminal::new(CrosstermBackend::new(stdout))
            .context("failed to initialize terminal backend")?;
        Ok(Self { terminal })
    }

    fn draw(&mut self, f: impl FnOnce(&mut Frame<'_>)) -> Result<()> {
        self.terminal.draw(f).context("failed to draw frame")?;
        Ok(())
    }

    fn suspend(&mut self) -> Result<()> {
        disable_raw_mode().context("failed to disable raw mode")?;
        execute!(
            self.terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture,
            Show
        )
        .context("failed to leave alternate screen")?;
        self.terminal.show_cursor().ok();
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
        Ok(())
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
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

#[derive(Clone, Debug)]
enum Dialog {
    None,
    Save,
    ConfirmDelete,
    Settings,
    EditGhosttyShortcut,
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
    last_click: Option<ClickState>,
    filter: String,
    show_preview: bool,
    dialog: Dialog,
    save_input: String,
    shortcut_input: String,
    splash_started_at: Instant,
    splash_visible: bool,
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
            last_click: None,
            filter: String::new(),
            show_preview: true,
            dialog: Dialog::None,
            save_input: String::new(),
            shortcut_input: String::new(),
            splash_started_at: Instant::now(),
            splash_visible: true,
            status: Some(StatusLine {
                kind: StatusKind::Info,
                text: "Click selects, double-click or Enter launches, w/s moves, a saves."
                    .to_string(),
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
        self.shortcut_input.clear();
    }

    fn dismiss_splash(&mut self) {
        self.splash_visible = false;
    }

    fn is_splash_visible(&self) -> bool {
        self.splash_visible && self.splash_started_at.elapsed() < Duration::from_millis(SPLASH_MS)
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
            Dialog::ConfirmDelete => self.handle_delete_key(key),
            Dialog::Settings => self.handle_settings_key(key, env),
            Dialog::EditGhosttyShortcut => self.handle_shortcut_key(key),
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
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.save_input.push(c);
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
                self.dialog = Dialog::EditGhosttyShortcut;
                self.shortcut_input = env.ghostty_shortcut_display().to_string();
                Ok(Action::None)
            }
            _ => Ok(Action::None),
        }
    }

    fn handle_shortcut_key(&mut self, key: KeyEvent) -> Result<Action> {
        match key.code {
            KeyCode::Esc => {
                self.dialog = Dialog::Settings;
                self.shortcut_input.clear();
                Ok(Action::None)
            }
            KeyCode::Enter => {
                let shortcut = self.shortcut_input.trim().to_string();
                if shortcut.is_empty() {
                    self.set_error("Ghostty shortcut cannot be empty");
                    return Ok(Action::None);
                }

                Ok(Action::SetGhosttyShortcut(shortcut))
            }
            KeyCode::Backspace => {
                self.shortcut_input.pop();
                Ok(Action::None)
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.shortcut_input.push(c);
                Ok(Action::None)
            }
            _ => Ok(Action::None),
        }
    }

    fn handle_main_key(&mut self, key: KeyEvent, _env: &AppEnv) -> Result<Action> {
        if let KeyCode::Char(c) = key.code {
            if !key.modifiers.contains(KeyModifiers::CONTROL) && self.should_extend_filter(c) {
                self.filter.push(c);
                self.selected = 0;
                self.clear_pending_click();
                self.clamp_selection();
                return Ok(Action::None);
            }
        }

        match key.code {
            KeyCode::Char('q') => Ok(Action::Quit),
            KeyCode::Esc => {
                if !self.filter.is_empty() {
                    self.filter.clear();
                    self.selected = 0;
                    self.clear_pending_click();
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
            KeyCode::Char('g') => {
                self.selected = 0;
                self.clear_pending_click();
                Ok(Action::None)
            }
            KeyCode::Char('G') => {
                let len = self.visible_indices().len();
                if len > 0 {
                    self.selected = len - 1;
                }
                self.clear_pending_click();
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
            KeyCode::Char('d') => {
                if self.selected_workspace().is_some() {
                    self.dialog = Dialog::ConfirmDelete;
                } else {
                    self.set_error("No workspace selected");
                }
                Ok(Action::None)
            }
            KeyCode::Char('t') => {
                self.dialog = Dialog::Settings;
                Ok(Action::None)
            }
            KeyCode::Char('p') => {
                self.show_preview = !self.show_preview;
                Ok(Action::None)
            }
            KeyCode::Backspace => {
                self.filter.pop();
                self.selected = 0;
                self.clear_pending_click();
                self.clamp_selection();
                Ok(Action::None)
            }
            KeyCode::Char('/') => Ok(Action::None),
            _ => Ok(Action::None),
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) -> Result<Action> {
        if !matches!(self.dialog, Dialog::None) {
            return Ok(Action::None);
        }

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
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

                self.set_info(format!(
                    "Selected \"{}\". Double-click or press Enter to launch.",
                    workspace.name
                ));
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

    fn should_extend_filter(&self, c: char) -> bool {
        if c.is_ascii_uppercase() || c.is_ascii_digit() {
            return true;
        }

        if c == '-' || c == '_' || c == '.' {
            return true;
        }

        !self.filter.is_empty() && c.is_ascii_lowercase()
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
}

#[derive(Debug, PartialEq, Eq)]
enum Action {
    None,
    Quit,
    Launch(String),
    Save(String),
    Edit(String),
    Delete(String),
    ToggleCloseTab,
    SetGhosttyShortcut(String),
}

fn draw(frame: &mut Frame<'_>, app: &mut App, env: &AppEnv) {
    if app.is_splash_visible() {
        draw_splash(frame, app);
        return;
    }

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(2),
        ])
        .split(frame.area());

    draw_header(frame, layout[0], app, env);
    draw_body(frame, layout[1], app);
    draw_footer(frame, layout[2], app);

    match app.dialog {
        Dialog::None => {}
        Dialog::Save => draw_save_dialog(frame, app),
        Dialog::ConfirmDelete => draw_delete_dialog(frame, app),
        Dialog::Settings => draw_settings_dialog(frame, env),
        Dialog::EditGhosttyShortcut => draw_shortcut_dialog(frame, app, env),
    }
}

fn draw_splash(frame: &mut Frame<'_>, app: &App) {
    let area = centered_rect(70, 50, frame.area());
    let accent = if app.splash_started_at.elapsed().as_millis() % 2_000 < 1_000 {
        Color::Cyan
    } else {
        Color::Yellow
    };

    let logo = Text::from(vec![
        Line::from("   __ _        _     ").style(Style::default().fg(accent)),
        Line::from("  / _` |___ __| |_ _ ").style(Style::default().fg(accent)),
        Line::from("  \\__, / -_) _|  _| |").style(Style::default().fg(accent)),
        Line::from("  |___/\\___\\__|\\__|_|").style(Style::default().fg(accent)),
        Line::default(),
        Line::from("Ghostty Tab Workspace Manager").style(Style::default().fg(Color::Gray)),
        Line::from("Launching the new TUI...").style(Style::default().fg(Color::DarkGray)),
    ]);

    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(logo)
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL).title("gtab")),
        area,
    );
}

fn draw_header(frame: &mut Frame<'_>, area: Rect, app: &App, env: &AppEnv) {
    let title = Line::from(vec![
        Span::styled(
            "gtab",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled("workspace launcher", Style::default().fg(Color::Gray)),
    ]);

    let subtitle = Line::from(vec![
        Span::styled("dir ", Style::default().fg(Color::DarkGray)),
        Span::raw(env.base_dir.display().to_string()),
        Span::raw("   "),
        Span::styled("filter ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            if app.filter.is_empty() {
                "all".to_string()
            } else {
                app.filter.clone()
            },
            Style::default().fg(Color::Yellow),
        ),
    ]);

    frame.render_widget(
        Paragraph::new(Text::from(vec![title, subtitle])).block(
            Block::default()
                .borders(Borders::ALL)
                .padding(Padding::horizontal(1)),
        ),
        area,
    );
}

fn draw_body(frame: &mut Frame<'_>, area: Rect, app: &mut App) {
    let show_preview = app.show_preview && area.width >= 90;
    let chunks = if show_preview {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(100)])
            .split(area)
    };

    draw_workspace_list(frame, chunks[0], app);

    if show_preview {
        draw_preview(frame, chunks[1], app);
    }
}

fn draw_workspace_list(frame: &mut Frame<'_>, area: Rect, app: &mut App) {
    app.list_area = Block::default().borders(Borders::ALL).inner(area);
    let visible = app.visible_workspaces();
    let items: Vec<ListItem<'_>> = if visible.is_empty() {
        vec![ListItem::new(Line::from("No workspaces found"))]
    } else {
        visible
            .iter()
            .map(|workspace| ListItem::new(Line::from(format!("  {}", workspace.name))))
            .collect()
    };

    let title = format!("Workspaces ({})", visible.len());
    let block = Block::default().borders(Borders::ALL).title(title);

    let mut state = ListState::default()
        .with_selected((!visible.is_empty()).then_some(app.selected))
        .with_offset(app.list_offset);

    let list = List::new(items)
        .block(block)
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("›");

    frame.render_stateful_widget(list, area, &mut state);
    app.list_offset = state.offset();
}

fn draw_preview(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let text = match app.selected_workspace() {
        Some(workspace) if workspace.tabs.is_empty() => Text::from(vec![
            Line::from(format!("Workspace: {}", workspace.name)),
            Line::default(),
            Line::from("No tab titles were found in this workspace yet."),
        ]),
        Some(workspace) => {
            let mut lines = vec![
                Line::from(vec![
                    Span::styled("Workspace: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(&workspace.name, Style::default().fg(Color::Cyan)),
                ]),
                Line::from(vec![
                    Span::styled("Tabs: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        workspace.tabs.len().to_string(),
                        Style::default().fg(Color::Yellow),
                    ),
                ]),
                Line::default(),
            ];

            lines.extend(
                workspace
                    .tabs
                    .iter()
                    .enumerate()
                    .map(|(index, tab)| Line::from(format!("{}. {}", index + 1, tab))),
            );

            Text::from(lines)
        }
        None => Text::from("No workspace selected.\n\nUse a to save one or clear the filter."),
    };

    frame.render_widget(
        Paragraph::new(text)
            .block(Block::default().borders(Borders::ALL).title("Preview"))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_footer(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let status = app
        .status
        .as_ref()
        .map(|status| {
            let color = match status.kind {
                StatusKind::Info => Color::Gray,
                StatusKind::Success => Color::Green,
                StatusKind::Error => Color::Red,
            };

            Line::from(vec![Span::styled(
                status.text.clone(),
                Style::default().fg(color),
            )])
        })
        .unwrap_or_else(|| Line::from("Ready"));

    let keys = Line::from(vec![
        Span::styled("click", Style::default().fg(Color::Cyan)),
        Span::raw(" select  "),
        Span::styled("dbl-click", Style::default().fg(Color::Cyan)),
        Span::raw(" launch  "),
        Span::styled("Enter", Style::default().fg(Color::Cyan)),
        Span::raw(" launch  "),
        Span::styled("w/s", Style::default().fg(Color::Cyan)),
        Span::raw(" move  "),
        Span::styled("a", Style::default().fg(Color::Cyan)),
        Span::raw(" save  "),
        Span::styled("e", Style::default().fg(Color::Cyan)),
        Span::raw(" edit  "),
        Span::styled("d", Style::default().fg(Color::Cyan)),
        Span::raw(" delete  "),
        Span::styled("t", Style::default().fg(Color::Cyan)),
        Span::raw(" settings  "),
        Span::styled("p", Style::default().fg(Color::Cyan)),
        Span::raw(" preview  "),
        Span::styled("q", Style::default().fg(Color::Cyan)),
        Span::raw(" quit"),
    ]);

    let block = Block::default().borders(Borders::TOP);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let footer_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(inner);

    frame.render_widget(Paragraph::new(status), footer_layout[0]);
    frame.render_widget(Paragraph::new(keys), footer_layout[1]);
}

fn draw_save_dialog(frame: &mut Frame<'_>, app: &App) {
    let area = centered_rect(55, 25, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(Text::from(vec![
            Line::from("Save the current Ghostty window as a workspace."),
            Line::default(),
            Line::from(vec![
                Span::styled("Name: ", Style::default().fg(Color::Cyan)),
                Span::raw(if app.save_input.is_empty() {
                    " "
                } else {
                    &app.save_input
                }),
            ]),
            Line::default(),
            Line::from("Enter to save, Esc to cancel"),
        ]))
        .block(
            Block::default()
                .title("Save Workspace")
                .borders(Borders::ALL),
        ),
        area,
    );
}

fn draw_delete_dialog(frame: &mut Frame<'_>, app: &App) {
    let workspace_name = app
        .selected_workspace()
        .map(|workspace| workspace.name.as_str())
        .unwrap_or("this workspace");

    let area = centered_rect(50, 22, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(Text::from(vec![
            Line::from(format!("Delete \"{workspace_name}\"?")),
            Line::default(),
            Line::from("y / Enter to confirm"),
            Line::from("n / Esc to cancel"),
        ]))
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .title("Confirm Delete")
                .borders(Borders::ALL),
        ),
        area,
    );
}

fn draw_settings_dialog(frame: &mut Frame<'_>, env: &AppEnv) {
    let area = centered_rect(62, 32, frame.area());
    let close_tab = if env.config.close_tab { "on" } else { "off" };
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(Text::from(vec![
            Line::from("Current settings"),
            Line::default(),
            Line::from(vec![
                Span::styled("close_tab", Style::default().fg(Color::Cyan)),
                Span::raw(" = "),
                Span::styled(close_tab, Style::default().fg(Color::Yellow)),
            ]),
            Line::from("Close the current tab after launching a workspace."),
            Line::default(),
            Line::from(vec![
                Span::styled("ghostty_shortcut", Style::default().fg(Color::Cyan)),
                Span::raw(" = "),
                Span::styled(
                    env.ghostty_shortcut_display(),
                    Style::default().fg(Color::Yellow),
                ),
            ]),
            Line::from("Runs `gtab` in the focused Ghostty shell."),
            Line::from("The managed keybind is stored in ~/.config/gtab/ghostty-shortcut.conf."),
            Line::default(),
            Line::from("Press c to toggle close_tab"),
            Line::from("Press g to edit the Ghostty shortcut"),
            Line::from("Reload Ghostty config or restart Ghostty after changing it"),
            Line::from("Enter or Esc to close"),
        ]))
        .block(Block::default().title("Settings").borders(Borders::ALL)),
        area,
    );
}

fn draw_shortcut_dialog(frame: &mut Frame<'_>, app: &App, env: &AppEnv) {
    let area = centered_rect(62, 28, frame.area());
    let current_input = if app.shortcut_input.is_empty() {
        env.ghostty_shortcut_display()
    } else {
        app.shortcut_input.as_str()
    };

    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(Text::from(vec![
            Line::from("Set the Ghostty shortcut used to run `gtab`."),
            Line::default(),
            Line::from(vec![
                Span::styled("Shortcut: ", Style::default().fg(Color::Cyan)),
                Span::raw(current_input),
            ]),
            Line::default(),
            Line::from("Examples: cmd+g, cmd+shift+g, ctrl+alt+g"),
            Line::from("This sends `gtab` plus Enter to the focused Ghostty shell."),
            Line::default(),
            Line::from("Enter to save, Esc to cancel"),
        ]))
        .block(
            Block::default()
                .title("Ghostty Shortcut")
                .borders(Borders::ALL),
        ),
        area,
    );
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

fn shortcut_sync_message(sync: &GhosttyShortcutSync) -> String {
    format!(
        "Ghostty shortcut {} saved to {}. Reload Ghostty config or restart Ghostty.",
        sync.shortcut,
        sync.include_path.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

    fn workspace(name: &str) -> Workspace {
        Workspace {
            name: name.to_string(),
            tabs: vec!["tab".to_string()],
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

    #[test]
    fn single_click_selects_and_double_click_launches() {
        let mut app = App::new(vec![workspace("alpha"), workspace("beta")]);
        app.dismiss_splash();
        app.list_area = Rect::new(0, 0, 40, 6);

        assert_eq!(app.handle_mouse(left_click(1, 1)).unwrap(), Action::None);
        assert_eq!(app.selected, 1);

        assert_eq!(
            app.handle_mouse(left_click(1, 1)).unwrap(),
            Action::Launch("beta".to_string())
        );
    }
}
