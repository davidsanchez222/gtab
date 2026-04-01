use anyhow::{Context, Result, anyhow, bail};
use gtab::core::{AppEnv, parse_global_hotkey};
use std::{
    ffi::c_void,
    path::{Path, PathBuf},
    ptr,
    sync::OnceLock,
};

type OSStatus = i32;
type EventTargetRef = *mut c_void;
type EventHandlerRef = *mut c_void;
type EventHotKeyRef = *mut c_void;
type EventHandlerCallRef = *mut c_void;
type EventRef = *mut c_void;
type ItemCount = u32;
type OptionBits = u32;
type EventHandlerUPP =
    Option<extern "C" fn(EventHandlerCallRef, EventRef, *mut c_void) -> OSStatus>;

#[repr(C)]
#[derive(Clone, Copy)]
struct EventTypeSpec {
    event_class: u32,
    event_kind: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct EventHotKeyID {
    signature: u32,
    id: u32,
}

const K_EVENT_CLASS_KEYBOARD: u32 = u32::from_be_bytes(*b"keyb");
const K_EVENT_HOTKEY_PRESSED: u32 = 5;
const K_EVENT_HOTKEY_EXCLUSIVE: u32 = 1;
const HOTKEY_SIGNATURE: u32 = u32::from_be_bytes(*b"gtab");

static GTAB_BIN: OnceLock<PathBuf> = OnceLock::new();

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn GetApplicationEventTarget() -> EventTargetRef;
    fn InstallEventHandler(
        target: EventTargetRef,
        handler: EventHandlerUPP,
        num_types: ItemCount,
        list: *const EventTypeSpec,
        user_data: *mut c_void,
        out_handler_ref: *mut EventHandlerRef,
    ) -> OSStatus;
    fn RegisterEventHotKey(
        key_code: u32,
        modifiers: u32,
        hotkey_id: EventHotKeyID,
        target: EventTargetRef,
        options: OptionBits,
        out_ref: *mut EventHotKeyRef,
    ) -> OSStatus;
    fn RunApplicationEventLoop();
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let env = AppEnv::load()?;
    let gtab_bin = resolve_gtab_binary()?;
    let _ = GTAB_BIN.set(gtab_bin);

    let shortcut = env.global_shortcut_display().to_string();
    let parsed = parse_global_hotkey(&shortcut)?;

    let event = EventTypeSpec {
        event_class: K_EVENT_CLASS_KEYBOARD,
        event_kind: K_EVENT_HOTKEY_PRESSED,
    };

    let mut handler_ref = ptr::null_mut();
    unsafe {
        check_status(
            InstallEventHandler(
                GetApplicationEventTarget(),
                Some(on_hotkey_pressed),
                1,
                &event,
                ptr::null_mut(),
                &mut handler_ref,
            ),
            "failed to install hotkey handler",
        )?;

        if let Some(parsed) = parsed {
            let mut hotkey_ref = ptr::null_mut();
            check_status(
                RegisterEventHotKey(
                    parsed.key_code,
                    parsed.modifiers,
                    EventHotKeyID {
                        signature: HOTKEY_SIGNATURE,
                        id: 1,
                    },
                    GetApplicationEventTarget(),
                    K_EVENT_HOTKEY_EXCLUSIVE,
                    &mut hotkey_ref,
                ),
                "failed to register global hotkey",
            )?;
        }

        RunApplicationEventLoop();
    }

    Ok(())
}

extern "C" fn on_hotkey_pressed(
    _handler_call_ref: EventHandlerCallRef,
    _event: EventRef,
    _user_data: *mut c_void,
) -> OSStatus {
    if let Some(path) = GTAB_BIN.get()
        && let Err(error) = launch_gtab(path)
    {
        eprintln!("error: {error}");
    }

    0
}

fn resolve_gtab_binary() -> Result<PathBuf> {
    let current = std::env::current_exe().context("failed to resolve current executable")?;
    let Some(parent) = current.parent() else {
        bail!("failed to resolve executable directory");
    };

    let candidate = parent.join("gtab");
    if candidate.exists() {
        return Ok(candidate);
    }

    bail!("gtab binary not found next to {}", current.display())
}

fn launch_gtab(path: &Path) -> Result<()> {
    let status = std::process::Command::new(path)
        .arg("shortcut-launch")
        .status()
        .with_context(|| format!("failed to launch {} shortcut entrypoint", path.display()))?;

    if status.success() {
        return Ok(());
    }

    bail!("shortcut entrypoint exited with status {status}")
}

fn check_status(status: OSStatus, context: &str) -> Result<()> {
    if status == 0 {
        return Ok(());
    }

    Err(anyhow!("{context}: OSStatus {status}"))
}
