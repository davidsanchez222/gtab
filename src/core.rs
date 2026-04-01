use anyhow::{Context, Result, anyhow, bail};
#[cfg(target_os = "macos")]
use std::ffi::{CStr, c_void};
use std::{
    env, fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const APPLE_EXT: &str = "applescript";
const DEFAULT_GLOBAL_SHORTCUT: &str = "cmd+g";
const DEFAULT_GHOSTTY_SHORTCUT: &str = "off";
const DEFAULT_LAUNCH_MODE: LaunchMode = LaunchMode::Smart;
const GHOSTTY_SHORTCUT_INCLUDE_NAME: &str = "ghostty-shortcut.conf";
const LAUNCHER_SCRIPT_NAME: &str = "launcher.sh";
const SHORTCUT_LAUNCHED_ENV_VAR: &str = "GTAB_LAUNCHED_FROM_SHORTCUT";
const LAUNCHER_AUTO_CLOSE_ENV_VAR: &str = "GTAB_AUTO_CLOSE_LAUNCHER";
const HOTKEY_SERVICE_LABEL: &str = "com.franvy.gtab.hotkey";
const HOTKEY_PLIST_NAME: &str = "com.franvy.gtab.hotkey.plist";
const HOTKEY_LOG_NAME: &str = "gtab-hotkey.log";
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
    pub global_shortcut: String,
    pub ghostty_shortcut: String,
    pub launch_mode: LaunchMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchMode {
    Smart,
    Window,
    Inject,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GhosttyLauncherTarget {
    pub window_id: String,
    pub tab_id: String,
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

#[derive(Clone, Debug, PartialEq, Eq)]
struct GhosttyShortcutContext {
    terminal_id: String,
    terminal_title: String,
    tab_title: String,
    working_dir: Option<String>,
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

    pub fn set_global_shortcut(&mut self, shortcut: &str) -> Result<()> {
        self.config.global_shortcut = normalize_global_shortcut(shortcut)?;
        self.write_config()
    }

    pub fn set_ghostty_shortcut(&mut self, shortcut: &str) -> Result<GhosttyShortcutSync> {
        self.config.ghostty_shortcut = normalize_ghostty_shortcut(shortcut)?;
        self.write_config()?;
        self.sync_ghostty_shortcut()
    }

    pub fn set_launch_mode(&mut self, mode: &str) -> Result<()> {
        self.config.launch_mode = normalize_launch_mode(mode)?;
        self.write_config()
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

    pub fn global_shortcut_display(&self) -> &str {
        &self.config.global_shortcut
    }

    pub fn ghostty_shortcut_display(&self) -> &str {
        &self.config.ghostty_shortcut
    }

    pub fn launch_mode(&self) -> LaunchMode {
        self.config.launch_mode
    }

    pub fn launch_mode_display(&self) -> &'static str {
        self.config.launch_mode.as_str()
    }

    pub fn launch_from_shortcut(&self) -> Result<()> {
        let gtab_path =
            env::current_exe().context("failed to resolve the current gtab executable")?;
        let context = current_ghostty_shortcut_context().ok().flatten();

        if should_inject_shortcut_launch(self.config.launch_mode, context.as_ref()) {
            let terminal_id = context
                .as_ref()
                .map(|context| context.terminal_id.as_str())
                .ok_or_else(|| anyhow!("no focused Ghostty terminal is available for injection"))?;
            inject_gtab_into_ghostty_terminal(terminal_id, &gtab_path)?;
        } else {
            launch_gtab_in_new_window(&gtab_path)?;
        }

        Ok(())
    }

    pub fn hotkey_plist_path(&self) -> Result<PathBuf> {
        let home = home_dir().ok_or_else(|| anyhow!("failed to resolve home directory"))?;
        Ok(home.join("Library/LaunchAgents").join(HOTKEY_PLIST_NAME))
    }

    pub fn hotkey_log_path(&self) -> PathBuf {
        self.base_dir.join(HOTKEY_LOG_NAME)
    }

    pub fn helper_binary_path(&self) -> Result<PathBuf> {
        let current = env::current_exe().context("failed to resolve current executable")?;
        let Some(parent) = current.parent() else {
            bail!("failed to resolve executable directory");
        };

        let helper = parent.join("gtab-hotkey");
        if helper.exists() {
            return Ok(helper);
        }

        bail!("gtab-hotkey helper not found next to {}", current.display())
    }

    pub fn install_hotkey_agent(&self) -> Result<HotkeyAgentStatus> {
        let plist_path = self.hotkey_plist_path()?;
        self.write_hotkey_plist(&plist_path)?;
        self.bootout_hotkey_agent().ok();
        self.bootstrap_hotkey_agent()?;
        self.kickstart_hotkey_agent()?;
        self.hotkey_agent_status()
    }

    pub fn restart_hotkey_agent(&self) -> Result<HotkeyAgentStatus> {
        self.install_hotkey_agent()
    }

    pub fn uninstall_hotkey_agent(&self) -> Result<()> {
        let plist_path = self.hotkey_plist_path()?;
        self.bootout_hotkey_agent().ok();
        if plist_path.exists() {
            fs::remove_file(&plist_path)
                .with_context(|| format!("failed to remove {}", plist_path.display()))?;
        }
        Ok(())
    }

    pub fn hotkey_agent_status(&self) -> Result<HotkeyAgentStatus> {
        let plist_path = self.hotkey_plist_path()?;
        let helper_path = self.helper_binary_path()?;
        let loaded = self.launchctl_print().success();

        Ok(HotkeyAgentStatus {
            global_shortcut: self.config.global_shortcut.clone(),
            plist_path,
            helper_path,
            loaded,
        })
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

    fn write_hotkey_plist(&self, path: &Path) -> Result<bool> {
        let helper_path = self.helper_binary_path()?;
        let log_path = self.hotkey_log_path();
        let content = build_hotkey_launch_agent_plist(&helper_path, &log_path);
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

        Ok(changed)
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

    fn bootstrap_hotkey_agent(&self) -> Result<()> {
        let plist_path = self.hotkey_plist_path()?;
        let domain = self.launchctl_domain()?;
        let status = Command::new("launchctl")
            .args(["bootstrap", &domain])
            .arg(&plist_path)
            .status()
            .with_context(|| format!("failed to bootstrap {}", plist_path.display()))?;

        if !status.success() {
            bail!("failed to bootstrap hotkey agent");
        }

        Ok(())
    }

    fn bootout_hotkey_agent(&self) -> Result<()> {
        let plist_path = self.hotkey_plist_path()?;
        let domain = self.launchctl_domain()?;
        let status = Command::new("launchctl")
            .args(["bootout", &domain])
            .arg(&plist_path)
            .status()
            .with_context(|| format!("failed to stop {}", plist_path.display()))?;

        if !status.success() {
            bail!("failed to stop hotkey agent");
        }

        Ok(())
    }

    fn kickstart_hotkey_agent(&self) -> Result<()> {
        let domain = self.launchctl_domain()?;
        let status = Command::new("launchctl")
            .args([
                "kickstart",
                "-k",
                &format!("{domain}/{HOTKEY_SERVICE_LABEL}"),
            ])
            .status()
            .context("failed to kickstart hotkey agent")?;

        if !status.success() {
            bail!("failed to kickstart hotkey agent");
        }

        Ok(())
    }

    fn launchctl_print(&self) -> std::process::ExitStatus {
        let Ok(domain) = self.launchctl_domain() else {
            return exit_status_from_code(1);
        };

        match Command::new("launchctl")
            .args(["print", &format!("{domain}/{HOTKEY_SERVICE_LABEL}")])
            .output()
        {
            Ok(output) => output.status,
            Err(_) => exit_status_from_code(1),
        }
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

pub fn hotkey_service_label() -> &'static str {
    HOTKEY_SERVICE_LABEL
}

impl LaunchMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Smart => "smart",
            Self::Window => "window",
            Self::Inject => "inject",
        }
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
            } else if key.trim() == "global_shortcut" && !value.trim().is_empty() {
                config.global_shortcut = normalize_global_shortcut(value.trim())?;
            } else if key.trim() == "ghostty_shortcut" && !value.trim().is_empty() {
                config.ghostty_shortcut = normalize_ghostty_shortcut(value.trim())?;
            } else if key.trim() == "launch_mode" && !value.trim().is_empty() {
                config.launch_mode = normalize_launch_mode(value.trim())?;
            }
        }

        Ok(config)
    }

    fn serialize(&self) -> String {
        let close_tab = if self.close_tab { "true" } else { "false" };
        format!(
            "close_tab={close_tab}\nglobal_shortcut={}\nghostty_shortcut={}\nlaunch_mode={}\n",
            self.global_shortcut,
            self.ghostty_shortcut,
            self.launch_mode.as_str()
        )
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            close_tab: false,
            global_shortcut: DEFAULT_GLOBAL_SHORTCUT.to_string(),
            ghostty_shortcut: DEFAULT_GHOSTTY_SHORTCUT.to_string(),
            launch_mode: DEFAULT_LAUNCH_MODE,
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

fn normalize_global_shortcut(shortcut: &str) -> Result<String> {
    let normalized = shortcut.trim().to_lowercase();
    if normalized.is_empty() {
        bail!("global_shortcut cannot be empty");
    }

    if is_shortcut_disabled(&normalized) {
        return Ok("off".to_string());
    }

    if normalized.contains('=') || normalized.contains('\n') || normalized.contains('\r') {
        bail!("global_shortcut contains invalid characters");
    }

    let _ = parse_global_hotkey(&normalized)?;

    Ok(normalized)
}

fn normalize_ghostty_shortcut(shortcut: &str) -> Result<String> {
    let normalized = shortcut.trim().to_lowercase();
    if normalized.is_empty() {
        bail!("ghostty_shortcut cannot be empty");
    }

    if is_shortcut_disabled(&normalized) {
        return Ok(DEFAULT_GHOSTTY_SHORTCUT.to_string());
    }

    if normalized.contains('=') || normalized.contains('\n') || normalized.contains('\r') {
        bail!("ghostty_shortcut contains invalid characters");
    }

    Ok(normalized)
}

fn normalize_launch_mode(mode: &str) -> Result<LaunchMode> {
    match mode.trim().to_lowercase().as_str() {
        "smart" => Ok(LaunchMode::Smart),
        "window" => Ok(LaunchMode::Window),
        "inject" => Ok(LaunchMode::Inject),
        _ => bail!("launch_mode must be 'smart', 'window', or 'inject'"),
    }
}

fn is_shortcut_disabled(shortcut: &str) -> bool {
    matches!(shortcut.trim(), "off" | "none" | "disabled")
}

pub fn parse_global_hotkey(shortcut: &str) -> Result<Option<ParsedHotkey>> {
    let normalized = shortcut.trim().to_lowercase();
    if is_shortcut_disabled(&normalized) {
        return Ok(None);
    }

    let mut modifiers = 0u32;
    let mut key = None;

    for part in normalized.split('+') {
        match part.trim() {
            "cmd" | "command" => modifiers |= 1 << 8,
            "shift" => modifiers |= 1 << 9,
            "alt" | "option" => modifiers |= 1 << 11,
            "ctrl" | "control" => modifiers |= 1 << 12,
            value if !value.is_empty() && key.is_none() => key = Some(value),
            _ => bail!("unsupported global_shortcut: {shortcut}"),
        }
    }

    let Some(key) = key else {
        bail!("global_shortcut must include a key");
    };

    let key_code = key_code_for_shortcut(key)
        .ok_or_else(|| anyhow!("unsupported global_shortcut key: {key}"))?;

    Ok(Some(ParsedHotkey {
        key_code,
        modifiers,
    }))
}

fn key_code_for_shortcut(key: &str) -> Option<u32> {
    Some(match key {
        "a" => 0x00,
        "s" => 0x01,
        "d" => 0x02,
        "f" => 0x03,
        "h" => 0x04,
        "g" => 0x05,
        "z" => 0x06,
        "x" => 0x07,
        "c" => 0x08,
        "v" => 0x09,
        "b" => 0x0B,
        "q" => 0x0C,
        "w" => 0x0D,
        "e" => 0x0E,
        "r" => 0x0F,
        "y" => 0x10,
        "t" => 0x11,
        "1" => 0x12,
        "2" => 0x13,
        "3" => 0x14,
        "4" => 0x15,
        "6" => 0x16,
        "5" => 0x17,
        "=" => 0x18,
        "9" => 0x19,
        "7" => 0x1A,
        "-" => 0x1B,
        "8" => 0x1C,
        "0" => 0x1D,
        "]" => 0x1E,
        "o" => 0x1F,
        "u" => 0x20,
        "[" => 0x21,
        "i" => 0x22,
        "p" => 0x23,
        "enter" | "return" => 0x24,
        "l" => 0x25,
        "j" => 0x26,
        "'" => 0x27,
        "k" => 0x28,
        ";" => 0x29,
        "\\" => 0x2A,
        "," => 0x2B,
        "/" => 0x2C,
        "n" => 0x2D,
        "m" => 0x2E,
        "." => 0x2F,
        "tab" => 0x30,
        "space" => 0x31,
        "`" => 0x32,
        "backspace" => 0x33,
        "esc" | "escape" => 0x35,
        "delete" => 0x75,
        "left" => 0x7B,
        "right" => 0x7C,
        "down" => 0x7D,
        "up" => 0x7E,
        _ => return None,
    })
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

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn build_window_launcher_applescript(gtab_path_expr: &str) -> String {
    format!(
        r#"set gtabPath to {gtab_path_expr}
tell application "Ghostty"
  activate
  set cfg to new surface configuration
  set command of cfg to gtabPath
  set environment variables of cfg to {{"{SHORTCUT_LAUNCHED_ENV_VAR}=1", "{LAUNCHER_AUTO_CLOSE_ENV_VAR}=1"}}
  set wait after command of cfg to false
  new window with configuration cfg
end tell
"#
    )
}

fn build_ghostty_shortcut_include(shortcut: &str) -> String {
    if is_shortcut_disabled(shortcut) {
        return "# Managed by gtab. Update this in gtab settings.\n# Legacy Ghostty text-injection shortcut is disabled.\n".to_string();
    }

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

exec "$GTAB_BIN" shortcut-launch
"#
    .to_string()
}

fn build_hotkey_launch_agent_plist(helper_path: &Path, log_path: &Path) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{HOTKEY_SERVICE_LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{}</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardOutPath</key>
  <string>{}</string>
  <key>StandardErrorPath</key>
  <string>{}</string>
</dict>
</plist>
"#,
        helper_path.display(),
        log_path.display(),
        log_path.display()
    )
}

fn exit_status_from_code(code: i32) -> std::process::ExitStatus {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        std::process::ExitStatus::from_raw(code)
    }

    #[cfg(not(unix))]
    {
        let _ = code;
        unreachable!()
    }
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

#[derive(Clone, Debug)]
pub struct HotkeyAgentStatus {
    pub global_shortcut: String,
    pub plist_path: PathBuf,
    pub helper_path: PathBuf,
    pub loaded: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParsedHotkey {
    pub key_code: u32,
    pub modifiers: u32,
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
    let legacy_note = if is_shortcut_disabled(&env.config.ghostty_shortcut) {
        "Legacy Ghostty shortcut is disabled so the built-in global Cmd+G helper can own the shortcut."
    } else {
        "Legacy Ghostty shortcut sends `gtab` to the focused shell and can fail in Claude Code/Codex."
    };
    let launch_note = match env.launch_mode() {
        LaunchMode::Smart => {
            "launch_mode = smart prefers the current Ghostty prompt and falls back to a new window when it is not safe to inject."
        }
        LaunchMode::Window => {
            "launch_mode = window always opens a separate Ghostty launcher window."
        }
        LaunchMode::Inject => {
            "launch_mode = inject always types gtab into the current Ghostty terminal when one is focused."
        }
    };

    format!(
        "Settings:\n  close_tab = {close_tab}\n  global_shortcut = {}\n  ghostty_shortcut = {}\n  launch_mode = {}\n  launch_agent = {}\n  helper = {}\n  {launch_note}\n  {legacy_note}",
        env.config.global_shortcut,
        env.config.ghostty_shortcut,
        env.launch_mode_display(),
        env.hotkey_plist_path()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|_| "~".to_string()),
        env.helper_binary_path()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|_| "gtab-hotkey".to_string())
    )
}

pub fn format_shortcut_guide(env: &AppEnv, launcher_path: &Path) -> String {
    let mode_note = match env.launch_mode() {
        LaunchMode::Smart => {
            "In smart mode, gtab prefers the current Ghostty prompt and falls back to a new Ghostty window when it is not safe to inject."
        }
        LaunchMode::Window => {
            "In window mode, gtab always opens a separate Ghostty launcher window."
        }
        LaunchMode::Inject => {
            "In inject mode, gtab always types into the current Ghostty terminal when Ghostty is focused."
        }
    };

    format!(
        "Shortcut launcher:\n  {}\n\nBind Cmd+G in macOS Shortcuts, Raycast, or Hammerspoon to run this script.\nIt runs the same shortcut-launch logic as the built-in hotkey helper. {mode_note}\n\nLegacy Ghostty keybind:\n  {}\n  This sends `gtab` to the focused shell and can fail in Claude Code, Codex, vim, or fzf.",
        launcher_path.display(),
        env.ghostty_shortcut_display()
    )
}

fn launch_gtab_in_new_window(path: &Path) -> Result<()> {
    let script = build_window_launcher_applescript(&format!(
        "\"{}\"",
        apple_escape(&path.display().to_string())
    ));

    run_osascript(&script)
        .with_context(|| format!("failed to launch {} in Ghostty", path.display()))?;
    Ok(())
}

fn inject_gtab_into_ghostty_terminal(terminal_id: &str, path: &Path) -> Result<()> {
    let command = format!(
        "{SHORTCUT_LAUNCHED_ENV_VAR}=1 {}",
        shell_single_quote(&path.display().to_string())
    );
    let script = format!(
        r#"tell application "Ghostty"
  set terminalId to "{}"
  if (count of (every terminal whose id is terminalId)) is 0 then
    error "focused Ghostty terminal is no longer available"
  end if
  set target to first terminal whose id is terminalId
  input text "{}" to target
  send key "enter" to target
end tell"#,
        apple_escape(terminal_id),
        apple_escape(&command)
    );

    run_osascript(&script).context("failed to inject gtab into the current Ghostty terminal")?;
    Ok(())
}

pub fn launched_from_shortcut() -> bool {
    matches!(
        env::var(SHORTCUT_LAUNCHED_ENV_VAR).as_deref(),
        Ok("1" | "true" | "on")
    )
}

pub fn launched_from_shortcut_launcher() -> bool {
    matches!(
        env::var(LAUNCHER_AUTO_CLOSE_ENV_VAR).as_deref(),
        Ok("1" | "true" | "on")
    )
}

fn current_ghostty_shortcut_context() -> Result<Option<GhosttyShortcutContext>> {
    let output = run_osascript(
        r#"tell application "Ghostty"
  if not frontmost then
    return ""
  end if
  if (count of windows) is 0 then
    return ""
  end if
  set win to front window
  if selected tab of win is missing value then
    return ""
  end if
  set tabRef to selected tab of win
  set termRef to focused terminal of tabRef
  if termRef is missing value then
    return ""
  end if
  try
    set wd to working directory of termRef
  on error
    set wd to ""
  end try
  return ((id of termRef) as text) & linefeed & (name of termRef) & linefeed & (name of tabRef) & linefeed & wd
end tell"#,
    )
    .context("failed to resolve the current Ghostty terminal context")?;

    let mut lines = output.lines();
    let Some(terminal_id) = lines
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };

    let terminal_title = lines.next().map(str::trim).unwrap_or_default().to_string();
    let tab_title = lines.next().map(str::trim).unwrap_or_default().to_string();
    let working_dir = lines
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    Ok(Some(GhosttyShortcutContext {
        terminal_id: terminal_id.to_string(),
        terminal_title,
        tab_title,
        working_dir,
    }))
}

pub fn current_ghostty_launcher_target() -> Result<Option<GhosttyLauncherTarget>> {
    let output = run_osascript(
        r#"tell application "Ghostty"
  if (count of windows) is 0 then
    return ""
  end if
  set win to front window
  if selected tab of win is missing value then
    return ""
  end if
  return ((id of win) as text) & linefeed & ((id of selected tab of win) as text)
end tell"#,
    )
    .context("failed to resolve the current Ghostty launcher target")?;

    let mut lines = output.lines();
    let Some(window_id) = lines
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    let Some(tab_id) = lines
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };

    Ok(Some(GhosttyLauncherTarget {
        window_id: window_id.to_string(),
        tab_id: tab_id.to_string(),
    }))
}

pub fn close_ghostty_launcher_tab_later(target: &GhosttyLauncherTarget) -> Result<()> {
    let script = format!(
        r#"delay 0.15
tell application "Ghostty"
  set winId to "{}"
  set tabId to "{}"
  if (count of (every window whose id is winId)) is greater than 0 then
    set win to first window whose id is winId
    if (count of (every tab of win whose id is tabId)) is greater than 0 then
      close tab (first tab of win whose id is tabId)
    end if
  end if
end tell"#,
        apple_escape(&target.window_id),
        apple_escape(&target.tab_id)
    );

    run_osascript(&script).context("failed to close the launcher Ghostty tab")?;
    Ok(())
}

fn normalize_ghostty_title(title: &str) -> String {
    title
        .trim()
        .trim_start_matches("⠐ ")
        .trim_start_matches("🔔 ")
        .trim()
        .to_string()
}

fn prompt_title_matches_working_dir(title: &str, working_dir: &str) -> bool {
    let title = normalize_ghostty_title(title);
    let working_dir = working_dir.trim();
    if title.is_empty() || working_dir.is_empty() {
        return false;
    }

    if title == working_dir {
        return true;
    }

    let home = home_dir()
        .unwrap_or_else(|| PathBuf::from("/"))
        .to_string_lossy()
        .into_owned();

    if working_dir == home && title == "~" {
        return true;
    }

    if let Some(relative) = working_dir
        .strip_prefix(&home)
        .and_then(|value| value.strip_prefix('/'))
        && title == format!("~/{relative}")
    {
        return true;
    }

    if let Some(name) = Path::new(working_dir)
        .file_name()
        .and_then(|name| name.to_str())
        && title == name
    {
        return true;
    }

    if let Some(suffix) = title.strip_prefix("…/") {
        return working_dir.ends_with(&format!("/{suffix}"));
    }

    false
}

fn prompt_title_looks_like_shell(title: &str) -> bool {
    matches!(
        normalize_ghostty_title(title).as_str(),
        "bash" | "fish" | "nu" | "sh" | "zsh"
    )
}

fn current_terminal_looks_safe_for_inject(context: &GhosttyShortcutContext) -> bool {
    if let Some(working_dir) = context.working_dir.as_deref() {
        if prompt_title_matches_working_dir(&context.terminal_title, working_dir)
            || prompt_title_matches_working_dir(&context.tab_title, working_dir)
        {
            return true;
        }
    }

    prompt_title_looks_like_shell(&context.terminal_title)
        || prompt_title_looks_like_shell(&context.tab_title)
}

fn should_inject_shortcut_launch(
    mode: LaunchMode,
    context: Option<&GhosttyShortcutContext>,
) -> bool {
    match mode {
        LaunchMode::Window => false,
        LaunchMode::Inject => context.is_some(),
        LaunchMode::Smart => context.is_some_and(current_terminal_looks_safe_for_inject),
    }
}

pub fn format_hotkey_status(
    status: &HotkeyAgentStatus,
    launch_mode: &str,
    legacy_shortcut: &str,
) -> String {
    let loaded = if status.loaded {
        "loaded"
    } else {
        "not loaded"
    };
    format!(
        "Hotkey Agent:\n  global_shortcut = {}\n  launch_mode = {launch_mode}\n  service = {HOTKEY_SERVICE_LABEL} ({loaded})\n  plist = {}\n  helper = {}\n  legacy_ghostty_shortcut = {}",
        status.global_shortcut,
        status.plist_path.display(),
        status.helper_path.display(),
        legacy_shortcut
    )
}

pub fn format_hotkey_doctor(
    status: &HotkeyAgentStatus,
    launch_mode: &str,
    legacy_shortcut: &str,
    log_path: &Path,
) -> String {
    let launchd_state = if status.loaded {
        "launchd service is loaded"
    } else {
        "launchd service is not loaded"
    };
    let legacy_state = if is_shortcut_disabled(legacy_shortcut) {
        "legacy Ghostty text-injection shortcut is disabled"
    } else {
        "legacy Ghostty text-injection shortcut is still enabled"
    };
    let launch_mode_state = match launch_mode {
        "smart" => {
            "launch_mode smart injects into the current Ghostty prompt only when the title still looks like a prompt; otherwise it falls back to a new window"
        }
        "window" => "launch_mode window always uses a separate Ghostty launcher window",
        "inject" => {
            "launch_mode inject always types gtab into the current Ghostty terminal when one is focused"
        }
        _ => "launch_mode is unknown",
    };

    format!(
        "Hotkey Doctor:\n  shortcut = {}\n  launch_mode = {launch_mode}\n  {launchd_state}\n  {legacy_state}\n  {launch_mode_state}\n  plist = {}\n  helper = {}\n  log = {}\n  Press Cmd+G in Ghostty to test after reload/restart.",
        status.global_shortcut,
        status.plist_path.display(),
        status.helper_path.display(),
        log_path.display()
    )
}

#[cfg(test)]
mod tests {
    use super::{
        Config, HOTKEY_SERVICE_LABEL, LaunchMode, TabRow, apple_escape,
        build_ghostty_shortcut_include, build_hotkey_launch_agent_plist, build_launcher_script,
        build_window_launcher_applescript, build_workspace_script, normalize_launch_mode,
        parse_global_hotkey, parse_workspace_tabs, prompt_title_matches_working_dir,
        should_inject_shortcut_launch, should_switch_to_ascii_input_source,
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
        assert_eq!(config.global_shortcut, "cmd+g");
        assert_eq!(config.ghostty_shortcut, "cmd+shift+g");
        assert_eq!(config.launch_mode, LaunchMode::Smart);
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
    fn config_defaults_global_shortcut_to_cmd_g() {
        let config = Config::default();
        assert_eq!(config.global_shortcut, "cmd+g");
        assert_eq!(config.ghostty_shortcut, "off");
        assert_eq!(config.launch_mode, LaunchMode::Smart);
    }

    #[test]
    fn launch_mode_parses_known_values() {
        assert_eq!(normalize_launch_mode("smart").unwrap(), LaunchMode::Smart);
        assert_eq!(normalize_launch_mode("window").unwrap(), LaunchMode::Window);
        assert_eq!(normalize_launch_mode("inject").unwrap(), LaunchMode::Inject);
    }

    #[test]
    fn prompt_title_matching_accepts_shell_integration_titles() {
        assert!(prompt_title_matches_working_dir("api", "/tmp/project/api"));
        assert!(prompt_title_matches_working_dir(
            "~/work/api",
            "/Users/fran/work/api"
        ));
        assert!(prompt_title_matches_working_dir(
            "…/work/api",
            "/tmp/demo/work/api"
        ));
        assert!(!prompt_title_matches_working_dir(
            "nvim",
            "/tmp/project/api"
        ));
    }

    #[test]
    fn smart_launch_only_injects_when_context_looks_like_prompt() {
        let prompt_context = super::GhosttyShortcutContext {
            terminal_id: "term-1".to_string(),
            terminal_title: "api".to_string(),
            tab_title: "api".to_string(),
            working_dir: Some("/tmp/project/api".to_string()),
        };
        let command_context = super::GhosttyShortcutContext {
            terminal_id: "term-1".to_string(),
            terminal_title: "claude".to_string(),
            tab_title: "claude".to_string(),
            working_dir: Some("/tmp/project/api".to_string()),
        };

        assert!(should_inject_shortcut_launch(
            LaunchMode::Smart,
            Some(&prompt_context)
        ));
        assert!(!should_inject_shortcut_launch(
            LaunchMode::Smart,
            Some(&command_context)
        ));
        assert!(!should_inject_shortcut_launch(LaunchMode::Smart, None));
        assert!(should_inject_shortcut_launch(
            LaunchMode::Inject,
            Some(&command_context)
        ));
        assert!(!should_inject_shortcut_launch(LaunchMode::Inject, None));
        assert!(!should_inject_shortcut_launch(
            LaunchMode::Window,
            Some(&prompt_context)
        ));
    }

    #[test]
    fn global_shortcut_supports_shifted_symbol_keys() {
        assert!(parse_global_hotkey("cmd+shift+/").unwrap().is_some());
        assert!(parse_global_hotkey("cmd+=").unwrap().is_some());
    }

    #[test]
    fn global_shortcut_supports_named_navigation_keys() {
        assert!(parse_global_hotkey("cmd+left").unwrap().is_some());
        assert!(parse_global_hotkey("cmd+tab").unwrap().is_some());
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
        assert!(include.contains("keybind = cmd+g=text:gtab\\x0d"));
    }

    #[test]
    fn disabled_ghostty_shortcut_include_has_no_keybind() {
        let include = build_ghostty_shortcut_include("off");
        assert!(!include.contains("keybind ="));
        assert!(include.contains("shortcut is disabled"));
    }

    #[test]
    fn launcher_script_prefers_path_and_homebrew_fallback() {
        let script = build_launcher_script();
        assert!(script.contains("command -v gtab"));
        assert!(script.contains("/opt/homebrew/bin/gtab"));
        assert!(script.contains("exec \"$GTAB_BIN\" shortcut-launch"));
        assert!(!script.contains("osascript - \"$GTAB_BIN\""));
    }

    #[test]
    fn window_launcher_applescript_sets_shortcut_env_flags() {
        let script = build_window_launcher_applescript("\"/opt/homebrew/bin/gtab\"");
        assert!(script.contains("set gtabPath to \"/opt/homebrew/bin/gtab\""));
        assert!(script.contains("new window with configuration cfg"));
        assert!(script.contains("GTAB_LAUNCHED_FROM_SHORTCUT=1"));
        assert!(script.contains("GTAB_AUTO_CLOSE_LAUNCHER=1"));
    }

    #[test]
    fn launch_agent_plist_points_to_helper_binary() {
        let plist = build_hotkey_launch_agent_plist(
            std::path::Path::new("/opt/homebrew/bin/gtab-hotkey"),
            std::path::Path::new("/tmp/gtab-hotkey.log"),
        );
        assert!(plist.contains(HOTKEY_SERVICE_LABEL));
        assert!(plist.contains("/opt/homebrew/bin/gtab-hotkey"));
        assert!(plist.contains("/tmp/gtab-hotkey.log"));
    }

    fn tempfile_path(name: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("gtab-{name}-{nanos}.tmp"))
    }
}
