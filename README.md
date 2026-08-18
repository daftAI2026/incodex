# Incodex

给本机 [Codex](https://openai.com/codex/) 桌面端加一个侧栏按钮。点一下，再开一扇无痕窗口：登录和设置跟平时一样，看不到以前的对话，这次的聊天也不会进平时的列表。正常关掉后，这次的临时数据会清掉。

这是非官方工具。

## 它做什么

- 装完重启 Codex，搜索左边会出现帽子墨镜按钮
- 点按钮，或按 `Shift+Command+N`（Windows / Linux 为 `Ctrl+Shift+N`），打开无痕窗口
- 无痕窗口沿用你的登录、语言和 `config.toml` 里的基础设置
- 看不到平时的对话列表；原来的窗口不受影响
- 对话写在独立目录里，不会出现在平时的会话列表里
- 关掉这扇窗，或再点一次按钮，会在正常退出后清掉这次的临时会话（含独立 Chromium 档案）；登录和设置会留着。这还不是「本机完全不留记录」的取证结论
- 按钮和说明会跟主窗口语言走

## 要求

- macOS
- [Bun](https://bun.sh) 1.3.14（见仓库根目录 `.bun-version`）
- 已安装 Codex / ChatGPT 桌面端

## 安装

当前还是开发中版本。请用 [Releases](https://github.com/daftAI2026/incodex/releases) 里带 checksum 的 tag；没有 tag 时再临时用一个明确的 commit，不要直接追着移动中的 `main` 打正式应用。

先打一份副本试：

```bash
git clone https://github.com/daftAI2026/incodex.git
cd incodex
bun install --frozen-lockfile
bun src/cli.ts install --clone
```

然后打开用户目录下的副本：`~/.incodex/scratch/ChatGPT.app`。

确认没问题后，再打进正在用的官方应用。`--live` 仍是实验功能，必须先看完计划再确认：

```bash
bun src/cli.ts install --live --confirm-live
```

`--live` 会在用户目录里打好补丁，再整份替换 `/Applications/ChatGPT.app`。卸掉：

```bash
bun src/cli.ts uninstall --live
```

查看状态：

```bash
bun src/cli.ts status
bun src/cli.ts doctor
```

## 注意

- 官方插件加不了这个按钮，必须改应用包
- 改包之后没法继续用一份有效的 OpenAI 签名。第一次开无痕窗，系统可能要你输入 **Mac 登录密码**（钥匙串里的 Codex Storage Key）。选 **始终允许**。这不是 ChatGPT 账号密码
- 因此官方 **智能快照**（拍照 / 截屏附件，英文 Appshot）会不可用，提示「无法附加智能快照」。这不是相机权限没开。Computer Use 一般还能用。`uninstall` 卸掉补丁、回到未改的官方应用后，快照会恢复
- 官方应用升级后，补丁会被冲掉，需要再跑一次 `install --live`。这次会给**当前这份官方应用**打补丁，不会把旧版本换回去，也不会删掉刚升上来的新版本
- 如果官方已经升级成新版本，`uninstall --live` 不会用旧备份覆盖它，会提示当前应用已经不是 Incodex 安装态
- 每次安装的原始包按应用路径隔离，写在 `~/.incodex/installations/` 里。副本、正式应用和自定义 `--app` 不会共用一份备份。卸载前会核对当前包的 install ID、build 和完整 ASAR hash，对不上就拒绝恢复
- 目前只在 macOS 上试过

## 许可

MIT。完整文本见 [LICENSE](./LICENSE)。

- 仓库里的 `assets/hat-glasses.svg` 和 `src/runtime/incognito-copy.ts` 翻译文案是 Incodex 原作，同样按 MIT 授权
- Codex / ChatGPT 的名称、图标和官方应用本身属于 OpenAI，本仓库不授予那些材料的许可
