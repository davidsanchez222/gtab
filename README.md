# gtab — Ghostty Tab Workspace Manager

**English** | [中文](README_CN.md)

A Rust-powered Ghostty workspace manager with a keyboard-first TUI and compatible CLI commands. Save the current terminal layout as a workspace, search saved workspaces, inspect their saved tabs in a dialog-style TUI, then relaunch them with one key or one command.

![gtab demo](Gtab.gif)

---

## Requirements

- macOS
- [Ghostty](https://ghostty.org) terminal
- Rust toolchain (`cargo`, `rustc`) for local builds

---

## Build From Source

```bash
cargo build --release
./target/release/gtab
```

For development:

```bash
cargo run --
cargo fmt
cargo test
```

Note: the repository still keeps the original Bash prototype in `./gtab` during the migration. The Rust application lives in `src/`.

---

## Usage

```text
gtab                 Open the interactive TUI
gtab tui             Open the interactive TUI
gtab <name>          Launch a workspace directly
gtab save <name>     Save current Ghostty window as a workspace
gtab list            List all saved workspaces
gtab edit <name>     Edit a workspace script
gtab remove <name>   Remove a workspace
gtab hotkey status   Show the built-in global hotkey helper status
gtab hotkey doctor   Show diagnostics for the hotkey helper
gtab set             Show settings
gtab set close_tab on|off
gtab set launch_mode smart|window|inject
gtab set global_shortcut cmd+g
gtab set ghostty_shortcut off|cmd+shift+g
```

### Quick start

1. Open Ghostty and set up your tabs.
2. Run `gtab save myproject` to capture the layout.
3. Run `gtab` to open the TUI and search, inspect, or launch saved workspaces.
4. After `brew install gtab`, the built-in hotkey helper should register `Cmd+G` automatically.
5. If `Cmd+G` is not opening gtab, run `gtab hotkey doctor`.

### TUI shortcuts

```text
Enter   launch selected workspace
/       start live search
j/k     move through the workspace list
↑/↓     move through the workspace list
PgUp/Dn jump by a screen
Home    jump to the top
End/G   jump to the bottom
a       save the current Ghostty window
e       edit the selected workspace in $EDITOR
d       delete the selected workspace
g       edit the global shortcut from the quick settings pane
r       reload the workspace list
t       open settings
?       open help
mouse   click to select, double-click to launch, click shortcut to edit
q       quit
```

The TUI keeps a stable dialog layout: the left pane shows bracketed workspace labels, the middle pane shows the selected workspace's tabs as horizontal labels in saved order, and the right pane exposes the global shortcut and helper status.

---

## Configuration

Workspace scripts are stored in `~/.config/gtab/` by default.

Override the directory with the `GTAB_DIR` environment variable:

```bash
export GTAB_DIR="$HOME/Scripts/ghostty"
```

Each workspace is stored as a plain AppleScript file (`.applescript`) that you can inspect and edit freely with `gtab edit <name>`.
The `config` file in the same directory currently supports:

- `close_tab=true|false`
- `launch_mode=smart|window|inject`
- `global_shortcut=cmd+g`
- `ghostty_shortcut=off|cmd+shift+g`

gtab also installs a user LaunchAgent and a companion helper binary (`gtab-hotkey`) to register a global macOS shortcut. The default `global_shortcut` is `cmd+g`.

When gtab opens its TUI, it temporarily switches macOS to the most recently used ASCII-capable input source so single-key actions like `q`, `g`, `s`, and `t` keep working. The previous input source is restored when you leave the TUI or launch a workspace.
The new `launch_mode` setting controls how the built-in shortcut opens gtab. `smart` is the default: it prefers the current Ghostty prompt and falls back to a separate Ghostty window when the current tab does not look safe to inject. `window` always uses a separate launcher window. `inject` always types gtab into the current Ghostty terminal when Ghostty is focused.

The old Ghostty text-injection shortcut is still available through `ghostty_shortcut`, but it is legacy-only. Keep it set to `off` unless you are debugging, because it just sends `gtab` plus Enter to the focused shell and can fail inside Claude Code, Codex, vim, or fzf.

---

## How it works

`gtab save` uses Ghostty's AppleScript API to read each tab's working directory and title, then generates an AppleScript that recreates the layout. The Rust app keeps the same workspace format for compatibility while adding a TUI layer on top.

---

## License

MIT
