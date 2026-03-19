# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Is

`gtab` is a single-file bash script that saves and restores Ghostty terminal window layouts using AppleScript. macOS only.

## Running and Testing

There is no build step. Run the script directly:

```bash
chmod +x gtab
./gtab --help
./gtab --version
```

There is no test suite or linter configured. Test manually by running commands against a live Ghostty instance.

## Architecture

The entire tool lives in the `gtab` file (142 lines of bash). Two documentation files exist: `README.md` (English) and `README_CN.md` (Chinese).

**Two-phase workflow:**

1. **Save** (`gtab save <name>`): Uses an inline AppleScript (via `osascript`) to read tab working directories and titles from Ghostty's AppleScript API, then generates a new `.applescript` file that can recreate the layout.

2. **Launch** (`gtab <name>`): Executes the saved `.applescript` file via `osascript` to recreate the window/tab layout in Ghostty.

**Storage:** Workspaces are stored as plain `.applescript` files in `~/.config/gtab/` (overridable via `GTAB_DIR` env var). They're human-readable and manually editable.

**Output:** Color-coded with box-drawing characters. Respects `NO_COLOR` and non-TTY environments.

## Key Implementation Details

- `set -euo pipefail` — strict error handling throughout
- AppleScript tab title stripping: titles prefixed with `⠐ ` have that prefix removed before saving
- The save command uses a temp file (`mktemp`) for the read AppleScript, then builds the restore script as a string
- `edit` subcommand opens the raw `.applescript` file in `$EDITOR` (falls back to vim)
- Version string is hardcoded at line 127: `"gtab 1.2.1"` — update this when releasing
