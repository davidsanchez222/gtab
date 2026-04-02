# gtab

**English** | [中文](README_CN.md)

`gtab` is a lightweight workspace manager for Ghostty on macOS.

If you like Ghostty's native tabs and just want to save a window layout, name it, and bring it back later, this is the tool. It is intentionally small. It does not try to be a terminal multiplexer.

![gtab demo](Gtab.gif)

---

## What It Does

- Save the current Ghostty window as a named workspace
- Restore that workspace later as a fresh Ghostty window with native tabs
- Keep the saved tab order, working directory, and tab title
- Let you search and launch workspaces from a small keyboard-first TUI
- Add a fast Ghostty-local `Cmd+G` launcher with `gtab init`

## What It Does Not Do

- It does not persist running processes
- It does not restore shell history, editor buffers, SSH sessions, or pane state
- It does not replace tmux for detach/attach, panes, or remote workflows

## Who It Is For

- macOS developers who use Ghostty as their main terminal
- people who prefer native Ghostty tabs over tmux panes
- people who want lightweight workspace recall without adding a heavy workflow layer

## Who It Is Not For

- people who already live in tmux
- people who need a real multiplexer with panes, sessions, and remote persistence
- people who want a shortcut that is always safe inside Claude Code, Codex, vim, or fzf

---

## Install

### Homebrew

Recommended for normal users:

```bash
brew tap Franvy/gtab
brew install gtab
gtab init
```

Then reload Ghostty config or restart Ghostty.

### Build from source

Requirements:

- macOS
- [Ghostty](https://ghostty.org)
- Rust toolchain

Install:

```bash
cargo install --path .
gtab init
```

Then reload Ghostty config or restart Ghostty.

## Uninstall
Disable the Ghostty-local shortcut:

```bash
gtab set ghostty_shortcut off
```

Remove the installed binary:

```bash
brew uninstall gtab
```

or:

```bash
cargo uninstall gtab
```

If you also want to remove saved workspaces and local config, delete:

```bash
rm -rf ~/.config/gtab
```

---

## Typical Workflow

1. Open Ghostty and arrange the tabs you want.
2. Save that layout:

```bash
gtab save myproject
```

3. Later, open the launcher with `Cmd+G` inside Ghostty or just run:

```bash
gtab
```

4. Search for the workspace and press Enter to relaunch it.
5. If you already know the name, launch it directly:

```bash
gtab myproject
```

That is the whole model: save a Ghostty tab layout, then reopen it quickly later.

## TUI Basics

Inside the TUI, the common keys are:

- `/` to start search
- `Enter` to launch the selected workspace
- `a` to save the current Ghostty window
- `d` to remove the selected workspace
- `q` to quit

---

## Core Commands

```text
gtab                 Open the TUI
gtab init            Enable the default Ghostty-local Cmd+G
gtab save <name>     Save the current Ghostty window
gtab <name>          Launch a workspace directly
gtab list            List saved workspaces
gtab remove <name>   Remove a workspace
```

## Advanced Commands

```text
gtab edit <name>
gtab set
gtab set close_tab on|off
gtab set ghostty_shortcut cmd+g|off
```

Saved workspaces are plain AppleScript files under `~/.config/gtab/`, so you can inspect or edit them if needed.

---

## Shortcut Model

`gtab init` enables the default fast path:

```conf
keybind = cmd+g=text:gtab\x0d
```

This works only when Ghostty is focused, and it feels fast because it is effectively the same as typing `gtab` in the current shell.

Tradeoff:

- it is fast and same-tab
- it is not safe inside interactive full-screen tools like Claude Code, Codex, vim, or fzf

## gtab vs tmux

`gtab` and tmux solve different problems.

| Topic | gtab | tmux |
| --- | --- | --- |
| Main idea | Save and relaunch Ghostty tab layouts | Full terminal multiplexer |
| Interface | Native Ghostty tabs | tmux sessions, windows, panes |
| State restored | Tab order, working dirs, titles | Multiplexer-managed sessions and panes |
| Learning curve | Low | Higher |
| Remote / detach / attach | No | Yes |
| Best for | Ghostty-first macOS users who want something light | Users who want a powerful terminal workflow layer |

If you already know you want panes, persistent sessions, remote attach, or server-side terminal workflows, use tmux.

If you are a Ghostty user on macOS and your need is simpler, "save this set of tabs and bring it back later," `gtab` is the lighter tool.

---

## FAQ

### Why does the Ghostty shortcut send `gtab` as text instead of executing the `gtab` binary directly?

Because Ghostty keybindings currently do not expose an action for "run an external command directly." The official keybinding actions are built-in Ghostty actions or terminal-encoding actions like `text`, `csi`, and `esc`.

Docs:

- https://ghostty.org/docs/config/keybind
- https://ghostty.org/docs/config/keybind/reference

So the fast Ghostty-local launcher uses:

```conf
keybind = cmd+g=text:gtab\x0d
```

That is why it feels like manually entering `gtab`: it is doing almost exactly that.

---

## How It Works

`gtab save` reads the current Ghostty window through Ghostty's AppleScript API and writes a plain `.applescript` workspace file. Launching that workspace later opens a fresh Ghostty window and recreates the saved tabs with their working directories and titles.

That is why `gtab` is lightweight: it stores layout metadata, not live terminal session state.

---

## License

MIT
