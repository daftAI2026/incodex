# Incodex

给本机 [Codex](https://openai.com/codex/) 桌面端加一个侧栏按钮。点一下，再开一个干净窗口：登录和基础设置跟你平时一样，旧对话不会出现。

这是非官方工具。默认只补丁一份副本，不会改你正在用的 `/Applications/ChatGPT.app`。

## 它做什么

- 装完重启 Codex，搜索按钮左边会出现帽子墨镜图标
- 点按钮，或按 `⇧⌘N`（Windows / Linux 为 `Ctrl+Shift+N`），打开第二个窗口
- 新窗口沿用你的登录、语言和 `config.toml` 里的基础设置
- 新窗口看不到平时的对话列表；原来的窗口不受影响
- 对话写在独立的 `CODEX_HOME` 里，不会改你平时的 `~/.codex` 会话库

它**不是**官方无痕模式，关掉新窗口也不会自动删这次聊过的内容。只是换了一套干净的家目录。

## 要求

- macOS
- [Bun](https://bun.sh)
- 已安装 Codex / ChatGPT 桌面端

## 安装

先只打副本，确认没问题再考虑改官方应用：

```bash
git clone https://github.com/daftAI2026/incodex.git
cd incodex
bun install
bun src/cli.ts install --clone
```

然后打开 `~/.incodex/scratch/ChatGPT.app`。

确认无误后，再补丁正在用的官方应用：

```bash
bun src/cli.ts install --live
```

`--live` 会先备份，再改 `/Applications/ChatGPT.app`。macOS 可能要求「App 管理」权限。卸掉：

```bash
bun src/cli.ts uninstall --live
```

查看状态：

```bash
bun src/cli.ts status
```

## 注意

- 需要改 Codex 的应用包才能出现按钮，官方插件体系做不到这件事
- 副本是临时签名，第一次开新窗口时，系统可能要你输入一次 Mac 登录密码（钥匙串）。这和 ChatGPT 账号密码不是一回事
- Sparkle 升级官方应用后，补丁会被冲掉，需要再装一次
- 目前只在 macOS 上试过

## 许可

MIT
