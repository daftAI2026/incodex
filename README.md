<p align="right">English | <strong><a href="./README_CN.md">简体中文</a></strong></p>

<div align="center">
  <img src="assets/hat-glasses.svg" alt="Incodex hat-glasses" width="96" />
  <h1>Incodex</h1>
  <p><em>Incognito toggle for Codex desktop.</em></p>
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
  <img src="assets/sidebar.png" alt="Hat-glasses control left of Search in Codex" width="1000" />
</p>

> Unofficial. `inc` is the same program as `incodex`.

## Features

- **Incognito window**: Same login and settings as usual. No old chats, and this session does not join the everyday list
- **Sidebar button**: After install, a hat-glasses control sits left of Search; `Shift+Command+N` also works
- **Burns on close**: A normal close clears this temp session (including the isolated Chromium profile); login and settings stay
- **Optional no-patch path**: `incodex open` launches an incognito window without touching the official signature; the hat-glasses control and banner still appear in that window
- **Local CLI**: Terminal menu, Homebrew or script install, `status` / `doctor` / `runtime`. Not an official plugin

This is not a forensics claim that the machine keeps no traces.

## Quick Start

**Install via Homebrew**

```bash
brew install daftAI2026/tap/incodex
```

This only puts `incodex` and `inc` on PATH. Patching Codex is still `incodex install`. Update with `brew upgrade incodex`.

**Or via script**

```bash
curl -fsSL https://raw.githubusercontent.com/daftAI2026/incodex/main/install.sh | bash
```

**From source**

```bash
git clone https://github.com/daftAI2026/incodex.git
cd incodex
bun install --frozen-lockfile
bun link
```

Needs [Bun](https://bun.sh) 1.3.14 (see `.bun-version`) and an installed Codex / ChatGPT desktop app.

**Run**

```bash
inc                         # Interactive menu (terminal only)
incodex --help
incodex --version

incodex install             # Patch the official Codex you are using
incodex install --dry-run   # Print the plan
incodex install --yes       # Required when stdin is not a terminal
incodex install --clone     # Dev: patch a copy

incodex uninstall           # Restore the official app
incodex status
incodex doctor
incodex runtime             # Update the button logic without re-signing Codex
incodex open                # Incognito window, no patch
incodex recover --transaction <id>
incodex update              # Update this CLI (script installs)
incodex self-uninstall      # Remove the CLI; add --restore-app to restore Codex
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
```

`brew install`, `curl … | bash`, and `bun link` only put the command on PATH. The command that changes `/Applications/ChatGPT.app` is `incodex install`.

## Security & Safety Design

Incodex patches a locally installed Electron app. Destructive commands print a plan first: TTY asks once, non-TTY needs `--yes`, `--dry-run` only prints.

- Official plugins cannot add this button. The app bundle has to change
- After the patch, a valid OpenAI signature cannot be kept. The first incognito window may ask for your **Mac login password** (Codex Storage Key in Keychain). Choose **Always Allow**. That is not your ChatGPT password
- Official **Appshot** (smart snapshot: photo / screenshot attachments) then stops working. This is not a missing camera permission. Computer Use usually still works. `incodex uninstall` restores Appshot
- Report vulnerabilities via [SECURITY.md](SECURITY.md). Do not open a public issue

## Tips

- An official upgrade wipes the patch. Run `incodex install` again on the **current** official package
- If Codex has already been upgraded, `incodex uninstall` will not put an old backup back
- Original-bundle backups are isolated per app path under `~/.incodex/installations/`
- Homebrew: `brew upgrade incodex`. Script: `incodex update`. Source: `git pull`
- The menu supports arrows, Vim `j/k`, digits that run immediately, `V` for version, `q` to quit
- If a script install cannot find the command, add `~/.local/bin` to PATH
- Button and copy follow the main window language

## Features in Detail

### Interactive menu

Run `inc` in a terminal:

```
  _____   _   _    _____    ____    _____    ______  __   __
 |_   _| | \ | |  / ____|  / __ \  |  __ \  |  ____| \ \ / /
   | |   |  \| | | |      | |  | | | |  | | | |__     \ V /
   | |   | . ` | | |      | |  | | | |  | | |  __|     > <
  _| |_  | |\  | | |____  | |__| | | |__| | | |____   / . \
 |_____| |_| \_|  \_____|  \____/  |_____/  |______| /_/ \_\

  https://github.com/daftAI2026/incodex
  Incognito toggle for Codex desktop.

➤ 1. Install     Patch the Codex app you are using
  2. Uninstall   Restore the official Codex app
  3. Open        Open an incognito window without patching
  4. Status      Show whether Incodex is installed
  5. Doctor      Diagnose the install and leftover sessions
  6. Quit        Exit this menu

↑↓ | Enter | V Version | Q Quit | 1-6 Jump
```

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
  ✓ Official app patched
  Install id   0778f0fa-…
  Runtime      0.1.0
  App          /Applications/ChatGPT.app
➤ Relaunch
  ✓ ChatGPT.app relaunched.
```

After install, the hat-glasses control appears left of Search. Click it or press `Shift+Command+N` for an incognito window.

### Open without patching

```bash
$ incodex open --dry-run

➤ Open incognito without patching Codex
  App          /Applications/ChatGPT.app
  Binary       /Applications/ChatGPT.app/Contents/MacOS/ChatGPT
  ! Dry run. No window opened.
```

Closing the window burns the isolated session:

```bash
$ incodex open

➤ Opening incognito window
  Binary       /Applications/ChatGPT.app/Contents/MacOS/ChatGPT
  Home         ~/.incodex/sessions/…
  ✓ Closed. Isolated session removed.
```

### Status

```bash
$ incodex status

➤ Status
  App          /Applications/ChatGPT.app
  Exists       yes
  Installed    yes
  Loader       asar loader only
  Runtime      0.1.0 releases/0.1.0
  Version      26.814.41407 6720
  Install id   0778f0fa-…
  Target       official-404f3389062b
  Main         .vite/build/early-bootstrap.js
```

### Doctor

```bash
$ incodex doctor

➤ App
  Path         /Applications/ChatGPT.app
  Exists       yes
  Installed    yes
  Bundle       com.openai.codex
  Version      26.814.41407 6720
  Arch         arm64

➤ Runtime
  Version      0.1.0
  External     0.1.0 releases/0.1.0
  Loader       asar only
  Main         .vite/build/early-bootstrap.js

➤ Signing
  Verify       ok
  Hardened     yes
  Gatekeeper   not accepted (diagnostic)

➤ Backup
  State        ok
  Matches      yes

➤ Sessions
  Orphans      0
  Chromium     0
  Stale pid    no
  Journals     0
```

The Gatekeeper line is diagnostic, not an install failure. After the bundle changes, the official signature will not pass Gatekeeper.

### Version

```bash
$ incodex --version

Incodex version 0.1.0
macOS: 26.6
Architecture: arm64
Kernel: 25.6.0
SIP: Enabled
Disk Free: 120.00GB
Install: Homebrew
Shell: /bin/zsh
```

`Install` is Homebrew, Script, or Source. Homebrew installs should not run `incodex update`; use `brew upgrade incodex`.

## License

MIT. Full text in [LICENSE](./LICENSE). Third-party notices in [NOTICE](./NOTICE).

- Translations in `src/runtime/incognito-copy.ts` are original Incodex work, also under MIT
- Codex / ChatGPT names, icons, and the official app belong to OpenAI. This repository does not license those materials
