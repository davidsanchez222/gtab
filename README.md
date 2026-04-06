# gtab

**English** | [中文](README_CN.md)

`gtab` is a lightweight workspace manager for [Ghostty](https://ghostty.org) on macOS.

Save your current Ghostty window layout as a named workspace. Reopen it later with a single keystroke. That is the whole idea.

<video src="https://github.com/Franvy/gtab/releases/download/v1.4.1/gtab.mp4" autoplay loop muted playsinline></video>

---

## Quick Install

```bash
brew tap Franvy/gtab
brew install gtab
gtab init
```

Reload Ghostty config (or restart Ghostty), then press **Cmd+G** inside any Ghostty shell to open the workspace launcher.

---

## What It Does

- Save a Ghostty window as a named workspace — tabs, working directories, titles, and split panes
- Reopen any workspace later as a fresh Ghostty window with native tabs
- Launch from a small keyboard-first TUI, or directly from the shell
- New window automatically aligns to your current Ghostty window position and size
- Rename, delete, and search workspaces without leaving the TUI
- Fast in-app shortcut via `Cmd+G` set up with `gtab init`

## What It Does Not Do

- Does not persist running processes
- Does not restore shell history, editor buffers, SSH sessions, or pane state
- Does not replace tmux for detach/attach, panes, or remote workflows

---

## Typical Workflow

1. Open Ghostty, arrange your tabs the way you want.
2. Save the layout:

```bash
gtab save myproject
```

3. Press `Cmd+G` inside Ghostty (or run `gtab`) to open the TUI.
4. Type to search, press `Enter` to launch.
5. Or launch directly by name:

```bash
gtab myproject
```

---

## TUI Keys

| Key | Action |
|-----|--------|
| `/` | Search workspaces |
| `↑` / `↓` | Move selection |
| `Enter` | Launch selected workspace |
| `a` | Save current Ghostty window as new workspace |
| `n` | Rename selected workspace |
| `d` | Delete selected workspace |
| `e` | Open workspace file in `$EDITOR` |
| `g` | Edit Ghostty shortcut |
| `q` / `Esc` | Quit |

> **Double-click** a workspace row also launches it.

When you launch from the TUI, the new Ghostty window is repositioned to match your current window's position and size. This uses macOS Accessibility (System Events), so you may need to grant permission once.

---

## Core Commands

```text
gtab                     Open the TUI
gtab init                Enable the Ghostty-local Cmd+G shortcut
gtab save <name>         Save the current Ghostty window
gtab <name>              Launch a workspace directly
gtab list                List saved workspaces
gtab rename <old> <new>  Rename a workspace
gtab remove <name>       Remove a workspace
```

## Advanced Commands

```text
gtab edit <name>                       Open workspace file in $EDITOR
gtab set                               Show current settings
gtab set close_tab on|off              Auto-close the launching tab after launch
gtab set ghostty_shortcut cmd+g|off    Change or disable the Ghostty shortcut
```

Workspaces are stored as plain `.applescript` files in `~/.config/gtab/` and are human-readable and manually editable.

---

## Install

### Homebrew (recommended)

```bash
brew tap Franvy/gtab
brew install gtab
gtab init
```

Reload Ghostty config or restart Ghostty. Then press `Cmd+G` inside any Ghostty shell.

### Build from source

Requirements: macOS, [Ghostty](https://ghostty.org), Rust toolchain.

```bash
cargo install --path .
gtab init
```

### Update

```bash
brew upgrade gtab
```

---

## Uninstall

```bash
# Disable the Ghostty shortcut first
gtab set ghostty_shortcut off

# Reload Ghostty config so Cmd+G stops working

# Then remove the binary
brew uninstall gtab
# or: cargo uninstall gtab

# Optionally remove saved workspaces and config
rm -rf ~/.config/gtab
```

---

## Shortcut Model

`gtab init` writes a managed Ghostty keybind file and adds an `include` line to your Ghostty config:

```conf
keybind = cmd+g=text:gtab\x0d
```

This works only when Ghostty is focused. It is fast because it is effectively the same as typing `gtab` in the active shell.

**Tradeoff:** this shortcut is not safe inside full-screen interactive programs like Claude Code, vim, or fzf — it will type the literal text `gtab` into them. Quit those programs first, or use `gtab <name>` from a clean shell prompt.

---

## gtab vs tmux

| Topic | gtab | tmux |
|-------|------|------|
| Main idea | Save and relaunch Ghostty tab layouts | Full terminal multiplexer |
| Interface | Native Ghostty tabs | tmux sessions, windows, panes |
| State restored | Tab order, working dirs, titles, splits | Multiplexer-managed sessions and panes |
| Learning curve | Low | Higher |
| Remote / detach / attach | No | Yes |
| Best for | Ghostty-first macOS users | Users who need a full workflow layer |

---

## How It Works

`gtab save` reads the current Ghostty window through Ghostty's AppleScript API. For split-pane tabs, it also queries macOS Accessibility to capture pane positions, then reconstructs the split tree. The result is a plain `.applescript` file stored in `~/.config/gtab/`.

Launching a workspace runs that script via `osascript` to open a fresh Ghostty window and recreate the saved layout.

That is why `gtab` is lightweight: it stores layout metadata, not live terminal session state.

---

## FAQ

### Why does `Cmd+G` type text instead of running the binary directly?

Ghostty keybindings do not have an action for running external commands. The `text` action sends a string to the active shell — which is effectively the same as typing it yourself.

See: [ghostty.org/docs/config/keybind](https://ghostty.org/docs/config/keybind)

### Does gtab support split panes?

Yes, as of v1.4.1. `gtab save` captures split pane layouts. Splits are restored when launching.

---

## License

MIT
