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

- **免改包无痕窗口**：`incodex open` 使用隔离档案启动官方 Codex 二进制，不修改应用或官方签名
- **对话隔离**：登录和设置跟平时一样，看不到以前的对话，这次的聊天也不会进平时的列表
- **临时资料遮罩**：`incodex open --mask [--name <text>] [--avatar <local-file>]` 给这扇窗口一个临时两词名称和离线确定性头像。头像只能用本地 PNG、JPEG 或 WebP；它只改当前窗口的 profile footer 与已打开账号菜单，不改真实账号
- **跟随主窗口**：无痕窗口参照主窗口的大小和位置打开
- **关窗即焚**：正常关掉后清掉这次的临时会话（含独立 Chromium 档案）；登录和设置会留着
- **可选侧栏按钮**：运行 `incodex install` 后，搜索左边会出现帽子墨镜；macOS 用 `Shift+Command+N`，Windows 用 `Ctrl+Shift+N`
- **本机 CLI**：终端菜单、Homebrew / 脚本安装、`status` / `doctor` / `runtime`，不经过官方插件

正常关窗会清理 Incodex 管理的隔离会话；这不等于对本机存储或远端服务作取证级零痕迹承诺。

## Quick Start

**支持平台：** Apple Silicon（arm64）和 Intel（x86_64）Mac，以及安装了官方 Microsoft Store Codex 的 x86_64 的 Windows 10/11。暂不支持 Linux。

**Windows（PowerShell）**

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://raw.githubusercontent.com/daftAI2026/incodex/main/install.ps1 | iex"
```

这会把原生 `incodex` / `inc` 启动器装进当前用户私有的 `%USERPROFILE%\.incodex` 目录，并把它的 `bin` 加入用户 PATH。首次安装后请新开一个终端。脚本只安装 CLI；要启用应用内帽子墨镜按钮，仍需另行运行 `incodex install`。Incodex 会发现当前用户的 Store 包，不假定固定安装位置。以后统一运行 `inc update` 更新。

**macOS 通过 Homebrew 安装**

```bash
brew install daftAI2026/tap/incodex
```

这里只把 `incodex` 和 `inc` 放到 PATH。可选的应用内按钮需要另行运行 `incodex install`。更新统一使用 `inc update`；Homebrew 安装仍会沿 Homebrew 路径升级，并自动发布新 CLI 内置的 Runtime。

**macOS 使用安装脚本**

```bash
curl -fsSL https://raw.githubusercontent.com/daftAI2026/incodex/main/install.sh | bash
```

**从源码安装**

```bash
git clone https://github.com/daftAI2026/incodex.git
cd incodex
cargo install --locked --path crates/incodex-cli
```

各平台安装器直接使用预编译的原生 Rust 二进制，不需要 Bun。从源码安装需要 [rustup](https://rustup.rs/)，仓库中的 `rust-toolchain.toml` 会选择受支持的 Rust 编译器；只有涉及应用集成时才需要已安装的 Codex / ChatGPT 桌面端，参与开发、重建 Electron Runtime 时还需要 [Bun](https://bun.sh) 1.3.14（见 `.bun-version`）。

## Security & Safety Design

主路径 `incodex open` 不会修改 Codex。macOS 的可选 `incodex install` 会修改并重新签名本机应用包；Windows 不修改也不复制 Microsoft Store 包，而是注册一套 Incodex 自己拥有的当前用户 Runtime 集成。`install`、`uninstall` 和受支持安装渠道的 `self-uninstall` 在破坏性操作前都会打印计划：TTY 问一次，非 TTY 要 `--yes`，`--dry-run` 只打印。Windows 默认自卸载只移除托管 CLI 及其精确的用户 PATH 项；加 `--restore-app` 才会先移除 Runtime 集成，Runtime 文件和会话状态仍保留。在 macOS 上，`recover` 是显式事务恢复例外：必须带 `--transaction <id>`，不接受 `--dry-run`，并且只续跑这一本已存在的 journal。

- 官方插件加不了这个按钮；macOS 修改应用包，Windows 则保持 Store 包不变，走系统集成边界
- 默认安装到官方应用后，改包没法继续保留有效的 OpenAI 签名。下次启动时，macOS 可能要求这个已修改的应用访问钥匙串中的 **Codex Storage Key**。只有对话框里的应用和钥匙串项目都符合预期时，才输入 **Mac 登录密码**（不是 ChatGPT 账号密码）并选择 **始终允许**。**允许** / **允许一次**只授权本次访问，之后还可能再次询问；信息不符合预期时选择 **拒绝**。CLI 不会对 `--clone` 或 `--app` 目标给出永久授权建议
- 官方 **智能快照**（拍照 / 截屏附件，英文 Appshot）会不可用。这不是相机权限没开。Computer Use 一般还能用。`incodex uninstall` 后快照会恢复
- 漏洞请走 [SECURITY.md](SECURITY.md)，不要开公开 issue

## Tips

- 官方 Codex 升级后，再跑一次 `incodex install`，目标是**当前这份**应用或 Store 包版本
- 如果官方已经升成新版本，`incodex uninstall` 不会用旧备份盖回去
- 当前原始包备份位于 `~/.incodex/transactions/<install-id>/original/ChatGPT.app`；卸载恢复并验证成功后会删除，新一代安装成功后也会清理同一应用已被取代的终态备份
- Homebrew、macOS 脚本和 Windows PowerShell 安装都运行 `inc update`；Incodex 会自动选择对应升级路径，并发布新 CLI 内置的 Runtime。源码更新用 `git pull && cargo install --locked --path crates/incodex-cli`；源码卸载用 `cargo uninstall incodex-cli`
- 菜单支持方向键、Vim `j/k`、数字立刻执行、`V` 看版本、`q` 退出
- macOS 脚本安装若找不到命令，把 `~/.local/bin` 加进 PATH；Windows 首次安装后请新开终端，让更新后的用户 PATH 生效
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

`open` 使用一份全新的隔离 Chromium 档案和 `CODEX_HOME` 启动官方 Codex 二进制。登录和使用所需的基础配置会保留，但旧对话不会进入这扇窗口，官方应用也不会被修改或重新签名。正常关窗后，隔离会话会被清掉。

```bash
$ incodex open --dry-run

➤ Open incognito without patching Codex
  App          /Applications/ChatGPT.app
  Binary       /Applications/ChatGPT.app/Contents/MacOS/ChatGPT
  ! Dry run. No window opened.
```

打开窗口：

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
$ incodex open --mask                                      # 随机名字和生成头像
$ incodex open --mask --name "Quiet Otter"                # 指定名字和生成头像
$ incodex open --mask --avatar ./avatar.png                # 随机名字和本地头像
$ incodex open --mask --name "Quiet Otter" --avatar ./avatar.png
```

没有 `--name` 时，每次启动会获得一个友好的随机两词名字；没有 `--avatar` 时，Incodex 会根据最终名字离线生成头像，因此同一个名字会得到同一个头像。自定义头像必须是普通本地 PNG、JPEG 或 WebP 文件，且不超过 5 MiB；原文件不会被修改，显示时会居中放进 Codex 的圆形头像槽。`--name` 和 `--avatar` 都必须与 `--mask` 一起使用，含空格的名字需要 shell 引号。

遮罩只改当前无痕窗口的 profile footer 与账号菜单身份行，不改真实账号、认证或已存资料。
如果遮罩无法挂载，或在 renderer 重挂载后无法恢复，Incodex 会关闭该窗口，而不是暴露真实身份。

### Install

```bash
$ incodex install

➤ Install
  App          /Applications/ChatGPT.app
  Version      26.814.41957 6744
  Signed       yes
  ! Replaces the app in place and resigns it ad hoc.
  ! Official Appshot (smart snapshot) stops until uninstall.
  Backup       ~/.incodex/transactions/<install-id>/original/ChatGPT.app
  Install id   0778f0fa-…
  Runtime      0.5.0
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
  Runtime      0.5.0 releases/0.5.0-<manifestSha256>
  CLI Runtime  0.5.0
  Runtime state current
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
  Version      0.5.0
  External     0.5.0 releases/0.5.0-<manifestSha256>
  External check checked
  CLI Runtime  0.5.0
  CLI manifest <manifestSha256>
  Deployed manifest <manifestSha256>
  Runtime state current
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

Incodex version 0.5.0
macOS: 26.6
Architecture: arm64
Kernel: 25.6.0
SIP: Enabled
Disk Free: 120.00GB
Install: Homebrew
Shell: /bin/zsh
```

`Install` 能识别 Homebrew 路径；其他原生二进制目前显示为 Script。`inc update` 会让 Homebrew 安装通过 Homebrew 刷新并升级，让脚本安装重新运行稳定版安装器。两条路径随后都会发布已安装 CLI 内置的 Runtime，不改包，也不重新签名 Codex。

### 命令速查

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
incodex doctor --deep       # 完整 nested 签名 / entitlement / Gatekeeper 证据
incodex runtime             # 只更新按钮逻辑，不重签 Codex
incodex open                # 不改官方包，直接开无痕窗
incodex open --mask         # 临时侧栏名称和离线头像
incodex open --mask --name "Quiet Otter" --avatar ./avatar.png
incodex recover --transaction <id>
inc update                  # 按安装来源更新 Incodex
incodex self-uninstall      # 卸掉 CLI；移除应用集成要加 --restore-app
```

**安全预览**

```bash
incodex install --dry-run
incodex uninstall --dry-run
incodex open --dry-run
inc update --dry-run
incodex self-uninstall --dry-run
incodex status --json
incodex doctor --json
incodex doctor --deep --json
```

`brew install`、`curl … | bash`、`cargo install` 都只把命令装到 PATH。改 `/Applications/ChatGPT.app` 的是随后那条 `incodex install`。

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

</details>

## License

MIT，完整文本见 [LICENSE](./LICENSE)。第三方声明见 [NOTICE](./NOTICE)。

- `src/runtime/incognito-copy.ts` 里的翻译文案是 Incodex 原作，同样按 MIT 授权
- Codex / ChatGPT 的名称、图标和官方应用本身属于 OpenAI，本仓库不授予那些材料的许可
