# gtab — Ghostty Tab Workspace Manager

[English](README.md) | **中文**

一个基于 Rust 的 Ghostty workspace 管理器，提供键盘优先的 TUI，同时保留兼容的 CLI 命令。你可以把当前终端布局保存为 workspace，在 TUI 中搜索、预览并重新启动它们。

![gtab demo](Gtab.gif)

---

## 环境要求

- macOS
- [Ghostty](https://ghostty.org) 终端
- 本地 Rust 工具链（`cargo`、`rustc`）

---

## 从源码构建

```bash
cargo build --release
./target/release/gtab
```

开发时可直接运行：

```bash
cargo run --
cargo fmt
cargo test
```

说明：仓库根目录里的 `./gtab` 仍保留为迁移中的 Bash 原型；Rust 版本代码位于 `src/`。

---

## 用法

```text
gtab                 打开交互式 TUI
gtab tui             打开交互式 TUI
gtab <name>          直接启动某个 workspace
gtab save <name>     将当前 Ghostty 窗口保存为 workspace
gtab list            列出所有已保存的 workspace
gtab edit <name>     编辑某个 workspace 脚本
gtab remove <name>   删除某个 workspace
gtab shortcut        查看推荐的 macOS 快捷键 launcher
gtab set             查看设置
gtab set close_tab on|off
gtab set ghostty_shortcut cmd+g
```

### 快速上手

1. 在 Ghostty 中打开并布置好你的标签页。
2. 运行 `gtab save myproject` 保存当前布局。
3. 运行 `gtab` 打开 TUI，在其中搜索、预览或启动 workspace。
4. 如果你想稳定地绑定 `Cmd+G`，运行 `gtab shortcut`，再把生成的 launcher 绑定到 Shortcuts、Raycast 或 Hammerspoon。

### TUI 快捷键

```text
Enter   启动当前选中的 workspace
mouse   单击选中，双击启动
w/s     在 workspace 列表中上下移动
a       保存当前 Ghostty 窗口
e       用 $EDITOR 编辑当前 workspace
d       删除当前 workspace
t       打开设置
g       在设置中编辑 Ghostty 快捷键
p       切换预览面板
q       退出
```

---

## 配置

Workspace 脚本默认存储在 `~/.config/gtab/` 目录下。

可通过环境变量 `GTAB_DIR` 自定义存储路径：

```bash
export GTAB_DIR="$HOME/Scripts/ghostty"
```

每个 workspace 是一个普通的 AppleScript 文件（`.applescript`），可以直接用 `gtab edit <name>` 查看和修改。
同目录下的 `config` 文件目前支持：

- `close_tab=true|false`
- `ghostty_shortcut=cmd+g`

gtab 还会管理一个 launcher 脚本：`~/.config/gtab/launcher.sh`。这是推荐给 macOS 快捷键工具使用的入口，因为它会新开一个 Ghostty 窗口并直接运行 `gtab`。

当你打开 TUI 或设置 `ghostty_shortcut` 时，gtab 会写入一个受管理的 Ghostty include 文件 `~/.config/gtab/ghostty-shortcut.conf`，并在需要时把 `config-file` 引用加到 Ghostty 配置中。这个旧方案的实现方式是向当前聚焦的 Ghostty shell 发送 `gtab` 加回车，所以在 Claude Code、Codex、vim 或 fzf 这类界面里可能失效。

---

## 工作原理

`gtab save` 通过 Ghostty 的 AppleScript API 读取每个标签的工作目录和标题，生成一个能完整还原布局的 AppleScript 脚本。Rust 版本继续沿用这一 workspace 格式，以保持兼容性，同时在上层增加 TUI 交互体验。

---

## License

MIT
