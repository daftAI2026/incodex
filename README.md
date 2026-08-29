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

- **No-patch incognito**: `incodex open` launches the official Codex binary with an isolated profile, without modifying the app or its signature
- **Separate history**: Same login and settings as usual. No old chats, and this session does not join the everyday list
- **Temporary profile mask**: `incodex open --mask [--name <text>] [--avatar <local-file>]` gives the window a temporary two-word name and deterministic offline avatar. The optional avatar must be a local PNG, JPEG, or WebP; this changes the current window's profile footer and open account menu, not account data
- **Follows the main window**: The incognito window opens using the main window’s size and placement
- **Burns on close**: A normal close clears this temp session (including the isolated Chromium profile); login and settings stay
- **Optional sidebar button**: After `incodex install`, a hat-glasses control sits left of Search; `Shift+Command+N` also works
- **Local CLI**: Terminal menu, Homebrew or script install, `status` / `doctor` / `runtime`. Not an official plugin

A normal close removes the isolated session managed by Incodex; this is not a claim of forensic erasure from the device or remote services.

## Quick Start

**Supported platform:** macOS on Apple Silicon (arm64) and Intel (x86_64). Windows and Linux are not supported because Incodex integrates with the macOS Codex app bundle, code signing, Keychain, and Launch Services.

**Install via Homebrew**

```bash
brew install daftAI2026/tap/incodex
```

This only puts `incodex` and `inc` on PATH. The optional in-app button is added separately with `incodex install`. Update with `inc update`; Incodex keeps Homebrew installs on the Homebrew upgrade path and publishes the bundled Runtime automatically.

**Or via script**

```bash
curl -fsSL https://raw.githubusercontent.com/daftAI2026/incodex/main/install.sh | bash
```

**From source**

```bash
git clone https://github.com/daftAI2026/incodex.git
cd incodex
cargo install --locked --path crates/incodex-cli
```

Homebrew and script installs use prebuilt native Rust binaries and do not require Bun. A source install requires [rustup](https://rustup.rs/); the repository's `rust-toolchain.toml` selects the supported Rust compiler. An installed Codex / ChatGPT desktop app is needed only for app integration work. Contributors rebuilding the Electron Runtime also need [Bun](https://bun.sh) 1.3.14 (see `.bun-version`).

## Security & Safety Design

The primary `incodex open` path does not patch Codex. The optional `incodex install` path adds the in-app button by modifying the locally installed Electron app. `install`, `uninstall`, and `self-uninstall` print a plan before destructive work: TTY asks once, non-TTY needs `--yes`, and `--dry-run` only prints. `recover` is the explicit transaction-recovery exception: it requires `--transaction <id>`, does not accept `--dry-run`, and resumes only that existing journal.

- Official plugins cannot add this button. The app bundle has to change
- After the default official-app install, a valid OpenAI signature cannot be kept. On the next launch, macOS may ask the patched app to access **Codex Storage Key**. Only if the dialog names the expected app and Keychain item should you enter your **Mac login password** (not your ChatGPT password) and choose **Always Allow**. **Allow** / **Allow Once** grants only that access and may prompt again later; if the details do not match, choose **Deny**. The CLI does not give permanent-authorization advice for `--clone` or `--app` targets
- Official **Appshot** (smart snapshot: photo / screenshot attachments) then stops working. This is not a missing camera permission. Computer Use usually still works. `incodex uninstall` restores Appshot
- Report vulnerabilities via [SECURITY.md](SECURITY.md). Do not open a public issue

## Tips

- An official upgrade wipes the patch. Run `incodex install` again on the **current** official package
- If Codex has already been upgraded, `incodex uninstall` will not put an old backup back
- The current original-bundle backup lives at `~/.incodex/transactions/<install-id>/original/ChatGPT.app`; verified uninstall removes it, and a later successful install prunes superseded terminal backups for the same app
- Run `inc update` for Homebrew and script installs; Incodex automatically uses the matching update path and publishes the bundled Runtime without re-signing Codex. Source update: `git pull && cargo install --locked --path crates/incodex-cli`; source removal: `cargo uninstall incodex-cli`
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

➤ 1. Open        Open an incognito window without patching
  2. Install     Patch the Codex app you are using
  3. Uninstall   Restore the official Codex app
  4. Status      Show whether Incodex is installed
  5. Doctor      Diagnose the install and leftover sessions
  6. Quit        Exit this menu

↑↓ | Enter | V Version | Q Quit | 1-6 Jump
```

### Open without patching

`open` starts the official Codex binary with a fresh isolated Chromium profile and `CODEX_HOME`. It keeps the login and base configuration needed for use, but does not bring old chats into the window or modify and re-sign the official app. A normal close burns the isolated session.

```bash
$ incodex open --dry-run

➤ Open incognito without patching Codex
  App          /Applications/ChatGPT.app
  Binary       /Applications/ChatGPT.app/Contents/MacOS/ChatGPT
  ! Dry run. No window opened.
```

Open the window:

```bash
$ incodex open

➤ Opening incognito Codex window
  Binary       /Applications/ChatGPT.app/Contents/MacOS/ChatGPT
  Home         ~/.incodex/sessions/…
  Session      s-…
  ✓ Opened. Incognito Codex window is ready.
  ✓ Closed. Isolated session removed.
```

For a temporary visual identity, use `--mask` on the `open` path only:

```bash
$ incodex open --mask                                      # Random name and generated avatar
$ incodex open --mask --name "Quiet Otter"                # Chosen name and generated avatar
$ incodex open --mask --avatar ./avatar.png                # Random name and local avatar
$ incodex open --mask --name "Quiet Otter" --avatar ./avatar.png
```

Without `--name`, each launch gets a friendly random two-word name. Without `--avatar`, Incodex generates an offline avatar from the final name, so the same name produces the same avatar. A custom avatar must be a regular local PNG, JPEG, or WebP no larger than 5 MiB; the original file is left unchanged and centered into Codex's circular avatar slot. `--name` and `--avatar` require `--mask`, and names containing spaces need shell quotes.

The mask changes only the current incognito window's profile footer and account-menu identity row. It does not change the real account, authentication, or stored profile.
If the mask cannot mount or later recover after a renderer remount, Incodex closes that window rather than expose the real identity.

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

After install, the hat-glasses control appears left of Search. Click it or press `Shift+Command+N` for an incognito window.

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

The default Doctor checks Incodex-owned Runtime, backup, journal, session, and marker state plus minimal outer app identity. It does not recurse into nested signing or invoke Gatekeeper. Run `incodex doctor --deep` for the full nested signing, entitlement, and Gatekeeper report. The Gatekeeper result is diagnostic, not an install failure; after the bundle changes, the official signature will not pass Gatekeeper.

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

`Install` reports Homebrew when the executable is recognized in its Homebrew location; other native binaries currently report Script. `inc update` refreshes and upgrades through Homebrew for Homebrew installs, and re-runs the stable installer for script installs. Both paths then publish the Runtime bundled with the installed CLI; they do not patch or re-sign Codex.

### Command reference

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
incodex doctor --deep       # Full nested signing / entitlement / Gatekeeper evidence
incodex runtime             # Update the button logic without re-signing Codex
incodex open                # Incognito window, no patch
incodex open --mask         # Temporary sidebar name and offline avatar
incodex open --mask --name "Quiet Otter" --avatar ./avatar.png
incodex recover --transaction <id>
inc update                  # Update Incodex through its install channel
incodex self-uninstall      # Remove the CLI; add --restore-app to restore Codex
```

**Preview safely**

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

`brew install`, `curl … | bash`, and `cargo install` only put the command on PATH. The command that changes `/Applications/ChatGPT.app` is `incodex install`.

## Quick Launchers

<details>
<summary><strong>Raycast and Alfred setup</strong></summary>

Install three launchers for Open, Status, and Doctor:

```bash
curl -fsSL https://raw.githubusercontent.com/daftAI2026/incodex/main/scripts/setup-quick-launchers.sh | bash
```

The script writes Raycast commands to `~/Library/Application Support/Raycast/script-commands`; when Alfred is detected, it creates the standard `.alfredworkflow` package and opens Alfred's normal import confirmation. Raycast needs one manual setup; v1 and v2 load the same scripts, but expose the directory picker in different places:

1. **Raycast v2:** open **Settings → Script Commands**, then click **+** beside **Script Folders**.
2. **Raycast v1:** open **Settings → Extensions**, click **+**, then choose **Add Script Directory**.
3. Select `~/Library/Application Support/Raycast/script-commands`.
4. Run **Reload Script Commands** in Raycast.

When Raycast provides a usable `TERM`, Status and Doctor run directly in its `fullOutput` pane. Without a usable `TERM`, they route through Terminal, iTerm2, Alacritty, kitty, WezTerm, Ghostty, Hyper, WindTerm, or Warp; set `INCODEX_LAUNCHER_APP=<name>` to choose one. If the selected app cannot be started, the launcher falls back to Terminal. Open launches the incognito window directly.

</details>

## License

MIT. Full text in [LICENSE](./LICENSE). Third-party notices in [NOTICE](./NOTICE).

- Translations in `src/runtime/incognito-copy.ts` are original Incodex work, also under MIT
- Codex / ChatGPT names, icons, and the official app belong to OpenAI. This repository does not license those materials
