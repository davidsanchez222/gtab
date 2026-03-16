# gtab — Ghostty Tab Workspace Manager

**English** | [中文](README_CN.md)

A lightweight CLI tool to save and restore [Ghostty](https://ghostty.org) terminal window layouts — capture your current tabs (with working directories and custom titles) into a named workspace, then reopen them anytime with a single command.

---

## Requirements

- macOS
- [Ghostty](https://ghostty.org) terminal

---

## Installation

### Homebrew (recommended)

```bash
brew tap Franvy/gtab
brew install gtab
```

### Manual

```bash
curl -fsSL https://raw.githubusercontent.com/Franvy/gtab/main/gtab \
  -o ~/.local/bin/gtab && chmod +x ~/.local/bin/gtab
```

Make sure `~/.local/bin` is in your `PATH`:

```bash
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zshrc
```

---

## Usage

```
gtab <name>          Launch a workspace
gtab save <name>     Save current Ghostty window as a workspace
gtab list            List all saved workspaces
gtab edit <name>     Edit a workspace script
gtab remove <name>   Remove a workspace
```

### Quick start

1. Open Ghostty and set up your tabs (directories + custom titles)
2. Run `gtab save myproject` to capture the layout
3. Next time, run `gtab myproject` to restore it

### Example

```bash
# Save current window layout
gtab save work

# List saved workspaces
gtab list

# Launch a workspace
gtab work

# Edit a workspace manually
gtab edit work

# Remove a workspace
gtab remove work
```

---

## Configuration

Workspace scripts are stored in `~/.config/gtab/` by default.

Override the directory with the `GTAB_DIR` environment variable:

```bash
export GTAB_DIR="$HOME/Scripts/ghostty"
```

Each workspace is stored as a plain AppleScript file (`.applescript`) that you can inspect and edit freely with `gtab edit <name>`.

---

## How it works

`gtab save` uses Ghostty's AppleScript API to read each tab's working directory and title, then generates an AppleScript that recreates the exact layout when run.
