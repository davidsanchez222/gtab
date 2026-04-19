# gtab

[English](README.md) | **中文**

`gtab` 是一个面向 macOS [Ghostty](https://ghostty.org) 用户的轻量 workspace 管理工具。

把当前 Ghostty 窗口布局保存成一个有名字的 workspace，以后一键拉起。就这么简单。

<video src="https://github.com/user-attachments/assets/f448994b-5c92-4215-aca7-624b62b50131" autoplay loop muted playsinline></video>

---

## 快速安装

```bash
brew tap Franvy/gtab
brew install gtab
gtab init
```

重新加载 Ghostty 配置（或直接重启 Ghostty），然后在任意 Ghostty shell 里按 **Cmd+G** 就能打开 workspace 启动器。

---

## 它能做什么

- 把 Ghostty 窗口保存成命名 workspace——含 tabs、工作目录、标题，以及分屏布局
- 随时把任意 workspace 重新拉起为一个新的 Ghostty 窗口，恢复原生 tabs
- 保存命名目录项，并把当前 split 快速切换成该目录里的新 shell
- 通过小巧的键盘优先 TUI 启动，或者直接在 shell 里运行
- 新窗口自动对齐当前 Ghostty 窗口的位置和大小
- 在 TUI 里直接重命名、删除、搜索 workspace，不用离开界面
- 用 `gtab init` 配置 Ghostty 内的 `Cmd+G` 快捷键

## 它不能做什么

- 不会持久化正在运行的进程
- 不会恢复 shell 历史、编辑器 buffer、SSH 会话或 pane 状态
- 不会取代 tmux 的 detach/attach、panes、remote workflow

---

## 典型工作流

1. 在 Ghostty 里把你想要的 tabs 排好。
2. 保存这个布局：

```bash
gtab save myproject
```

3. 在 Ghostty 里按 `Cmd+G`（或运行 `gtab`）打开 TUI。
4. 输入关键词搜索，按 `Enter` 启动。
5. 如果你已经知道名字，也可以直接：

```bash
gtab myproject
```

---

## TUI 快捷键

| 按键 | 操作 |
|------|------|
| `f` | 在 Workspace 空间 / 目录空间之间切换 |
| `/` | 搜索当前空间 |
| `↑` / `↓` | 移动选中项 |
| `Enter` | Workspace：启动；目录空间：用该目录里的新 shell 替换当前 split |
| `a` | Workspace：保存当前 Ghostty 窗口；目录空间：保存当前 shell 目录 |
| `n` | 重命名当前空间的选中项 |
| `d` | 删除当前空间的选中项 |
| `e` | 仅 Workspace：用 `$EDITOR` 打开 workspace 文件 |
| `g` | 仅 Workspace：编辑 Ghostty 快捷键 |
| `q` / `Esc` | 退出 |

> **双击** 会执行当前空间里的主操作（启动/替换）。

从 TUI 启动时，新的 Ghostty 窗口会自动对齐当前窗口的位置和大小。这依赖 macOS 辅助功能（System Events），首次使用可能需要授权。

---

## 目录空间

目录空间只保存目录路径，不会重建 Ghostty 的 tabs 或窗口。

- 在 TUI 里按 `f` 切到目录空间。
- 保存的目录项会按窗口宽度自适应为多列网格，放不下时自动换行。
- 按 `a` 把当前 shell 目录保存成一个命名目录项。
- 按 `Enter`（或双击）把当前 split 替换成一个从该目录启动的新 shell。

默认情况下，gtab 会把当前 split 直接替换成一个从目标目录启动的新 shell。也就是说，用户只要升级 gtab 就能直接使用目录空间，不需要额外 shell 配置。

这个动作会替换当前 split 里的 shell 进程，所以这个 split 里原本的临时 shell 状态会丢失。

如果你想保留 shell 包装兜底（例如在非 Ghostty 场景下），可以用：

```bash
gtab() {
  if [ "$#" -eq 0 ]; then
    local cmd
    cmd="$(command gtab --shell-cd)" || return $?
    if [ -n "$cmd" ]; then
      eval "$cmd"
    fi
    return 0
  fi

  command gtab "$@"
}
```

`gtab --shell-cd` 仅用于这个包装流程；其他命令和 workspace 启动行为不变。

---

## 核心命令

```text
gtab                     打开 TUI
gtab init                启用 Ghostty 内的 Cmd+G 快捷键
gtab save <name>         保存当前 Ghostty 窗口
gtab <name>              直接启动某个 workspace
gtab list                列出已保存的 workspace
gtab rename <old> <new>  重命名 workspace
gtab remove <name>       删除 workspace
```

## 高级命令

```text
gtab edit <name>                       用 $EDITOR 打开 workspace 文件
gtab set                               查看当前设置
gtab set close_tab on|off              启动后自动关闭发起 tab
gtab set ghostty_shortcut cmd+g|off    修改或禁用 Ghostty 快捷键
```

保存下来的 workspace 是 `~/.config/gtab/` 下的普通 `.applescript` 文件。
目录项保存在 `~/.config/gtab/dirs/` 下的普通 `.path` 文件。

---

## 安装

### Homebrew（推荐）

```bash
brew tap Franvy/gtab
brew install gtab
gtab init
```

重新加载 Ghostty 配置或重启 Ghostty，然后在任意 Ghostty shell 里按 `Cmd+G`。

### 从源码安装

环境要求：macOS、[Ghostty](https://ghostty.org)、Rust 工具链。

```bash
cargo install --path .
gtab init
```

### 更新

```bash
brew upgrade gtab
```

---

## 卸载

```bash
# 先关闭 Ghostty 快捷键
gtab set ghostty_shortcut off

# 重新加载 Ghostty 配置，让 Cmd+G 停止生效

# 然后删除二进制
brew uninstall gtab
# 或者：cargo uninstall gtab

# 可选：删除已保存的 workspace 和配置
rm -rf ~/.config/gtab
```

---

## 快捷键模型

`gtab init` 会写入一个托管的 Ghostty keybind 文件，并在你的 Ghostty 配置里加上 `include` 引用：

```conf
keybind = cmd+g=text:gtab\x0d
```

它只会在 Ghostty 处于前台时生效，速度很快，因为它本质上就等于在当前 shell 里手动输入 `gtab`。

**注意：** 这个快捷键在 Claude Code、vim、fzf 等交互式全屏程序里不安全——它会把 `gtab` 这几个字母打到程序里。在干净的 shell 提示符下使用，或者直接运行 `gtab <name>`。

如果你的 Ghostty 配置由 Nix/Home Manager 或其他只读方案管理，`gtab init` 仍然会写出 `~/.config/gtab/ghostty-shortcut.conf`，然后提示你把下面这行手动加到 Ghostty 的配置源里：

```conf
config-file = "/Users/you/.config/gtab/ghostty-shortcut.conf"
```

之后重新应用配置，再重载或重启 Ghostty 即可。

---

## gtab 和 tmux 的区别

| 维度 | gtab | tmux |
|------|------|------|
| 核心目标 | 保存并重新拉起 Ghostty tab 布局 | 完整的终端 multiplexer |
| 交互界面 | Ghostty 原生 tabs | tmux sessions、windows、panes |
| 恢复内容 | tab 顺序、工作目录、标题、分屏 | multiplexer 管理的 sessions 和 panes |
| 学习成本 | 低 | 更高 |
| 远程 / detach / attach | 不支持 | 支持 |
| 更适合谁 | Ghostty-first 的 macOS 用户 | 需要完整终端工作流层的用户 |

---

## 工作原理

`gtab save` 通过 Ghostty 的 AppleScript API 读取当前窗口。对于含分屏的 tab，还会通过 macOS Accessibility 获取各个 pane 的屏幕位置，然后重建分屏树结构。结果以普通 `.applescript` 文件的形式存放在 `~/.config/gtab/`。

启动 workspace 时，通过 `osascript` 执行这个脚本，打开一个新的 Ghostty 窗口并恢复保存时的布局。

这也是它足够轻量的原因：保存的是布局信息，不是活着的终端 session 状态。

---

## FAQ

### 为什么 `Cmd+G` 是发送文本，而不是直接执行二进制？

Ghostty 的 keybind action 没有"直接执行外部命令"的能力。`text` action 会把字符串发送到当前 shell——效果几乎等同于你自己手动输入。

参考：[ghostty.org/docs/config/keybind](https://ghostty.org/docs/config/keybind)

### 为什么 gtab 不直接修改我的 Nix/Home Manager 配置？

Nix/Home Manager 一般不是让你直接改 Ghostty 最终生成出来的配置文件，而是改声明式配置源。`gtab` 可以稳定地生成自己的托管 include 文件，但无法可靠地自动修改每个用户各不相同的 `home.nix`、flake 模块或仓库结构，否则很容易把配置改坏。所以在这类环境下，`gtab init` 会先写好 include 文件，再明确告诉你该把哪一行 `config-file = ...` 加到配置源里。

### gtab 支持分屏吗？

支持，从 v1.4.1 开始。`gtab save` 会捕捉分屏布局，启动时也会还原分屏。

---

## License

MIT
