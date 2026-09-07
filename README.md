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
- **Optional sidebar button**: After `incodex install`, a hat-glasses control sits left of Search; use `Shift+Command+N` on macOS or `Ctrl+Shift+N` on Windows
- **Local CLI**: Terminal menu, Homebrew or script install, `status` / `doctor` / `runtime`. Not an official plugin

A normal close removes the isolated session managed by Incodex; this is not a claim of forensic erasure from the device or remote services.

## Quick Start

**Supported platforms:** macOS on Apple Silicon (arm64) and Intel (x86_64), plus Windows 10/11 on x86_64 with the official Microsoft Store Codex app. Linux is not supported.

**Windows (PowerShell)**

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://raw.githubusercontent.com/daftAI2026/incodex/main/install.ps1 | iex"
```

This installs the native `incodex` / `inc` launchers under the current user's private `%USERPROFILE%\.incodex` directory and adds its `bin` directory to the user PATH. Open a new PowerShell window after the first install, then verify the CLI:

```powershell
incodex --version
```

The script installs only the CLI. `incodex open` can open an isolated window immediately without enabling app integration. To add the in-app hat-glasses control, fully quit Codex with `Ctrl+Q` or the tray **Quit** command, then run:

```powershell
incodex install
```

Reopen the official Codex app when installation finishes. Incodex discovers the current user's Store package instead of assuming its install location. Run `inc update` to update Incodex itself. After an official Store Codex update, fully quit Codex and run `incodex install` again for the current package generation.

**macOS via Homebrew**

```bash
brew install daftAI2026/tap/incodex
```

This only puts `incodex` and `inc` on PATH. The optional in-app button is added separately with `incodex install`. Update with `inc update`; Incodex keeps Homebrew installs on the Homebrew upgrade path and publishes the bundled Runtime automatically.

**macOS via script**

```bash
curl -fsSL https://raw.githubusercontent.com/daftAI2026/incodex/main/install.sh | bash
```

**From source**

```bash
git clone https://github.com/daftAI2026/incodex.git
cd incodex
cargo install --locked --path crates/incodex-cli
```

Platform installers use prebuilt native Rust binaries and do not require Bun. A source install requires [rustup](https://rustup.rs/); the repository's `rust-toolchain.toml` selects the supported Rust compiler. An installed Codex / ChatGPT desktop app is needed only for app integration work. Contributors rebuilding the Electron Runtime also need [Bun](https://bun.sh) 1.3.14 (see `.bun-version`).

## Security & Safety Design

The primary `incodex open` path does not patch Codex. On macOS, the optional `incodex install` path adds the in-app button by modifying and re-signing the local app bundle. On Windows, it never patches or copies the Microsoft Store package; it registers a separately owned, per-user Runtime integration. `install`, `uninstall`, and supported `self-uninstall` channels print a plan before destructive work: TTY asks once, non-TTY needs `--yes`, and `--dry-run` only prints. On Windows, `self-uninstall` removes only the managed CLI and its exact user PATH entry by default; `--restore-app` first removes the Runtime integration, while Runtime files and session state remain. On macOS, `recover` is the explicit transaction-recovery exception: it requires `--transaction <id>`, does not accept `--dry-run`, and resumes only that existing journal.

- Official plugins cannot add this button. macOS changes the app bundle; Windows keeps the Store package intact and uses its platform integration boundary
- After the default official-app install, a valid OpenAI signature cannot be kept. On the next launch, macOS may ask the patched app to access **Codex Storage Key**. Only if the dialog names the expected app and Keychain item should you enter your **Mac login password** (not your ChatGPT password) and choose **Always Allow**. **Allow** / **Allow Once** grants only that access and may prompt again later; if the details do not match, choose **Deny**. The CLI does not give permanent-authorization advice for `--clone` or `--app` targets
- Official **Appshot** (smart snapshot: photo / screenshot attachments) then stops working. This is not a missing camera permission. Computer Use usually still works. `incodex uninstall` restores Appshot
- Report vulnerabilities via [SECURITY.md](SECURITY.md). Do not open a public issue

## Tips

- After an official Codex upgrade, run `incodex install` again against the **current** app or Store package generation
- If Codex has already been upgraded, `incodex uninstall` will not put an old backup back
- On macOS, the current original-bundle backup lives at `~/.incodex/transactions/<install-id>/original/ChatGPT.app`; verified uninstall removes it, and a later successful install prunes superseded terminal backups for the same app
- Run `inc update` for Homebrew, macOS script, and Windows PowerShell installs; Incodex automatically uses the matching update path and publishes the bundled Runtime. Source update: `git pull && cargo install --locked --path crates/incodex-cli`; source removal: `cargo uninstall incodex-cli`
- The menu supports arrows, Vim `j/k`, digits that run immediately, `V` for version, `q` to quit
- If a macOS script install cannot find the command, add `~/.local/bin` to PATH. On Windows, open a new terminal so the updated user PATH is loaded
- Button and copy follow the main window language

## Features in Detail

The terminal snapshots below show macOS output. Windows keeps the same public command names and shared incognito-window UI, but prints Store package and Windows Runtime integration evidence instead of macOS app-bundle and signing details. Platform-only commands and flags are marked in the command reference.

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
  Runtime      1.0.0
  App          /Applications/ChatGPT.app
  ✓ Done. Open ChatGPT.app when you want Incognito.
  ! Keychain: On next launch, macOS may ask this patched Codex app to access Codex Storage Key.
  ! Confirm the dialog names this app and the Codex Storage Key item.
  ! If both match, enter your Mac login password (not your ChatGPT password) and choose Always Allow.
  ! Allow or Allow Once grants only that access and may prompt again later.
  ! If the details do not match, choose Deny; Incodex and Terminal never need that password.
```

After install, the hat-glasses control appears left of Search. Click it, press `Shift+Command+N` on macOS, or press `Ctrl+Shift+N` on Windows for an incognito window. The output above is the macOS bundle-patching path; Windows leaves the Store package untouched and registers a per-user Runtime integration.

### Status

```bash
$ incodex status

➤ Status
  App          /Applications/ChatGPT.app
  Exists       yes
  Installed    yes
  Loader       asar loader only
  Runtime      1.0.0 releases/1.0.0-<manifestSha256>
  CLI Runtime  1.0.0
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
  Version      1.0.0
  External     1.0.0 releases/1.0.0-<manifestSha256>
  External check checked
  CLI Runtime  1.0.0
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

The default Doctor checks Incodex-owned Runtime, backup, journal, session, and marker state plus minimal outer app identity. On macOS, it does not recurse into nested signing or invoke Gatekeeper; run `incodex doctor --deep` for the full nested signing, entitlement, and Gatekeeper report. The Gatekeeper result is diagnostic, not an install failure; after the bundle changes, the official signature will not pass Gatekeeper. Windows exposes its platform-relevant checks through the default `incodex doctor` command.

### Version

```bash
$ incodex --version

Incodex version 1.0.0
macOS: 26.6
Architecture: arm64
Kernel: 25.6.0
SIP: Enabled
Disk Free: 120.00GB
Install: Homebrew
Shell: /bin/zsh
```

`Install` reports Homebrew when the executable is recognized in its Homebrew location; other installed native binaries currently report Script. `inc update` refreshes and upgrades through Homebrew for Homebrew installs, re-runs `install.sh` for macOS script installs, and uses the verified PowerShell installer for managed Windows installs. Every successful path then publishes the Runtime bundled with the installed CLI; it does not patch or re-sign Codex.

### Command reference

```bash
inc                         # Interactive menu (terminal only)
incodex --help
incodex --version

incodex install             # Enable the in-app hat-glasses control
incodex install --dry-run   # Print the plan
incodex install --yes       # Required when stdin is not a terminal
incodex install --clone     # macOS development only: patch a copy

incodex uninstall           # Remove integration; macOS restores the official app
incodex status
incodex doctor
incodex doctor --deep       # macOS: nested signing / entitlement / Gatekeeper evidence
incodex runtime             # Publish the bundled Runtime without modifying the official app
incodex open                # Incognito window, no patch
incodex open --mask         # Temporary sidebar name and offline avatar
incodex open --mask --name "Quiet Otter" --avatar ./avatar.png
incodex recover --transaction <id>  # available on macOS
inc update                  # Update Incodex through its install channel
incodex self-uninstall      # Remove the CLI; add --restore-app to remove app integration
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
incodex doctor --deep --json      # available on macOS
```

`brew install`, `curl … | bash`, and `cargo install` only put the command on PATH. On macOS, the command that changes `/Applications/ChatGPT.app` is `incodex install`. The Windows PowerShell installer likewise installs only the CLI; the later `incodex install` command registers Incodex's per-user Runtime without changing the Store package.

## Quick Launchers

<details>
<summary><strong>Raycast and Alfred setup (available on macOS)</strong></summary>

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
