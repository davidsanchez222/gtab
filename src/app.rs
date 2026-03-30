use crate::core::{AppEnv, Workspace};
use anyhow::{Context, Result};
use crossterm::{
    cursor::{Hide, Show},
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
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

pub fn run_tui(env: &mut AppEnv) -> Result<()> {
    let mut terminal = TerminalSession::start()?;
    let mut app = App::new(env.list_workspaces()?);

    loop {
        terminal.draw(|frame| draw(frame, &app, env))?;

        if let Some(expiry) = app.status_expiry {
            if Instant::now() >= expiry {
                app.clear_status();
            }
        }

        if !event::poll(Duration::from_millis(60)).context("failed to poll terminal events")? {
            continue;
        }

        let Event::Key(key) = event::read().context("failed to read terminal event")? else {
            continue;
        };

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
                Ok(()) => app.set_success(format!("close_tab = {}", env.close_tab_display())),
                Err(error) => app.set_error(error.to_string()),
            },
        }
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
        execute!(stdout, EnterAlternateScreen, Hide).context("failed to enter alternate screen")?;
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
        execute!(self.terminal.backend_mut(), LeaveAlternateScreen, Show)
            .context("failed to leave alternate screen")?;
        self.terminal.show_cursor().ok();
        Ok(())
    }

    fn resume(&mut self) -> Result<()> {
        execute!(self.terminal.backend_mut(), EnterAlternateScreen, Hide)
            .context("failed to re-enter alternate screen")?;
        enable_raw_mode().context("failed to re-enable raw mode")?;
        self.terminal.clear().ok();
        Ok(())
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen, Show);
        let _ = self.terminal.show_cursor();
    }
}

#[derive(Clone, Debug)]
enum Dialog {
    None,
    Save,
    ConfirmDelete,
    Settings,
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
struct App {
    workspaces: Vec<Workspace>,
    selected: usize,
    filter: String,
    show_preview: bool,
    dialog: Dialog,
    save_input: String,
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
            filter: String::new(),
            show_preview: true,
            dialog: Dialog::None,
            save_input: String::new(),
            splash_started_at: Instant::now(),
            splash_visible: true,
            status: Some(StatusLine {
                kind: StatusKind::Info,
                text: "Type to filter, Enter to launch, s to save, t for settings.".to_string(),
            }),
            status_expiry: None,
        }
    }

    fn reload(&mut self, workspaces: Vec<Workspace>) {
        self.workspaces = workspaces;
        self.clamp_selection();
    }

    fn reset_dialogs(&mut self) {
        self.dialog = Dialog::None;
        self.save_input.clear();
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
            return;
        };

        self.selected = position;
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
            return;
        }

        let max = len.saturating_sub(1) as isize;
        let next = (self.selected as isize + delta).clamp(0, max);
        self.selected = next as usize;
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
            Dialog::Settings => self.handle_settings_key(key),
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

    fn handle_settings_key(&mut self, key: KeyEvent) -> Result<Action> {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => {
                self.reset_dialogs();
                Ok(Action::None)
            }
            KeyCode::Char('c') | KeyCode::Char(' ') => Ok(Action::ToggleCloseTab),
            _ => Ok(Action::None),
        }
    }

    fn handle_main_key(&mut self, key: KeyEvent, _env: &AppEnv) -> Result<Action> {
        if let KeyCode::Char(c) = key.code {
            if !key.modifiers.contains(KeyModifiers::CONTROL) && self.should_extend_filter(c) {
                self.filter.push(c);
                self.selected = 0;
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
                    return Ok(Action::None);
                }

                Ok(Action::Quit)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_selection(1);
                Ok(Action::None)
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_selection(-1);
                Ok(Action::None)
            }
            KeyCode::Char('g') => {
                self.selected = 0;
                Ok(Action::None)
            }
            KeyCode::Char('G') => {
                let len = self.visible_indices().len();
                if len > 0 {
                    self.selected = len - 1;
                }
                Ok(Action::None)
            }
            KeyCode::Enter => {
                let Some(workspace) = self.selected_workspace() else {
                    self.set_error("No workspace selected");
                    return Ok(Action::None);
                };
                Ok(Action::Launch(workspace.name.clone()))
            }
            KeyCode::Char('s') => {
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
                self.clamp_selection();
                Ok(Action::None)
            }
            KeyCode::Char('/') => Ok(Action::None),
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
}

enum Action {
    None,
    Quit,
    Launch(String),
    Save(String),
    Edit(String),
    Delete(String),
    ToggleCloseTab,
}

fn draw(frame: &mut Frame<'_>, app: &App, env: &AppEnv) {
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

fn draw_body(frame: &mut Frame<'_>, area: Rect, app: &App) {
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

fn draw_workspace_list(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let visible = app.visible_workspaces();
    let items: Vec<ListItem<'_>> = if visible.is_empty() {
        vec![ListItem::new(Line::from("No workspaces found"))]
    } else {
        visible
            .iter()
            .map(|workspace| ListItem::new(Line::from(format!("  {}", workspace.name))))
            .collect()
    };

    let mut state =
        ListState::default().with_selected((!visible.is_empty()).then_some(app.selected));
    let title = format!("Workspaces ({})", visible.len());

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("›");

    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_preview(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let text = match app.selected_workspace() {
        Some(workspace) => match std::fs::read_to_string(&workspace.path) {
            Ok(content) => Text::from(content),
            Err(error) => Text::from(error.to_string()),
        },
        None => Text::from(
            "No workspace selected.\n\nUse s to save the current Ghostty window or clear the filter.",
        ),
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
        Span::styled("Enter", Style::default().fg(Color::Cyan)),
        Span::raw(" launch  "),
        Span::styled("s", Style::default().fg(Color::Cyan)),
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
    let area = centered_rect(55, 24, frame.area());
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
            Line::from("Press c or Space to toggle"),
            Line::from("Enter or Esc to close"),
        ]))
        .block(Block::default().title("Settings").borders(Borders::ALL)),
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
