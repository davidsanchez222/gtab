# gtab — Ghostty Tab Workspace Manager

[English](README.md) | **中文**

一个基于 Rust 的 Ghostty workspace 管理器，提供键盘优先的 TUI，同时保留兼容的 CLI 命令。你可以把当前终端布局保存为 workspace，在对话框风格的 TUI 中搜索、查看保存的 tab 内容，并重新启动它们。

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
gtab hotkey status   查看内建全局快捷键 helper 状态
gtab hotkey doctor   查看全局快捷键 helper 诊断信息
gtab set             查看设置
gtab set close_tab on|off
gtab set global_shortcut cmd+g
gtab set ghostty_shortcut off|cmd+shift+g
```

### 快速上手

1. 在 Ghostty 中打开并布置好你的标签页。
2. 运行 `gtab save myproject` 保存当前布局。
3. 运行 `gtab` 打开 TUI，在其中搜索、查看或启动 workspace。
4. `brew install gtab` 之后，内建的热键 helper 会负责默认的 `Cmd+G`。
5. 如果 `Cmd+G` 没有打开 gtab，运行 `gtab hotkey doctor`。

### TUI 快捷键

```text
Enter   启动当前选中的 workspace
/       开始实时搜索
j/k     在 workspace 列表中上下移动
↑/↓     在 workspace 列表中上下移动
PgUp/Dn 按整屏跳转
Home    跳到顶部
End/G   跳到底部
a       保存当前 Ghostty 窗口
e       用 $EDITOR 编辑当前 workspace
d       删除当前 workspace
g       在快速设置区编辑全局快捷键
r       重新加载 workspace 列表
t       打开设置
?       打开帮助
mouse   单击选中，双击启动，点击 shortcut 可编辑
q       退出
```

现在的 TUI 采用固定的对话框布局：左侧是带方括号的 workspace 标签列表，中间按保存顺序横向显示当前选中 workspace 的 tab 标签，右侧显示全局快捷键和 helper 状态。

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
- `global_shortcut=cmd+g`
- `ghostty_shortcut=off|cmd+shift+g`

gtab 还会安装一个用户级 LaunchAgent 和配套 helper 二进制 `gtab-hotkey`，用于注册 macOS 全局快捷键。默认的 `global_shortcut` 是 `cmd+g`。

当你通过内建全局快捷键打开 gtab 时，它会临时切换到最近使用过的 ASCII 输入源，这样 `q`、`g`、`t` 这类单键操作不会被中文输入法拦住；退出 TUI 或启动 workspace 之后，会再恢复到之前的输入法。

旧的 Ghostty 文本注入快捷键仍然可以通过 `ghostty_shortcut` 使用，但现在只作为兼容模式保留。除非你在调试，否则应保持为 `off`，因为它本质上只是向当前聚焦的 shell 发送 `gtab` 加回车，在 Claude Code、Codex、vim 或 fzf 这类界面里可能失效。

---

## 工作原理

`gtab save` 通过 Ghostty 的 AppleScript API 读取每个标签的工作目录和标题，生成一个能完整还原布局的 AppleScript 脚本。Rust 版本继续沿用这一 workspace 格式，以保持兼容性，同时在上层增加 TUI 交互体验。

---

## License

MIT
