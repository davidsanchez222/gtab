use anyhow::{Context, Result, anyhow, bail};
#[cfg(target_os = "macos")]
use std::ffi::{CStr, c_void};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const APPLE_EXT: &str = "applescript";
const DEFAULT_GHOSTTY_SHORTCUT: &str = "cmd+g";
const GHOSTTY_SHORTCUT_INCLUDE_NAME: &str = "ghostty-shortcut.conf";
const LEGACY_LAUNCHER_SCRIPT_NAME: &str = "launcher.sh";
const LEGACY_HOTKEY_SERVICE_LABEL: &str = "com.franvy.gtab.hotkey";
const LEGACY_HOTKEY_PLIST_NAME: &str = "com.franvy.gtab.hotkey.plist";
const LEGACY_HOTKEY_LOG_NAME: &str = "gtab-hotkey.log";
#[cfg(target_os = "macos")]
const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;

#[cfg(target_os = "macos")]
type Boolean = u8;
#[cfg(target_os = "macos")]
type CFIndex = isize;
#[cfg(target_os = "macos")]
type CFStringEncoding = u32;
#[cfg(target_os = "macos")]
type CFTypeRef = *const c_void;
#[cfg(target_os = "macos")]
type CFStringRef = *const c_void;
#[cfg(target_os = "macos")]
type TISInputSourceRef = *const c_void;

#[cfg(target_os = "macos")]
#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn TISCopyCurrentKeyboardInputSource() -> TISInputSourceRef;
    fn TISCopyCurrentASCIICapableKeyboardInputSource() -> TISInputSourceRef;
    fn TISGetInputSourceProperty(
        input_source: TISInputSourceRef,
        property_key: CFStringRef,
    ) -> CFTypeRef;
    fn TISSelectInputSource(input_source: TISInputSourceRef) -> i32;
    static kTISPropertyInputSourceID: CFStringRef;
}

#[cfg(target_os = "macos")]
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(value: CFTypeRef);
    fn CFStringGetLength(value: CFStringRef) -> CFIndex;
    fn CFStringGetMaximumSizeForEncoding(length: CFIndex, encoding: CFStringEncoding) -> CFIndex;
    fn CFStringGetCString(
        value: CFStringRef,
        buffer: *mut i8,
        buffer_size: CFIndex,
        encoding: CFStringEncoding,
    ) -> Boolean;
}

#[derive(Clone, Debug)]
pub struct Config {
    pub close_tab: bool,
    pub ghostty_shortcut: String,
}

#[derive(Clone, Debug)]
pub struct WorkspaceTab {
    pub title: String,
    pub working_dir: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Workspace {
    pub name: String,
    pub path: PathBuf,
    pub tabs: Vec<WorkspaceTab>,
}

#[derive(Debug)]
pub struct ShortcutLauncherInputSourceGuard {
    #[cfg(target_os = "macos")]
    previous_source: Option<MacInputSource>,
}

#[cfg(target_os = "macos")]
#[derive(Debug)]
struct MacInputSource {
    raw: TISInputSourceRef,
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

    pub fn ensure_ghostty_shortcut(&self) -> Result<bool> {
        let sync = self.preview_ghostty_shortcut_sync();
        self.sync_ghostty_shortcut_files(&sync)
    }

    pub fn init_shortcuts(&mut self) -> Result<GhosttyShortcutSync> {
        self.config.ghostty_shortcut = DEFAULT_GHOSTTY_SHORTCUT.to_string();
        self.write_config()?;
        let sync = self.sync_ghostty_shortcut()?;
        self.cleanup_legacy_shortcut_artifacts().ok();
        Ok(sync)
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
                path,
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
        self.sync_ghostty_shortcut_files(&sync)?;
        Ok(sync)
    }

    fn sync_ghostty_shortcut_files(&self, sync: &GhosttyShortcutSync) -> Result<bool> {
        if is_shortcut_disabled(&self.config.ghostty_shortcut) {
            let config_changed =
                sync_ghostty_include_reference(&sync.config_path, &sync.include_path, false)?;
            let include_removed = remove_file_if_exists(&sync.include_path)?;
            return Ok(config_changed || include_removed);
        }

        let include_changed = self.write_ghostty_shortcut_include(&sync.include_path)?;
        let config_changed =
            sync_ghostty_include_reference(&sync.config_path, &sync.include_path, true)?;
        Ok(config_changed || include_changed)
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

    fn launchctl_domain(&self) -> Result<String> {
        let output = Command::new("id")
            .arg("-u")
            .output()
            .context("failed to resolve current user id")?;

        if !output.status.success() {
            bail!("failed to resolve current user id");
        }

        let uid = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if uid.is_empty() {
            bail!("failed to resolve current user id");
        }

        Ok(format!("gui/{uid}"))
    }

    fn legacy_hotkey_plist_path(&self) -> Result<PathBuf> {
        let home = home_dir().ok_or_else(|| anyhow!("failed to resolve home directory"))?;
        Ok(home
            .join("Library/LaunchAgents")
            .join(LEGACY_HOTKEY_PLIST_NAME))
    }

    fn bootout_legacy_hotkey_agent(&self) -> Result<()> {
        let plist_path = self.legacy_hotkey_plist_path()?;
        let domain = self.launchctl_domain()?;
        let status = Command::new("launchctl")
            .args(["bootout", &domain])
            .arg(&plist_path)
            .status()
            .with_context(|| format!("failed to stop {}", plist_path.display()))?;

        if !status.success() {
            bail!("failed to stop legacy hotkey agent");
        }

        Ok(())
    }

    fn legacy_hotkey_loaded(&self) -> bool {
        let Ok(domain) = self.launchctl_domain() else {
            return false;
        };

        match Command::new("launchctl")
            .args(["print", &format!("{domain}/{LEGACY_HOTKEY_SERVICE_LABEL}")])
            .output()
        {
            Ok(output) => output.status.success(),
            Err(_) => false,
        }
    }

    fn cleanup_legacy_shortcut_artifacts(&self) -> Result<()> {
        if self.legacy_hotkey_loaded() {
            self.bootout_legacy_hotkey_agent().ok();
        }

        let plist_path = self.legacy_hotkey_plist_path()?;
        if plist_path.exists() {
            fs::remove_file(&plist_path)
                .with_context(|| format!("failed to remove {}", plist_path.display()))?;
        }

        for path in [
            self.base_dir.join(LEGACY_HOTKEY_LOG_NAME),
            self.base_dir.join(LEGACY_LAUNCHER_SCRIPT_NAME),
        ] {
            if path.exists() {
                fs::remove_file(&path)
                    .with_context(|| format!("failed to remove {}", path.display()))?;
            }
        }

        Ok(())
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

impl ShortcutLauncherInputSourceGuard {
    pub fn activate_for_tui() -> Result<Self> {
        #[cfg(target_os = "macos")]
        {
            let current = MacInputSource::current_keyboard()
                .context("failed to resolve the current macOS input source")?;
            let ascii = MacInputSource::current_ascii_capable()
                .context("failed to resolve an ASCII-capable macOS input source")?;

            let should_switch = should_switch_to_ascii_input_source(
                current.id().ok().as_deref(),
                ascii.id().ok().as_deref(),
                current.ptr_eq(&ascii),
            );

            if !should_switch {
                return Ok(Self {
                    previous_source: None,
                });
            }

            ascii
                .select()
                .context("failed to switch gtab to an ASCII-capable input source")?;

            Ok(Self {
                previous_source: Some(current),
            })
        }

        #[cfg(not(target_os = "macos"))]
        {
            Ok(Self {})
        }
    }

    fn restore(&mut self) -> Result<()> {
        #[cfg(target_os = "macos")]
        {
            let Some(previous_source) = self.previous_source.take() else {
                return Ok(());
            };

            previous_source
                .select()
                .context("failed to restore the previous macOS input source")?;
        }

        Ok(())
    }
}

impl Drop for ShortcutLauncherInputSourceGuard {
    fn drop(&mut self) {
        if let Err(error) = self.restore() {
            eprintln!("warning: {error}");
        }
    }
}

#[cfg(target_os = "macos")]
impl MacInputSource {
    fn current_keyboard() -> Result<Self> {
        let raw = unsafe { TISCopyCurrentKeyboardInputSource() };
        Self::new(raw, "current keyboard input source was unavailable")
    }

    fn current_ascii_capable() -> Result<Self> {
        let raw = unsafe { TISCopyCurrentASCIICapableKeyboardInputSource() };
        Self::new(raw, "ASCII-capable keyboard input source was unavailable")
    }

    fn new(raw: TISInputSourceRef, context: &str) -> Result<Self> {
        if raw.is_null() {
            bail!("{context}");
        }

        Ok(Self { raw })
    }

    fn id(&self) -> Result<String> {
        let raw = unsafe { TISGetInputSourceProperty(self.raw, kTISPropertyInputSourceID) };
        cf_string_to_string(raw as CFStringRef)
            .context("failed to read the input source identifier")
    }

    fn ptr_eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }

    fn select(&self) -> Result<()> {
        let status = unsafe { TISSelectInputSource(self.raw) };
        if status == 0 {
            return Ok(());
        }

        bail!("macOS returned OSStatus {status} while selecting an input source")
    }
}

#[cfg(target_os = "macos")]
impl Drop for MacInputSource {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe { CFRelease(self.raw) };
        }
    }
}

fn should_switch_to_ascii_input_source(
    current_id: Option<&str>,
    ascii_id: Option<&str>,
    same_source: bool,
) -> bool {
    if same_source {
        return false;
    }

    match (current_id, ascii_id) {
        (Some(current_id), Some(ascii_id)) => current_id != ascii_id,
        _ => true,
    }
}

#[cfg(target_os = "macos")]
fn cf_string_to_string(value: CFStringRef) -> Result<String> {
    if value.is_null() {
        bail!("CFString value was null");
    }

    let length = unsafe { CFStringGetLength(value) };
    let buffer_size =
        unsafe { CFStringGetMaximumSizeForEncoding(length, K_CF_STRING_ENCODING_UTF8) };
    if buffer_size < 0 {
        bail!("failed to size the UTF-8 input source buffer");
    }

    let mut buffer = vec![0_i8; buffer_size as usize + 1];
    let ok = unsafe {
        CFStringGetCString(
            value,
            buffer.as_mut_ptr(),
            buffer.len() as CFIndex,
            K_CF_STRING_ENCODING_UTF8,
        )
    };
    if ok == 0 {
        bail!("failed to decode the input source identifier as UTF-8");
    }

    let string = unsafe { CStr::from_ptr(buffer.as_ptr()) };
    Ok(string.to_string_lossy().into_owned())
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
            self.ghostty_shortcut,
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

    if is_shortcut_disabled(&normalized) {
        return Ok("off".to_string());
    }

    if normalized.contains('=') || normalized.contains('\n') || normalized.contains('\r') {
        bail!("ghostty_shortcut contains invalid characters");
    }

    Ok(normalized)
}

fn is_shortcut_disabled(shortcut: &str) -> bool {
    matches!(shortcut.trim(), "off" | "none" | "disabled")
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

fn parse_workspace_tabs(script: &str) -> Vec<WorkspaceTab> {
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
        .map(|(index, tab)| WorkspaceTab {
            title: match tab.title.filter(|title| !title.is_empty()) {
                Some(title) => title,
                None => fallback_tab_name(index + 1, tab.working_dir.as_deref()),
            },
            working_dir: tab
                .working_dir
                .filter(|working_dir| !working_dir.is_empty()),
        })
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
    if is_shortcut_disabled(shortcut) {
        return "# Managed by gtab. Update this with `gtab init` or `gtab set ghostty_shortcut`.\n# Ghostty-local shortcut is disabled.\n".to_string();
    }

    format!(
        "# Managed by gtab. Update this with `gtab init` or `gtab set ghostty_shortcut`.\n# Default Ghostty-local shortcut: send `gtab` to the focused shell for same-tab launch.\nkeybind = {shortcut}=text:gtab\\x0d\n"
    )
}

fn sync_ghostty_include_reference(
    config_path: &Path,
    include_path: &Path,
    enabled: bool,
) -> Result<bool> {
    if enabled {
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
    }

    let existing = match fs::read_to_string(config_path) {
        Ok(existing) => existing,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if enabled {
                let next = render_ghostty_config_with_gtab_include(&[], include_path);
                fs::write(config_path, next)
                    .with_context(|| format!("failed to write {}", config_path.display()))?;
                return Ok(true);
            }
            return Ok(false);
        }
        Err(error) => {
            return Err(error).with_context(|| format!("failed to read {}", config_path.display()));
        }
    };

    let existing_lines: Vec<String> = existing.lines().map(str::to_string).collect();
    let stripped_lines = strip_gtab_include_reference(&existing_lines, include_path);
    let next = if enabled {
        render_ghostty_config_with_gtab_include(&stripped_lines, include_path)
    } else {
        render_ghostty_config(&stripped_lines)
    };

    if next == existing {
        return Ok(false);
    }

    fs::write(config_path, next)
        .with_context(|| format!("failed to write {}", config_path.display()))?;
    Ok(true)
}

fn strip_gtab_include_reference(lines: &[String], include_path: &Path) -> Vec<String> {
    let mut kept: Vec<String> = Vec::with_capacity(lines.len());
    let mut index = 0;

    while index < lines.len() {
        if is_gtab_include_reference_line(&lines[index], include_path) {
            if kept.last().map(|line| line.trim()) == Some("# gtab managed include") {
                kept.pop();
            }
            if kept.last().is_some_and(|line| line.trim().is_empty()) {
                kept.pop();
            }
            index += 1;
            continue;
        }

        kept.push(lines[index].clone());
        index += 1;
    }

    kept
}

fn render_ghostty_config_with_gtab_include(lines: &[String], include_path: &Path) -> String {
    let mut next = render_ghostty_config(lines);

    if !next.is_empty() {
        next.push('\n');
    }

    next.push_str("# gtab managed include\n");
    next.push_str(&format!("config-file = \"{}\"\n", include_path.display()));
    next
}

fn render_ghostty_config(lines: &[String]) -> String {
    if lines.is_empty() {
        return String::new();
    }

    let mut rendered = lines.join("\n");
    rendered.push('\n');
    rendered
}

fn is_gtab_include_reference_line(line: &str, include_path: &Path) -> bool {
    let Some((key, value)) = line.split_once('=') else {
        return false;
    };

    key.trim() == "config-file"
        && value.trim().trim_matches('"') == include_path.display().to_string()
}

fn remove_file_if_exists(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }

    fs::remove_file(path).with_context(|| format!("failed to remove {}", path.display()))?;
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
    let ghostty = env.preview_ghostty_shortcut_sync();
    let ghostty_note = if is_shortcut_disabled(&env.config.ghostty_shortcut) {
        "Ghostty-local shortcut is disabled. Run `gtab init` to restore the default same-shell Cmd+G."
    } else {
        "Ghostty-local shortcut is the default fast path. It types `gtab` into the focused Ghostty shell and only works when Ghostty is focused."
    };

    format!(
        "Settings:\n  close_tab = {close_tab}\n  ghostty_shortcut = {}\n  ghostty_config = {}\n  ghostty_include = {}\n  {ghostty_note}",
        env.config.ghostty_shortcut,
        ghostty.config_path.display(),
        ghostty.include_path.display(),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        Config, TabRow, apple_escape, build_ghostty_shortcut_include, build_workspace_script,
        parse_workspace_tabs, should_switch_to_ascii_input_source, sync_ghostty_include_reference,
    };

    #[test]
    fn config_parses_close_tab_truthy_values() {
        let path = tempfile_path("config");
        std::fs::write(
            &path,
            "close_tab=true\nglobal_shortcut=cmd+g\nghostty_shortcut=cmd+shift+g\n",
        )
        .unwrap();
        let config = Config::load(&path).unwrap();
        assert!(config.close_tab);
        assert_eq!(config.ghostty_shortcut, "cmd+shift+g");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn config_normalizes_disabled_ghostty_shortcut() {
        let path = tempfile_path("config-off");
        std::fs::write(&path, "global_shortcut=cmd+g\nghostty_shortcut=disabled\n").unwrap();
        let config = Config::load(&path).unwrap();
        assert_eq!(config.ghostty_shortcut, "off");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn config_defaults_to_ghostty_local_cmd_g() {
        let config = Config::default();
        assert_eq!(config.ghostty_shortcut, "cmd+g");
    }

    #[test]
    fn config_ignores_removed_launch_mode() {
        let path = tempfile_path("config-launch-mode");
        std::fs::write(
            &path,
            "global_shortcut=cmd+g\nghostty_shortcut=off\nlaunch_mode=inject\n",
        )
        .unwrap();
        let config = Config::load(&path).unwrap();
        assert_eq!(config.ghostty_shortcut, "off");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn ascii_input_source_switch_skips_matching_source_ids() {
        assert!(!should_switch_to_ascii_input_source(
            Some("com.apple.keylayout.ABC"),
            Some("com.apple.keylayout.ABC"),
            false,
        ));
    }

    #[test]
    fn ascii_input_source_switch_skips_matching_source_refs() {
        assert!(!should_switch_to_ascii_input_source(None, None, true));
    }

    #[test]
    fn ascii_input_source_switch_uses_ascii_source_when_current_differs() {
        assert!(should_switch_to_ascii_input_source(
            Some("com.apple.inputmethod.SCIM.ITABC"),
            Some("com.apple.keylayout.ABC"),
            false,
        ));
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

        assert_eq!(tabs.len(), 2);
        assert_eq!(tabs[0].title, "api");
        assert_eq!(tabs[0].working_dir.as_deref(), Some("/tmp/project"));
        assert_eq!(tabs[1].title, "work");
        assert_eq!(tabs[1].working_dir.as_deref(), Some("/tmp/work"));
    }

    #[test]
    fn ghostty_shortcut_include_writes_keybind_command() {
        let include = build_ghostty_shortcut_include("cmd+g");
        assert!(include.contains("Default Ghostty-local shortcut"));
        assert!(include.contains("keybind = cmd+g=text:gtab\\x0d"));
    }

    #[test]
    fn disabled_ghostty_shortcut_include_has_no_keybind() {
        let include = build_ghostty_shortcut_include("off");
        assert!(!include.contains("keybind ="));
        assert!(include.contains("Ghostty-local shortcut is disabled"));
    }

    #[test]
    fn enabling_ghostty_sync_adds_managed_include_reference() {
        let config_path = tempfile_path("ghostty-config-enable");
        let include_path = std::path::Path::new("/tmp/gtab-shortcut.conf");

        let changed = sync_ghostty_include_reference(&config_path, include_path, true).unwrap();

        assert!(changed);
        assert_eq!(
            std::fs::read_to_string(&config_path).unwrap(),
            "# gtab managed include\nconfig-file = \"/tmp/gtab-shortcut.conf\"\n"
        );
    }

    #[test]
    fn enabling_ghostty_sync_is_idempotent() {
        let config_path = tempfile_path("ghostty-config-idempotent");
        let include_path = std::path::Path::new("/tmp/gtab-shortcut.conf");

        sync_ghostty_include_reference(&config_path, include_path, true).unwrap();
        let changed = sync_ghostty_include_reference(&config_path, include_path, true).unwrap();

        assert!(!changed);
        assert_eq!(
            std::fs::read_to_string(&config_path).unwrap(),
            "# gtab managed include\nconfig-file = \"/tmp/gtab-shortcut.conf\"\n"
        );
    }

    #[test]
    fn enabling_ghostty_sync_deduplicates_existing_managed_reference() {
        let config_path = tempfile_path("ghostty-config-dedupe");
        let include_path = std::path::Path::new("/tmp/gtab-shortcut.conf");
        std::fs::write(
            &config_path,
            concat!(
                "font-size = 15\n\n",
                "# gtab managed include\n",
                "config-file = \"/tmp/gtab-shortcut.conf\"\n\n",
                "# gtab managed include\n",
                "config-file = \"/tmp/gtab-shortcut.conf\"\n"
            ),
        )
        .unwrap();

        let changed = sync_ghostty_include_reference(&config_path, include_path, true).unwrap();

        assert!(changed);
        assert_eq!(
            std::fs::read_to_string(&config_path).unwrap(),
            concat!(
                "font-size = 15\n\n",
                "# gtab managed include\n",
                "config-file = \"/tmp/gtab-shortcut.conf\"\n"
            )
        );
    }

    #[test]
    fn disabling_ghostty_sync_removes_managed_include_and_preserves_other_config() {
        let config_path = tempfile_path("ghostty-config-disable");
        let include_path = std::path::Path::new("/tmp/gtab-shortcut.conf");
        std::fs::write(
            &config_path,
            concat!(
                "theme = dark\n",
                "config-file = \"/tmp/shared.conf\"\n\n",
                "# gtab managed include\n",
                "config-file = \"/tmp/gtab-shortcut.conf\"\n",
                "shell-integration = zsh"
            ),
        )
        .unwrap();

        let changed = sync_ghostty_include_reference(&config_path, include_path, false).unwrap();

        assert!(changed);
        assert_eq!(
            std::fs::read_to_string(&config_path).unwrap(),
            concat!(
                "theme = dark\n",
                "config-file = \"/tmp/shared.conf\"\n",
                "shell-integration = zsh\n"
            )
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
