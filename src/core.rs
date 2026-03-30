use anyhow::{Context, Result, anyhow, bail};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const APPLE_EXT: &str = "applescript";

#[derive(Clone, Debug, Default)]
pub struct Config {
    pub close_tab: bool,
}

#[derive(Clone, Debug)]
pub struct Workspace {
    pub name: String,
    pub path: PathBuf,
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
        let value = if enabled { "true" } else { "false" };
        fs::write(&self.config_file, format!("close_tab={value}\n"))
            .with_context(|| format!("failed to write {}", self.config_file.display()))?;
        self.reload_config()
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

            workspaces.push(Workspace {
                name: stem.to_string(),
                path,
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
            }
        }

        Ok(config)
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

fn apple_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
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

pub fn format_settings(config: &Config) -> String {
    let close_tab = if config.close_tab { "on" } else { "off" };
    format!(
        "Settings:\n  close_tab = {close_tab}\n  (close current tab after launching a workspace)"
    )
}

#[cfg(test)]
mod tests {
    use super::{Config, TabRow, apple_escape, build_workspace_script};

    #[test]
    fn config_parses_close_tab_truthy_values() {
        let path = tempfile_path("config");
        std::fs::write(&path, "close_tab=true\n").unwrap();
        let config = Config::load(&path).unwrap();
        assert!(config.close_tab);
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

    fn tempfile_path(name: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("gtab-{name}-{nanos}.tmp"))
    }
}
