# gws — Ghostty Workspace Manager

**English** | [中文](#中文)

A lightweight CLI tool to save and restore [Ghostty](https://ghostty.org) terminal window layouts — capture your current tabs (with working directories and custom titles) into a named workspace, then reopen them anytime with a single command.

---

## Requirements

- macOS
- [Ghostty](https://ghostty.org) terminal

---

## Installation

### Homebrew (recommended)

```bash
brew install Franvy/gws/gws
```

### Manual

```bash
curl -fsSL https://raw.githubusercontent.com/Franvy/gws/main/gws \
  -o ~/.local/bin/gws && chmod +x ~/.local/bin/gws
```

Make sure `~/.local/bin` is in your `PATH`:

```bash
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zshrc
```

---

## Usage

```
gws <name>          Launch a workspace
gws save <name>     Save current Ghostty window as a workspace
gws list            List all saved workspaces
gws edit <name>     Edit a workspace script
gws remove <name>   Remove a workspace
```

### Quick start

1. Open Ghostty and set up your tabs (directories + custom titles)
2. Run `gws save myproject` to capture the layout
3. Next time, run `gws myproject` to restore it

### Example

```bash
# Save current window layout
gws save work

# List saved workspaces
gws list

# Launch a workspace
gws work

# Edit a workspace manually
gws edit work

# Remove a workspace
gws remove work
```

---

## Configuration

Workspace scripts are stored in `~/.config/gws/` by default.

Override the directory with the `GWS_DIR` environment variable:

```bash
export GWS_DIR="$HOME/Scripts/ghostty"
```

Each workspace is stored as a plain AppleScript file (`.applescript`) that you can inspect and edit freely with `gws edit <name>`.

---

## How it works

`gws save` uses Ghostty's AppleScript API to read each tab's working directory and title, then generates an AppleScript that recreates the exact layout when run.

---

---

# 中文

一个轻量的命令行工具，用于保存和恢复 [Ghostty](https://ghostty.org) 终端窗口布局 —— 将当前标签页（包含工作目录和自定义标题）保存为一个命名的 workspace，之后一条命令即可还原。

---

## 环境要求

- macOS
- [Ghostty](https://ghostty.org) 终端

---

## 安装

### Homebrew（推荐）

```bash
brew install Franvy/gws/gws
```

### 手动安装

```bash
curl -fsSL https://raw.githubusercontent.com/Franvy/gws/main/gws \
  -o ~/.local/bin/gws && chmod +x ~/.local/bin/gws
```

确保 `~/.local/bin` 在你的 `PATH` 中：

```bash
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zshrc
```

---

## 用法

```
gws <name>          启动 workspace
gws save <name>     将当前 Ghostty 窗口保存为 workspace
gws list            列出所有已保存的 workspace
gws edit <name>     编辑某个 workspace 脚本
gws remove <name>   删除某个 workspace
```

### 快速上手

1. 在 Ghostty 中打开并布置好你的标签页（设置好目录和自定义标题）
2. 运行 `gws save myproject` 保存当前布局
3. 下次运行 `gws myproject` 即可还原

### 示例

```bash
# 保存当前窗口布局
gws save work

# 列出所有 workspace
gws list

# 启动 workspace
gws work

# 手动编辑某个 workspace
gws edit work

# 删除 workspace
gws remove work
```

---

## 配置

Workspace 脚本默认存储在 `~/.config/gws/` 目录下。

可通过环境变量 `GWS_DIR` 自定义存储路径：

```bash
export GWS_DIR="$HOME/Scripts/ghostty"
```

每个 workspace 是一个普通的 AppleScript 文件（`.applescript`），可以直接用 `gws edit <name>` 查看和修改。

---

## 工作原理

`gws save` 通过 Ghostty 的 AppleScript API 读取每个标签的工作目录和标题，生成一个能完整还原布局的 AppleScript 脚本。
