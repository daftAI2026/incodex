# Incodex

给本机 [Codex](https://openai.com/codex/) 桌面端加一个侧栏按钮。点一下，再开一扇无痕窗口：登录和设置跟平时一样，看不到以前的对话，这次的聊天也不会进平时的列表。正常关掉后，这次的临时数据会清掉。

这是非官方工具。

短命令是 `inc`，和 `incodex` 是同一个程序。

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

```bash
git clone https://github.com/daftAI2026/incodex.git
cd incodex
bun install --frozen-lockfile
bun link
```

`bun link` 之后 PATH 上会有 `incodex` 和 `inc`。

终端里直接打 `inc` 会出菜单。打补丁进正在用的官方 Codex：

```bash
incodex install
```

没有终端（脚本 / Agent）时要加 `--yes`。先看计划、不改包：

```bash
incodex install --dry-run
incodex install --yes
```

开发可以先打一份副本：

```bash
incodex install --clone
```

然后打开 `~/.incodex/scratch/ChatGPT.app`。

卸掉官方包上的补丁：

```bash
incodex uninstall
```

查看状态：

```bash
incodex status
incodex doctor
```

只更新 Incodex 自己的按钮 / 文案 / 清理逻辑，不必再改官方应用、也不必重签：

```bash
incodex runtime
```

这只写入 `~/.incodex/runtime/`。官方 Codex 升级冲掉补丁后，仍要再跑一次 `incodex install`（只打**当前这份**官方包）。

`bun install` / `bun link` 只把命令装到你的 PATH，不会改 `/Applications/ChatGPT.app`。改官方包的是随后那条 `incodex install`。

## 注意

- 官方插件加不了这个按钮，必须改应用包
- 改包之后没法继续用一份有效的 OpenAI 签名。第一次开无痕窗，系统可能要你输入 **Mac 登录密码**（钥匙串里的 Codex Storage Key）。选 **始终允许**。这不是 ChatGPT 账号密码
- 因此官方 **智能快照**（拍照 / 截屏附件，英文 Appshot）会不可用，提示「无法附加智能快照」。这不是相机权限没开。Computer Use 一般还能用。`incodex uninstall` 卸掉补丁、回到未改的官方应用后，快照会恢复
- 官方应用升级后，补丁会被冲掉，需要再跑一次 `incodex install`。这次会给**当前这份官方应用**打补丁，不会把旧版本换回去，也不会删掉刚升上来的新版本
- 如果官方已经升级成新版本，`incodex uninstall` 不会用旧备份覆盖它，会提示当前应用已经不是 Incodex 安装态
- 每次安装的原始包按应用路径隔离，写在 `~/.incodex/installations/` 里。副本、正式应用和自定义 `--app` 不会共用一份备份。卸载前会核对当前包的 install ID、build 和完整 ASAR hash，对不上就拒绝恢复
- 目前只在 macOS 上试过

## 许可

MIT。完整文本见 [LICENSE](./LICENSE)。

- 仓库里的 `assets/hat-glasses.svg` 和 `src/runtime/incognito-copy.ts` 翻译文案是 Incodex 原作，同样按 MIT 授权
- Codex / ChatGPT 的名称、图标和官方应用本身属于 OpenAI，本仓库不授予那些材料的许可
