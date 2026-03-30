use anyhow::{Context, Result, anyhow, bail};
use std::{
    env, fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const APPLE_EXT: &str = "applescript";
const DEFAULT_GHOSTTY_SHORTCUT: &str = "cmd+g";
const GHOSTTY_SHORTCUT_INCLUDE_NAME: &str = "ghostty-shortcut.conf";
const LAUNCHER_SCRIPT_NAME: &str = "launcher.sh";

#[derive(Clone, Debug)]
pub struct Config {
    pub close_tab: bool,
    pub ghostty_shortcut: String,
}

#[derive(Clone, Debug)]
pub struct Workspace {
    pub name: String,
    pub tabs: Vec<String>,
}

#[derive(Debug)]
pub struct AppEnv {
    pub base_dir: PathBuf,
    pub config_file: PathBuf,
    pub config: Config,
}

#[derive(Clone, Debug)]
struct TabRow {
    working_dir: String,
    title: String,
}

impl AppEnv {
    pub fn load() -> Result<Self> {
        let base_dir = resolve_base_dir()?;
        fs::create_dir_all(&base_dir)
            .with_context(|| format!("failed to create {}", base_dir.display()))?;

        let config_file = base_dir.join("config");
        let config = Config::load(&config_file)?;

        Ok(Self {
            base_dir,
            config_file,
            config,
        })
    }

    pub fn reload_config(&mut self) -> Result<()> {
        self.config = Config::load(&self.config_file)?;
        Ok(())
    }

    pub fn set_close_tab(&mut self, enabled: bool) -> Result<()> {
        self.config.close_tab = enabled;
        self.write_config()
    }

    pub fn set_ghostty_shortcut(&mut self, shortcut: &str) -> Result<GhosttyShortcutSync> {
        self.config.ghostty_shortcut = normalize_ghostty_shortcut(shortcut)?;
        self.write_config()?;
        self.sync_ghostty_shortcut()
    }

    pub fn launcher_path(&self) -> PathBuf {
        self.base_dir.join(LAUNCHER_SCRIPT_NAME)
    }

    pub fn ensure_launcher_script(&self) -> Result<PathBuf> {
        let path = self.launcher_path();
        self.write_launcher_script(&path)?;
        Ok(path)
    }

    pub fn ensure_ghostty_shortcut(&self) -> Result<bool> {
        let sync = self.preview_ghostty_shortcut_sync();
        let include_changed = self.write_ghostty_shortcut_include(&sync.include_path)?;
        let config_changed =
            ensure_ghostty_include_reference(&sync.config_path, &sync.include_path)?;
        Ok(include_changed || config_changed)
    }

    pub fn list_workspaces(&self) -> Result<Vec<Workspace>> {
        let mut workspaces = Vec::new();

        for entry in fs::read_dir(&self.base_dir)
            .with_context(|| format!("failed to read {}", self.base_dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some(APPLE_EXT) {
                continue;
            }

            let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };

            let tabs = fs::read_to_string(&path)
                .map(|content| parse_workspace_tabs(&content))
                .unwrap_or_default();

            workspaces.push(Workspace {
                name: stem.to_string(),
                tabs,
            });
        }

        workspaces.sort_by_key(|workspace| workspace.name.to_lowercase());
        Ok(workspaces)
    }

    pub fn workspace_path(&self, name: &str) -> Result<PathBuf> {
        validate_workspace_name(name)?;
        Ok(self.base_dir.join(format!("{name}.{APPLE_EXT}")))
    }

    pub fn save_current_window(&self, name: &str) -> Result<PathBuf> {
        let path = self.workspace_path(name)?;
        let rows = capture_ghostty_tabs()?;
        if rows.is_empty() {
            bail!("could not read Ghostty tabs (make sure Ghostty is the frontmost app)");
        }

        let script = build_workspace_script(&rows);
        fs::write(&path, script).with_context(|| format!("failed to write {}", path.display()))?;
        Ok(path)
    }

    pub fn open_in_editor(&self, name: &str) -> Result<()> {
        let path = self.workspace_path(name)?;
        let editor = env::var("EDITOR")
            .ok()
            .filter(|editor| !editor.trim().is_empty())
            .unwrap_or_else(|| "vim".to_string());

        let status = Command::new(&editor)
            .arg(&path)
            .status()
            .with_context(|| format!("failed to launch editor {editor}"))?;

        if !status.success() {
            bail!("editor exited with status {status}");
        }

        Ok(())
    }

    pub fn remove_workspace(&self, name: &str) -> Result<PathBuf> {
        let path = self.workspace_path(name)?;
        if !path.exists() {
            bail!("workspace '{name}' not found");
        }

        fs::remove_file(&path).with_context(|| format!("failed to remove {}", path.display()))?;
        Ok(path)
    }

    pub fn launch_workspace(&self, name: &str) -> Result<()> {
        let path = self.workspace_path(name)?;
        if !path.exists() {
            bail!("workspace '{name}' not found");
        }

        let status = Command::new("osascript")
            .arg(&path)
            .status()
            .with_context(|| format!("failed to run {}", path.display()))?;

        if !status.success() {
            bail!("workspace launch failed");
        }

        if self.config.close_tab {
            hup_parent_process()?;
        }

        Ok(())
    }

    pub fn close_tab_display(&self) -> &'static str {
        if self.config.close_tab { "on" } else { "off" }
    }

    pub fn ghostty_shortcut_display(&self) -> &str {
        &self.config.ghostty_shortcut
    }

    pub fn preview_ghostty_shortcut_sync(&self) -> GhosttyShortcutSync {
        GhosttyShortcutSync {
            config_path: resolve_ghostty_config_path().unwrap_or_else(|_| {
                home_dir()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join(".config/ghostty/config.ghostty")
            }),
            include_path: self.base_dir.join(GHOSTTY_SHORTCUT_INCLUDE_NAME),
            shortcut: self.config.ghostty_shortcut.clone(),
        }
    }

    fn sync_ghostty_shortcut(&self) -> Result<GhosttyShortcutSync> {
        let sync = self.preview_ghostty_shortcut_sync();
        self.write_ghostty_shortcut_include(&sync.include_path)?;
        ensure_ghostty_include_reference(&sync.config_path, &sync.include_path)?;
        Ok(sync)
    }

    fn write_ghostty_shortcut_include(&self, path: &Path) -> Result<bool> {
        let content = build_ghostty_shortcut_include(&self.config.ghostty_shortcut);
        if fs::read_to_string(path).ok().as_deref() == Some(content.as_str()) {
            return Ok(false);
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        fs::write(path, content).with_context(|| format!("failed to write {}", path.display()))?;
        Ok(true)
    }

    fn write_launcher_script(&self, path: &Path) -> Result<bool> {
        let content = build_launcher_script();
        let mut changed = false;

        if fs::read_to_string(path).ok().as_deref() != Some(content.as_str()) {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }

            fs::write(path, content)
                .with_context(|| format!("failed to write {}", path.display()))?;
            changed = true;
        }

        let metadata =
            fs::metadata(path).with_context(|| format!("failed to read {}", path.display()))?;
        let mut permissions = metadata.permissions();
        if permissions.mode() & 0o777 != 0o755 {
            permissions.set_mode(0o755);
            fs::set_permissions(path, permissions)
                .with_context(|| format!("failed to chmod {}", path.display()))?;
            changed = true;
        }

        Ok(changed)
    }

    fn write_config(&mut self) -> Result<()> {
        if let Some(parent) = self.config_file.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        fs::write(&self.config_file, self.config.serialize())
            .with_context(|| format!("failed to write {}", self.config_file.display()))?;
        self.reload_config()
    }
}

impl Config {
    fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }

        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let mut config = Self::default();

        for line in raw.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };

            if key.trim() == "close_tab" {
                config.close_tab = matches!(value.trim(), "true" | "on");
            } else if key.trim() == "ghostty_shortcut" && !value.trim().is_empty() {
                config.ghostty_shortcut = normalize_ghostty_shortcut(value.trim())?;
            }
        }

        Ok(config)
    }

    fn serialize(&self) -> String {
        let close_tab = if self.close_tab { "true" } else { "false" };
        format!(
            "close_tab={close_tab}\nghostty_shortcut={}\n",
            self.ghostty_shortcut
        )
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            close_tab: false,
            ghostty_shortcut: DEFAULT_GHOSTTY_SHORTCUT.to_string(),
        }
    }
}

fn resolve_base_dir() -> Result<PathBuf> {
    if let Some(dir) = env::var_os("GTAB_DIR") {
        return Ok(PathBuf::from(dir));
    }

    let home = home_dir().ok_or_else(|| anyhow!("failed to resolve home directory"))?;
    Ok(home.join(".config").join("gtab"))
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME").map(PathBuf::from)
}

fn resolve_ghostty_config_path() -> Result<PathBuf> {
    let home = home_dir().ok_or_else(|| anyhow!("failed to resolve home directory"))?;
    let xdg_dir = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    let ghostty_dir = xdg_dir.join("ghostty");
    let config_ghostty = ghostty_dir.join("config.ghostty");
    let legacy_config = ghostty_dir.join("config");

    if config_ghostty.exists() {
        return Ok(config_ghostty);
    }

    if legacy_config.exists() {
        return Ok(legacy_config);
    }

    Ok(config_ghostty)
}

fn validate_workspace_name(name: &str) -> Result<()> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        bail!("missing workspace name");
    }

    if trimmed == "." || trimmed == ".." || trimmed.contains('/') || trimmed.contains('\0') {
        bail!("invalid workspace name '{name}'");
    }

    Ok(())
}

fn normalize_ghostty_shortcut(shortcut: &str) -> Result<String> {
    let normalized = shortcut.trim().to_lowercase();
    if normalized.is_empty() {
        bail!("ghostty_shortcut cannot be empty");
    }

    if normalized.contains('=') || normalized.contains('\n') || normalized.contains('\r') {
        bail!("ghostty_shortcut contains invalid characters");
    }

    Ok(normalized)
}

fn capture_ghostty_tabs() -> Result<Vec<TabRow>> {
    let output = run_osascript(
        r#"set rows to {}
tell application "Ghostty"
  set win to front window
  set n to count of tabs of win
  repeat with i from 1 to n
    set t to tab i of win
    set surf to focused terminal of t
    try
      set wd to working directory of surf
    on error
      set wd to ""
    end try
    set ttl to name of t
    if ttl starts with "⠐ " then set ttl to text 3 thru -1 of ttl
    set end of rows to (wd & "	" & ttl)
  end repeat
end tell
set AppleScript's text item delimiters to linefeed
return rows as text"#,
    )
    .context("could not read Ghostty tabs (make sure Ghostty is the frontmost app)")?;

    let home = home_dir()
        .unwrap_or_else(|| PathBuf::from("/"))
        .into_os_string()
        .into_string()
        .unwrap_or_else(|_| "/".to_string());

    let rows = output
        .lines()
        .map(|line| {
            let mut parts = line.splitn(2, '\t');
            let working_dir = parts.next().unwrap_or("").trim().to_string();
            let title = parts.next().unwrap_or("").trim().to_string();
            TabRow {
                working_dir: if working_dir.is_empty() {
                    home.clone()
                } else {
                    working_dir
                },
                title,
            }
        })
        .collect();

    Ok(rows)
}

fn build_workspace_script(rows: &[TabRow]) -> String {
    let mut out = String::from("tell application \"Ghostty\"\n    activate");

    for (index, row) in rows.iter().enumerate() {
        let n = index + 1;
        out.push_str("\n\n");
        out.push_str(&format!("    set cfg{n} to new surface configuration\n"));
        out.push_str(&format!(
            "    set initial working directory of cfg{n} to \"{}\"\n",
            apple_escape(&row.working_dir)
        ));

        if index == 0 {
            out.push_str(&format!(
                "    set win to new window with configuration cfg{n}\n"
            ));
            out.push_str(&format!(
                "    set term{n} to focused terminal of selected tab of win"
            ));
        } else {
            out.push_str(&format!(
                "    set tab{n} to new tab in win with configuration cfg{n}\n"
            ));
            out.push_str(&format!("    set term{n} to focused terminal of tab{n}"));
        }

        if !row.title.is_empty() {
            out.push_str(&format!(
                "\n    perform action \"set_tab_title:{}\" on term{n}",
                apple_escape(&row.title)
            ));
        }
    }

    out.push_str("\nend tell\n");
    out
}

fn parse_workspace_tabs(script: &str) -> Vec<String> {
    let mut tabs: Vec<ParsedWorkspaceTab> = Vec::new();

    for line in script.lines() {
        let trimmed = line.trim();

        if let Some((index, working_dir)) =
            parse_indexed_assignment(trimmed, "set initial working directory of cfg", " to ")
        {
            ensure_tab_slot(&mut tabs, index);
            tabs[index - 1].working_dir = Some(working_dir);
            continue;
        }

        if let Some((index, title)) = parse_title_assignment(trimmed) {
            ensure_tab_slot(&mut tabs, index);
            tabs[index - 1].title = Some(title);
        }
    }

    tabs.into_iter()
        .enumerate()
        .map(
            |(index, tab)| match tab.title.filter(|title| !title.is_empty()) {
                Some(title) => title,
                None => fallback_tab_name(index + 1, tab.working_dir.as_deref()),
            },
        )
        .collect()
}

fn parse_indexed_assignment(line: &str, prefix: &str, marker: &str) -> Option<(usize, String)> {
    let rest = line.strip_prefix(prefix)?;
    let (index, rest) = split_digits(rest)?;
    let value = rest.strip_prefix(marker)?;
    parse_apple_string(value).map(|parsed| (index, parsed))
}

fn parse_title_assignment(line: &str) -> Option<(usize, String)> {
    let rest = line.strip_prefix("perform action \"set_tab_title:")?;
    let quoted = format!("\"{rest}");
    let (title, after_title) = parse_apple_string_prefix(&quoted)?;
    let after_marker = after_title.strip_prefix(" on term")?;
    let (index, _) = split_digits(after_marker)?;
    Some((index, title))
}

fn split_digits(value: &str) -> Option<(usize, &str)> {
    let digits_len = value.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits_len == 0 {
        return None;
    }

    let (digits, rest) = value.split_at(digits_len);
    let index = digits.parse().ok()?;
    Some((index, rest))
}

fn parse_apple_string(value: &str) -> Option<String> {
    parse_apple_string_prefix(value.trim_start()).map(|(parsed, _)| parsed)
}

fn parse_apple_string_prefix(value: &str) -> Option<(String, &str)> {
    let quoted = value.strip_prefix('"')?;
    let mut escaped = false;
    let mut out = String::new();

    for (index, ch) in quoted.char_indices() {
        if escaped {
            out.push(ch);
            escaped = false;
            continue;
        }

        match ch {
            '\\' => escaped = true,
            '"' => return Some((out, &quoted[index + ch.len_utf8()..])),
            _ => out.push(ch),
        }
    }

    None
}

fn fallback_tab_name(index: usize, working_dir: Option<&str>) -> String {
    let Some(working_dir) = working_dir.map(str::trim).filter(|value| !value.is_empty()) else {
        return format!("Tab {index}");
    };

    if working_dir
        == home_dir()
            .unwrap_or_else(|| PathBuf::from("/"))
            .to_string_lossy()
    {
        return "~".to_string();
    }

    Path::new(working_dir)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| working_dir.to_string())
}

fn apple_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn build_ghostty_shortcut_include(shortcut: &str) -> String {
    format!(
        "# Managed by gtab. Update this in gtab settings.\n# This sends `gtab` to the focused Ghostty shell.\nkeybind = {shortcut}=text:gtab\\x0d\n"
    )
}

fn build_launcher_script() -> String {
    r#"#!/bin/sh
set -eu

GTAB_BIN="$(command -v gtab 2>/dev/null || true)"

if [ -z "$GTAB_BIN" ] && [ -x "/opt/homebrew/bin/gtab" ]; then
  GTAB_BIN="/opt/homebrew/bin/gtab"
fi

if [ -z "$GTAB_BIN" ] || [ ! -x "$GTAB_BIN" ]; then
  echo "gtab binary not found. Install gtab or add it to PATH." >&2
  exit 1
fi

exec open -na Ghostty.app --args -e "$GTAB_BIN"
"#
    .to_string()
}

fn ensure_ghostty_include_reference(config_path: &Path, include_path: &Path) -> Result<bool> {
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let existing = fs::read_to_string(config_path).unwrap_or_default();
    let include_line = format!("config-file = \"{}\"", include_path.display());

    if existing.lines().any(|line| {
        let Some((key, value)) = line.split_once('=') else {
            return false;
        };

        key.trim() == "config-file"
            && value.trim().trim_matches('"') == include_path.display().to_string()
    }) {
        return Ok(false);
    }

    let mut next = existing;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }

    if !next.is_empty() {
        next.push('\n');
    }

    next.push_str("# gtab managed include\n");
    next.push_str(&include_line);
    next.push('\n');

    fs::write(config_path, next)
        .with_context(|| format!("failed to write {}", config_path.display()))?;
    Ok(true)
}

#[derive(Clone, Debug, Default)]
struct ParsedWorkspaceTab {
    working_dir: Option<String>,
    title: Option<String>,
}

fn ensure_tab_slot(tabs: &mut Vec<ParsedWorkspaceTab>, index: usize) {
    while tabs.len() < index {
        tabs.push(ParsedWorkspaceTab::default());
    }
}

#[derive(Clone, Debug)]
pub struct GhosttyShortcutSync {
    pub config_path: PathBuf,
    pub include_path: PathBuf,
    pub shortcut: String,
}

fn run_osascript(script: &str) -> Result<String> {
    let mut child = Command::new("osascript")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to launch osascript")?;

    {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| anyhow!("failed to open osascript stdin"))?;
        use std::io::Write as _;
        stdin
            .write_all(script.as_bytes())
            .context("failed to write AppleScript")?;
    }

    let output = child
        .wait_with_output()
        .context("failed to wait for osascript")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("{}", stderr.trim());
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn hup_parent_process() -> Result<()> {
    let pid = std::process::id().to_string();
    let output = Command::new("ps")
        .args(["-o", "ppid=", "-p", &pid])
        .output()
        .context("failed to resolve parent process")?;

    if !output.status.success() {
        bail!("failed to resolve parent process");
    }

    let ppid = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if ppid.is_empty() {
        bail!("failed to resolve parent process");
    }

    let status = Command::new("kill")
        .args(["-HUP", &ppid])
        .status()
        .context("failed to signal parent process")?;

    if !status.success() {
        bail!("failed to signal parent process");
    }

    Ok(())
}

pub fn format_workspace_list(workspaces: &[Workspace]) -> String {
    if workspaces.is_empty() {
        return "No workspaces saved.".to_string();
    }

    let mut lines = vec!["Workspaces:".to_string()];
    for workspace in workspaces {
        lines.push(format!("  - {}", workspace.name));
    }
    lines.join("\n")
}

pub fn format_settings(env: &AppEnv) -> String {
    let close_tab = if env.config.close_tab { "on" } else { "off" };
    format!(
        "Settings:\n  close_tab = {close_tab}\n  launcher = {}\n  ghostty_shortcut = {}\n  Recommended: bind `gtab shortcut` in Shortcuts, Raycast, or Hammerspoon.\n  Legacy Ghostty shortcut sends `gtab` to the focused shell and can fail in Claude Code/Codex.",
        env.launcher_path().display(),
        env.config.ghostty_shortcut
    )
}

pub fn format_shortcut_guide(env: &AppEnv, launcher_path: &Path) -> String {
    format!(
        "Shortcut launcher:\n  {}\n\nBind Cmd+G in macOS Shortcuts, Raycast, or Hammerspoon to run this script.\nIt opens a new Ghostty window and runs gtab without typing into the current shell.\n\nLegacy Ghostty keybind:\n  {}\n  This sends `gtab` to the focused shell and can fail in Claude Code, Codex, vim, or fzf.",
        launcher_path.display(),
        env.ghostty_shortcut_display()
    )
}

#[cfg(test)]
mod tests {
    use super::{
        Config, TabRow, apple_escape, build_ghostty_shortcut_include, build_launcher_script,
        build_workspace_script, parse_workspace_tabs,
    };

    #[test]
    fn config_parses_close_tab_truthy_values() {
        let path = tempfile_path("config");
        std::fs::write(&path, "close_tab=true\nghostty_shortcut=cmd+shift+g\n").unwrap();
        let config = Config::load(&path).unwrap();
        assert!(config.close_tab);
        assert_eq!(config.ghostty_shortcut, "cmd+shift+g");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn apple_script_generation_preserves_workspace_structure() {
        let script = build_workspace_script(&[
            TabRow {
                working_dir: "/tmp/demo".to_string(),
                title: "main".to_string(),
            },
            TabRow {
                working_dir: "/tmp/api".to_string(),
                title: String::new(),
            },
        ]);

        assert!(script.contains("set win to new window"));
        assert!(script.contains("new tab in win"));
        assert!(script.contains("set_tab_title:main"));
    }

    #[test]
    fn apple_escape_handles_quotes_and_backslashes() {
        assert_eq!(
            apple_escape(r#"/tmp/"quote"\path"#),
            r#"/tmp/\"quote\"\\path"#
        );
    }

    #[test]
    fn workspace_preview_uses_titles_and_fallbacks() {
        let tabs = parse_workspace_tabs(
            r#"tell application "Ghostty"
    activate

    set cfg1 to new surface configuration
    set initial working directory of cfg1 to "/tmp/project"
    set win to new window with configuration cfg1
    set term1 to focused terminal of selected tab of win
    perform action "set_tab_title:api" on term1

    set cfg2 to new surface configuration
    set initial working directory of cfg2 to "/tmp/work"
    set tab2 to new tab in win with configuration cfg2
    set term2 to focused terminal of tab2
end tell
"#,
        );

        assert_eq!(tabs, vec!["api".to_string(), "work".to_string()]);
    }

    #[test]
    fn ghostty_shortcut_include_writes_keybind_command() {
        let include = build_ghostty_shortcut_include("cmd+g");
        assert!(include.contains("keybind = cmd+g=text:gtab\\x0d"));
    }

    #[test]
    fn launcher_script_prefers_path_and_homebrew_fallback() {
        let script = build_launcher_script();
        assert!(script.contains("command -v gtab"));
        assert!(script.contains("/opt/homebrew/bin/gtab"));
        assert!(script.contains("open -na Ghostty.app --args -e \"$GTAB_BIN\""));
    }

    fn tempfile_path(name: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("gtab-{name}-{nanos}.tmp"))
    }
}
