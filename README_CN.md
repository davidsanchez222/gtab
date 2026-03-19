# gtab — Ghostty Tab Workspace Manager

[English](README.md) | **中文**

一个轻量的命令行工具，用于保存和恢复 [Ghostty](https://ghostty.org) 终端窗口布局 —— 将当前标签页（包含工作目录和自定义标题）保存为一个命名的 workspace，之后一条命令即可还原。

![gtab demo](Gtab.gif)

---

## 环境要求

- macOS
- [Ghostty](https://ghostty.org) 终端

---

## 安装

### Homebrew（推荐）

```bash
brew tap Franvy/gtab
brew install gtab
```

### 手动安装

```bash
curl -fsSL https://raw.githubusercontent.com/Franvy/gtab/main/gtab \
  -o ~/.local/bin/gtab && chmod +x ~/.local/bin/gtab
```

确保 `~/.local/bin` 在你的 `PATH` 中：

```bash
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zshrc
```

---

## 用法

```
gtab <name>          启动 workspace
gtab save <name>     将当前 Ghostty 窗口保存为 workspace
gtab list            列出所有已保存的 workspace
gtab edit <name>     编辑某个 workspace 脚本
gtab remove <name>   删除某个 workspace
```

### 快速上手

1. 在 Ghostty 中打开并布置好你的标签页（设置好目录和自定义标题）
2. 运行 `gtab save myproject` 保存当前布局
3. 下次运行 `gtab myproject` 即可还原

### 示例

```bash
# 保存当前窗口布局
gtab save work

# 列出所有 workspace
gtab list

# 启动 workspace
gtab work

# 手动编辑某个 workspace
gtab edit work

# 删除 workspace
gtab remove work
```

---

## 配置

Workspace 脚本默认存储在 `~/.config/gtab/` 目录下。

可通过环境变量 `GTAB_DIR` 自定义存储路径：

```bash
export GTAB_DIR="$HOME/Scripts/ghostty"
```

每个 workspace 是一个普通的 AppleScript 文件（`.applescript`），可以直接用 `gtab edit <name>` 查看和修改。

---

## 工作原理

`gtab save` 通过 Ghostty 的 AppleScript API 读取每个标签的工作目录和标题，生成一个能完整还原布局的 AppleScript 脚本。

---

## License

MIT
