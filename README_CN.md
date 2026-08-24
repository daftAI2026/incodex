<p align="right"><strong><a href="./README.md">English</a></strong> | 简体中文</p>

<div align="center">
  <img src="assets/hat-glasses.svg" alt="Incodex 帽子墨镜" width="96" />
  <h1>Incodex</h1>
  <p><em>给本机 Codex 桌面端加一扇无痕窗口。</em></p>
</div>

<p align="center">
  <a href="https://github.com/daftAI2026/incodex/stargazers"><img src="https://img.shields.io/github/stars/daftAI2026/incodex?style=flat-square" alt="Stars"></a>
  <a href="https://github.com/daftAI2026/incodex/releases"><img src="https://img.shields.io/github/v/tag/daftAI2026/incodex?label=version&style=flat-square" alt="Version"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg?style=flat-square" alt="License"></a>
  <a href="https://github.com/daftAI2026/incodex/commits"><img src="https://img.shields.io/github/commit-activity/m/daftAI2026/incodex?style=flat-square" alt="Commits"></a>
  <a href="https://github.com/daftAI2026/incodex/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/daftAI2026/incodex/ci.yml?style=flat-square" alt="CI"></a>
  <a href="https://x.com/singkid9527"><img src="https://img.shields.io/badge/follow-singkid9527-red?style=flat-square&logo=X" alt="Follow"></a>
</p>

<p align="center">
  <img src="assets/sidebar.png" alt="Codex 搜索左边的帽子墨镜" width="1000" />
</p>

> 这是非官方工具。短命令是 `inc`，和 `incodex` 是同一个程序。

## Features

- **无痕窗口**：登录和设置跟平时一样，看不到以前的对话，这次的聊天也不会进平时的列表
- **跟随主窗口**：无痕窗口参照主窗口的大小和位置打开
- **侧栏按钮**：装进正在用的 Codex 后，搜索左边会出现帽子墨镜；`Shift+Command+N` 也能开
- **关窗即焚**：正常关掉后清掉这次的临时会话（含独立 Chromium 档案）；登录和设置会留着
- **可选不改包**：`incodex open` 直接开一扇无痕窗，不碰官方签名
- **临时资料遮罩**：`incodex open --mask [--name <text>] [--avatar <local-file>]` 给这扇窗口一个临时两词名称和离线确定性头像。头像只能用本地 PNG、JPEG 或 WebP；它只改当前窗口的 profile footer 与已打开账号菜单，不改真实账号
- **本机 CLI**：终端菜单、Homebrew / 脚本安装、`status` / `doctor` / `runtime`，不经过官方插件

这还不是「本机完全不留记录」的取证结论。

## Quick Start

**支持平台：** Apple Silicon（arm64）和 Intel（x86_64）Mac。当前不支持 Windows 或 Linux，因为 Incodex 依赖 macOS 的 Codex 应用包、代码签名、钥匙串和 Launch Services。

**Install via Homebrew**

```bash
brew install daftAI2026/tap/incodex
```

只把 `incodex` 和 `inc` 放到 PATH。改 Codex 仍然是随后那条 `incodex install`。更新用 `brew upgrade incodex`。

**Or via script**

```bash
curl -fsSL https://raw.githubusercontent.com/daftAI2026/incodex/main/install.sh | bash
```

**从源码**

```bash
git clone https://github.com/daftAI2026/incodex.git
cd incodex
cargo install --locked --path crates/incodex-cli
```

Homebrew 和脚本安装直接使用预编译的原生 Rust 二进制，不需要 Bun。从源码安装需要 [rustup](https://rustup.rs/)，仓库中的 `rust-toolchain.toml` 会选择受支持的 Rust 编译器；只有涉及应用集成时才需要已安装的 Codex / ChatGPT 桌面端，参与开发、重建 Electron Runtime 时还需要 [Bun](https://bun.sh) 1.3.14（见 `.bun-version`）。

## Security & Safety Design

Incodex 会改本机已安装的 Electron 应用包。高风险操作默认要看计划：TTY 问一次，非 TTY 要 `--yes`，`--dry-run` 只打印。

- 官方插件加不了这个按钮，必须改应用包
- 默认安装到官方应用后，改包没法继续保留有效的 OpenAI 签名。下次启动时，macOS 可能要求这个已修改的应用访问钥匙串中的 **Codex Storage Key**。只有对话框里的应用和钥匙串项目都符合预期时，才输入 **Mac 登录密码**（不是 ChatGPT 账号密码）并选择 **始终允许**。**允许** / **允许一次**只授权本次访问，之后还可能再次询问；信息不符合预期时选择 **拒绝**。CLI 不会对 `--clone` 或 `--app` 目标给出永久授权建议
- 官方 **智能快照**（拍照 / 截屏附件，英文 Appshot）会不可用。这不是相机权限没开。Computer Use 一般还能用。`incodex uninstall` 后快照会恢复
- 漏洞请走 [SECURITY.md](SECURITY.md)，不要开公开 issue

## Tips

- 官方升级会冲掉补丁。再跑一次 `incodex install`，打的是**当前这份**新官方包
- 如果官方已经升成新版本，`incodex uninstall` 不会用旧备份盖回去
- 原始包备份按应用路径隔离，在 `~/.incodex/installations/`
- Homebrew 装的用 `brew upgrade incodex`；脚本装的用 `incodex update`；源码更新用 `git pull && cargo install --locked --path crates/incodex-cli`；源码卸载用 `cargo uninstall incodex-cli`
- 菜单支持方向键、Vim `j/k`、数字立刻执行、`V` 看版本、`q` 退出
- 脚本安装若找不到命令，把 `~/.local/bin` 加进 PATH
- 按钮和说明跟主窗口语言走

## Features in Detail

### Interactive menu

终端里直接跑 `inc`：

```
  _____   _   _    _____    ____    _____    ______  __   __
 |_   _| | \ | |  / ____|  / __ \  |  __ \  |  ____| \ \ / /
   | |   |  \| | | |      | |  | | | |  | | | |__     \ V /
   | |   | . ` | | |      | |  | | | |  | | |  __|     > <
  _| |_  | |\  | | |____  | |__| | | |__| | | |____   / . \
 |_____| |_| \_|  \_____|  \____/  |_____/  |______| /_/ \_\

  https://github.com/daftAI2026/incodex
  Incognito toggle for Codex desktop.

➤ 1. Open        Open an incognito window without patching
  2. Install     Patch the Codex app you are using
  3. Uninstall   Restore the official Codex app
  4. Status      Show whether Incodex is installed
  5. Doctor      Diagnose the install and leftover sessions
  6. Quit        Exit this menu

↑↓ | Enter | V Version | Q Quit | 1-6 Jump
```

### Open without patching

```bash
$ incodex open --dry-run

➤ Open incognito without patching Codex
  App          /Applications/ChatGPT.app
  Binary       /Applications/ChatGPT.app/Contents/MacOS/ChatGPT
  ! Dry run. No window opened.
```

关窗后隔离会话会被清掉：

```bash
$ incodex open

➤ Opening incognito Codex window
  Binary       /Applications/ChatGPT.app/Contents/MacOS/ChatGPT
  Home         ~/.incodex/sessions/…
  Session      s-…
  ✓ Opened. Incognito Codex window is ready.
  ✓ Closed. Isolated session removed.
```

需要临时视觉身份时，只在 `open` 路径加 `--mask`：

```bash
$ incodex open --mask
$ incodex open --mask --name "Quiet Otter" --avatar ./avatar.png
```

遮罩只改当前无痕 renderer 的 profile footer 与已打开账号菜单中的身份行，不改真实账号。

### Install

```bash
$ incodex install

➤ Install
  App          /Applications/ChatGPT.app
  Version      26.814.41957 6744
  Signed       yes
  ! Replaces the app in place and resigns it ad hoc.
  ! Official Appshot (smart snapshot) stops until uninstall.
  Backup       ~/.incodex/installations/
  Install id   0778f0fa-…
  Runtime      0.4.0
  App          /Applications/ChatGPT.app
  ✓ Done. Open ChatGPT.app when you want Incognito.
  ! Keychain: On next launch, macOS may ask this patched Codex app to access Codex Storage Key.
  ! Confirm the dialog names this app and the Codex Storage Key item.
  ! If both match, enter your Mac login password (not your ChatGPT password) and choose Always Allow.
  ! Allow or Allow Once grants only that access and may prompt again later.
  ! If the details do not match, choose Deny; Incodex and Terminal never need that password.
```

装进去之后，搜索左边会出现帽子墨镜。点它或 `Shift+Command+N` 开无痕窗。

### Status

```bash
$ incodex status

➤ Status
  App          /Applications/ChatGPT.app
  Exists       yes
  Installed    yes
  Loader       asar loader only
  Runtime      0.4.0 releases/0.4.0-<manifestSha256>
  Version      26.814.41957 6744
  Install id   0778f0fa-…
  Target       official-404f3389062b
  Main         .vite/build/early-bootstrap.js
  ✓ Incodex is installed. Use doctor for hashes and signing.
```

### Doctor

```bash
$ incodex doctor

➤ App
  Path         /Applications/ChatGPT.app
  Exists       yes
  Installed    yes
  Bundle       com.openai.codex
  Version      26.814.41957 6744
  Arch         arm64

➤ Runtime
  Version      0.4.0
  External     0.4.0 releases/0.4.0-<manifestSha256>
  External check checked
  Loader       asar only
  Main         .vite/build/early-bootstrap.js

➤ Signing
  Verify       ok
  Nested       unknown

➤ Backup
  State        ok
  Proof        checked
  Matches      yes

➤ Sessions
  Orphans      0 (checked)
  Chromium     0 (checked)
  Stale pid    no (checked)
  Journals     0 (checked)
```

默认 Doctor 会检查 Incodex 自己的 Runtime、备份、journal、session 和 marker 状态，以及目标应用最小的 outer identity 证据；不会递归 nested 签名，也不会调用 Gatekeeper。要看完整的 nested 签名、entitlement 和 Gatekeeper 报告，请运行 `incodex doctor --deep`。Gatekeeper 结果只是诊断，不是安装失败；改包之后官方签名本来就不会过 Gatekeeper。

### Version

```bash
$ incodex --version

Incodex version 0.4.0
macOS: 26.6
Architecture: arm64
Kernel: 25.6.0
SIP: Enabled
Disk Free: 120.00GB
Install: Homebrew
Shell: /bin/zsh
```

`Install` 能识别 Homebrew 路径；其他原生二进制目前显示为 Script。Homebrew 装的不要跑 `incodex update`，用 `brew upgrade incodex`。

## Quick Launchers

<details>
<summary><strong>Raycast 和 Alfred 设置</strong></summary>

安装 Open、Status、Doctor 三个快捷入口：

```bash
curl -fsSL https://raw.githubusercontent.com/daftAI2026/incodex/main/scripts/setup-quick-launchers.sh | bash
```

脚本会把 Raycast 命令写入 `~/Library/Application Support/Raycast/script-commands`；检测到 Alfred 时，才会生成标准 `.alfredworkflow` 包并打开 Alfred 的正常导入确认。Raycast 需要手动设置一次；v1 和 v2 加载的是同一套脚本，只是添加目录的入口不同：

1. **Raycast v2：**打开 **Settings → Script Commands**，在 **Script Folders** 右侧点击 **+**。
2. **Raycast v1：**打开 **Settings → Extensions**，点击 **+**，再选 **Add Script Directory**。
3. 选择 `~/Library/Application Support/Raycast/script-commands`。
4. 在 Raycast 中运行 **Reload Script Commands**。

Raycast 提供可用的 `TERM` 时，Status 和 Doctor 直接在它的 `fullOutput` 中运行。没有可用的 `TERM` 时，会通过 Terminal、iTerm2、Alacritty、kitty、WezTerm、Ghostty、Hyper、WindTerm 或 Warp 启动；可以用 `INCODEX_LAUNCHER_APP=<name>` 指定。如果所选应用启动失败，会回退到 Terminal。Open 会直接打开无痕窗口。

**Run**

```bash
inc                         # 交互菜单（终端里）
incodex --help
incodex --version

incodex install             # 打进正在用的官方 Codex
incodex install --dry-run   # 只看计划
incodex install --yes       # 没有终端时必须加
incodex install --clone     # 开发：打到副本

incodex uninstall           # 还原官方包
incodex status
incodex doctor
incodex doctor --deep        # 完整 nested 签名 / entitlement / Gatekeeper 证据
incodex runtime             # 只更新按钮逻辑，不重签 Codex
incodex open                # 不改官方包，直接开无痕窗
incodex open --mask         # 临时侧栏名称和离线头像
incodex open --mask --name "Quiet Otter" --avatar ./avatar.png
incodex recover --transaction <id>
incodex update              # 更新这个 CLI（脚本安装）
incodex self-uninstall      # 卸掉 CLI；还原 Codex 要加 --restore-app
```

**Preview safely**

```bash
incodex install --dry-run
incodex uninstall --dry-run
incodex open --dry-run
incodex update --dry-run
incodex self-uninstall --dry-run
incodex status --json
incodex doctor --json
incodex doctor --deep --json
```

`brew install`、`curl … | bash`、`cargo install` 都只把命令装到 PATH。改 `/Applications/ChatGPT.app` 的是随后那条 `incodex install`。

</details>

## License

MIT，完整文本见 [LICENSE](./LICENSE)。第三方声明见 [NOTICE](./NOTICE)。

- `src/runtime/incognito-copy.ts` 里的翻译文案是 Incodex 原作，同样按 MIT 授权
- Codex / ChatGPT 的名称、图标和官方应用本身属于 OpenAI，本仓库不授予那些材料的许可
