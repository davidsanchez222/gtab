# gtab

[English](README.md) | **中文**

`gtab` 是一个面向 macOS Ghostty 用户的轻量 workspace 管理工具。

如果你喜欢 Ghostty 的原生 tab，只是想把当前窗口布局保存下来、起个名字、以后再一键拉起，那么它就是为这个场景做的。它刻意保持简单，不打算变成终端复用器。

![gtab demo](Gtab.gif)

---

## 它能做什么

- 把当前 Ghostty 窗口保存成一个命名 workspace
- 以后把这个 workspace 重新拉起为一个新的 Ghostty 窗口，并恢复原生 tabs
- 保留 tab 顺序、工作目录和 tab 标题
- 用一个小巧的键盘优先 TUI 搜索和启动 workspace
- 通过 `gtab init` 配置一个 Ghostty 内的快速 `Cmd+G`

## 它不能做什么

- 不会持久化正在运行的进程
- 不会恢复 shell 历史、编辑器 buffer、SSH 会话或 pane 状态
- 不会取代 tmux 的 detach/attach、panes、remote workflow

## 适合谁

- 主要在 macOS 上使用 Ghostty 的开发者
- 喜欢 Ghostty 原生 tab，而不是 tmux pane 的用户
- 只想轻量管理 workspace，不想引入更重的终端工作流层

## 不适合谁

- 已经长期使用 tmux 的用户
- 需要真正的 multiplexer、panes、sessions、remote persistence 的用户
- 希望快捷键在 Claude Code、Codex、vim、fzf 这类界面里也始终安全的用户

---

## 安装

### Homebrew

普通用户推荐这样安装：

```bash
brew tap Franvy/gtab
brew install gtab
gtab init
```

然后重新加载 Ghostty 配置，或者直接重启 Ghostty。

### 从源码安装

环境要求：

- macOS
- [Ghostty](https://ghostty.org)
- Rust 工具链

安装：

```bash
cargo install --path .
gtab init
```

然后重新加载 Ghostty 配置，或者直接重启 Ghostty。

## 卸载
关闭 Ghostty 内快捷键：

```bash
gtab set ghostty_shortcut off
```

然后重新加载 Ghostty 配置，或者直接重启 Ghostty，这样 `Cmd+G` 才不会继续向当前 shell 输入 `gtab`。

删除已安装的二进制：

```bash
brew uninstall gtab
```

或者：

```bash
cargo uninstall gtab
```

等 Ghostty 已经重新加载、且不再引用 gtab 托管的快捷键配置后，如果你还想一并删除保存的 workspace 和本地配置，再删除：

```bash
rm -rf ~/.config/gtab
```

---

## 典型工作流

1. 在 Ghostty 里把你想要的 tabs 排好。
2. 保存这个布局：

```bash
gtab save myproject
```

3. 以后在 Ghostty 里按 `Cmd+G`，或者直接运行：

```bash
gtab
```

4. 在 TUI 里搜索对应 workspace，按 Enter 启动。
5. 如果你已经知道名字，也可以直接：

```bash
gtab myproject
```

整个模型其实就这么简单：把一个 Ghostty tab 布局保存下来，之后再快速打开。

## TUI 基本操作

在 TUI 里，最常用的是这几个键：

- `/` 开始搜索
- `Enter` 启动当前选中的 workspace
- `a` 保存当前 Ghostty 窗口
- `d` 删除当前选中的 workspace
- `q` 退出

---

## 核心命令

```text
gtab                 打开 TUI
gtab init            启用默认的 Ghostty 内 Cmd+G
gtab save <name>     保存当前 Ghostty 窗口
gtab <name>          直接启动某个 workspace
gtab list            列出已保存的 workspace
gtab remove <name>   删除某个 workspace
```

## 高级命令

```text
gtab edit <name>
gtab set
gtab set close_tab on|off
gtab set ghostty_shortcut cmd+g|off
```

保存下来的 workspace 本质上就是 `~/.config/gtab/` 下的普通 AppleScript 文件，所以你也可以按需直接查看或修改。

---

## 快捷键模型

`gtab init` 会启用默认的快速路径：

```conf
keybind = cmd+g=text:gtab\x0d
```

它只会在 Ghostty 处于前台时生效，而且之所以快，是因为它几乎就等价于在当前 shell 里手动输入 `gtab`。

它的取舍也很明确：

- 优点是快，而且就在当前页打开
- 缺点是它不适合 Claude Code、Codex、vim、fzf 这类交互式全屏程序

## gtab 和 tmux 的区别

`gtab` 和 tmux 解决的不是同一类问题。

| 维度 | gtab | tmux |
| --- | --- | --- |
| 核心目标 | 保存并重新拉起 Ghostty tab 布局 | 完整的终端 multiplexer |
| 交互界面 | Ghostty 原生 tabs | tmux sessions、windows、panes |
| 恢复内容 | tab 顺序、工作目录、标题 | multiplexer 管理下的 sessions 和 panes |
| 学习成本 | 低 | 更高 |
| 远程 / detach / attach | 不支持 | 支持 |
| 更适合谁 | Ghostty-first 的 macOS 用户，需求简单轻量 | 需要强大终端工作流层的用户 |

如果你已经明确需要 panes、persistent sessions、remote attach，直接用 tmux。

如果你是 macOS 上的 Ghostty 用户，而你的需求只是“把这组 tabs 保存下来，之后再拉起来”，那 `gtab` 会更轻、更直接。

---

## FAQ

### 为什么 Ghostty 快捷键是发送 `gtab` 文本，而不是直接执行 `gtab` 二进制？

因为 Ghostty 当前的 keybind action 并没有“直接执行外部命令”的能力。官方文档里支持的是 Ghostty 内建动作，或者像 `text`、`csi`、`esc` 这种向终端发送内容的动作。

文档：

- https://ghostty.org/docs/config/keybind
- https://ghostty.org/docs/config/keybind/reference

所以 Ghostty 内这条最快的快捷键路径只能是：

```conf
keybind = cmd+g=text:gtab\x0d
```

也正因为如此，它的手感才会和你手动输入 `gtab` 很接近。

---

## 工作原理

`gtab save` 会通过 Ghostty 的 AppleScript API 读取当前窗口，然后生成一个普通的 `.applescript` workspace 文件。之后再次启动这个 workspace 时，`gtab` 会新开一个 Ghostty 窗口，并按保存时的 working directory 和 tab title 重新创建这些 tabs。

这也是它足够轻量的原因：它保存的是布局信息，不是活着的终端 session 状态。

---

## License

MIT
