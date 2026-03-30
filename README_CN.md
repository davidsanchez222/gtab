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
gtab set             查看设置
gtab set close_tab on|off
```

### 快速上手

1. 在 Ghostty 中打开并布置好你的标签页。
2. 运行 `gtab save myproject` 保存当前布局。
3. 运行 `gtab` 打开 TUI，在其中搜索、预览或启动 workspace。

### TUI 快捷键

```text
Enter   启动当前选中的 workspace
s       保存当前 Ghostty 窗口
e       用 $EDITOR 编辑当前 workspace
d       删除当前 workspace
t       打开设置
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
同目录下的 `config` 文件目前支持 `close_tab=true|false`。

---

## 工作原理

`gtab save` 通过 Ghostty 的 AppleScript API 读取每个标签的工作目录和标题，生成一个能完整还原布局的 AppleScript 脚本。Rust 版本继续沿用这一 workspace 格式，以保持兼容性，同时在上层增加 TUI 交互体验。

---

## License

MIT
