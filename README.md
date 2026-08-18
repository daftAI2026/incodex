# Incodex

给本机 [Codex](https://openai.com/codex/) 桌面端加一个侧栏按钮。点一下，再开一扇无痕窗口：登录和设置跟平时一样，不会带入之前的对话。关掉后，这次聊天不会留下记录。

这是非官方工具。

## 它做什么

- 装完重启 Codex，搜索左边会出现帽子墨镜按钮
- 点按钮，或按 `⇧⌘N`（Windows / Linux 为 `Ctrl+Shift+N`），打开无痕窗口
- 无痕窗口沿用你的登录、语言和 `config.toml` 里的基础设置
- 看不到平时的对话列表；原来的窗口不受影响
- 对话写在独立目录里，不会改你平时的 `~/.codex` 会话库
- 关掉这扇窗，或再点一次按钮，会清掉这次的聊天；登录和设置会留着
- 按钮和说明会跟主窗口语言走

## 要求

- macOS
- [Bun](https://bun.sh)
- 已安装 Codex / ChatGPT 桌面端

## 安装

先打一份副本试：

```bash
git clone https://github.com/daftAI2026/incodex.git
cd incodex
bun install
bun src/cli.ts install --clone
```

然后打开用户目录下的副本：`~/.incodex/scratch/ChatGPT.app`。

确认没问题后，再打进正在用的官方应用：

```bash
bun src/cli.ts install --live
```

`--live` 会在用户目录里打好补丁，再整份替换 `/Applications/ChatGPT.app`。卸掉：

```bash
bun src/cli.ts uninstall --live
```

查看状态：

```bash
bun src/cli.ts status
```

## 注意

- 官方插件加不了这个按钮，必须改应用包
- 改包之后没法继续用一份有效的 OpenAI 签名。第一次开无痕窗，系统可能要你输入 **Mac 登录密码**（钥匙串里的 Codex Storage Key）。选 **始终允许**。这不是 ChatGPT 账号密码
- 官方应用升级后，补丁会被冲掉，需要再跑一次 `install --live`
- 目前只在 macOS 上试过

## 许可

MIT
