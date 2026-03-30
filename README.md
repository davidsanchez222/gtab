# gtab — Ghostty Tab Workspace Manager

**English** | [中文](README_CN.md)

A Rust-powered Ghostty workspace manager with a keyboard-first TUI and compatible CLI commands. Save the current terminal layout as a workspace, search and preview saved workspaces, then relaunch them with one key or one command.

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
gtab set             Show settings
gtab set close_tab on|off
```

### Quick start

1. Open Ghostty and set up your tabs.
2. Run `gtab save myproject` to capture the layout.
3. Run `gtab` to open the TUI and search, preview, or launch saved workspaces.

### TUI shortcuts

```text
Enter   launch selected workspace
s       save the current Ghostty window
e       edit the selected workspace in $EDITOR
d       delete the selected workspace
t       open settings
p       toggle the preview pane
q       quit
```

---

## Configuration

Workspace scripts are stored in `~/.config/gtab/` by default.

Override the directory with the `GTAB_DIR` environment variable:

```bash
export GTAB_DIR="$HOME/Scripts/ghostty"
```

Each workspace is stored as a plain AppleScript file (`.applescript`) that you can inspect and edit freely with `gtab edit <name>`.
The `config` file in the same directory currently supports `close_tab=true|false`.

---

## How it works

`gtab save` uses Ghostty's AppleScript API to read each tab's working directory and title, then generates an AppleScript that recreates the layout. The Rust app keeps the same workspace format for compatibility while adding a TUI layer on top.

---

## License

MIT
