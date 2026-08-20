---
name: bugs
description: Incodex incident catalog for installer, signing, asar, session burn, and CLI confirmation. Use when a symptom or diff touches those surfaces. Not for generic review, release-note copy, or unrelated docs.
---

# Incodex bug patterns

Read `AGENTS.md` first. Load only the shapes the diff actually touches. A fix ships with a failing regression that the pre-fix code would fail.

| # | Shape | Where it shows up |
|---|---|---|
| 1 | Tests written after the code to match it | Any change |
| 2 | Destructive CLI without a plan / `--yes` | `src/cli.ts`, `src/confirm.ts`, `src/install.ts` |
| 3 | `install.sh` patches Codex | `install.sh` |
| 4 | Installer spawns `bun` to rebuild runtime | `src/install.ts`, `src/packaged-runtime.ts` |
| 5 | `--deep` signs vendor CUA sidecars | `src/codesign.ts` |
| 6 | Fake OpenAI Team ID or hidden IPC proxy | signing, runtime IPC |
| 7 | Writes or deletes `~/.codex` session DBs | session / forensics |
| 8 | `open` patches asar or resigns | `src/open-incognito.ts` |
| 9 | Close does not burn the isolated home | `incodex-safe-home`, `waitAndBurn` |
| 10 | Help or status regex matches a temp path | `src/help.ts`, tests with `incodex-` in tmp names |

### 1. Tests written after the code

Write the failing repro first. A test added to match already-written output is not a regression.

### 2. Destructive CLI without a plan

Official `install` / `uninstall` print a plan. TTY asks once. Non-TTY requires `--yes`. `--dry-run` and `--clone` do not ask. Do not teach `--live` / `--confirm-live` as the user language.

### 3. `install.sh` patches Codex

The script verifies `SHA256SUMS` and puts `incodex` / `inc` on PATH. It never runs `incodex install`.

### 4. Installer rebuilds runtime with bun

Runtime artifacts come from committed `dist/`. The installer must not `spawn bun src/build-runtime.ts`.

### 5. CUA sidecars get adhoc-signed

Stash vendor Computer Use helpers, sign the rest, restore, then outer `signOne`. Do not `--deep` them.

### 6. Fake Team ID

Appshot dies after asar/plist changes. Do not fake `2DC432GLL2` or proxy official ChatGPT IPC.

### 7. Touching `~/.codex` sessions

Copy only `auth.json` and `config.toml` into the isolated home. Never delete or rewrite official session DBs.

### 8. `open` mutates the app

`incodex open` spawns the official binary with `--user-data-dir`, `CODEX_HOME`, `CODEX_ELECTRON_USER_DATA_PATH`, an exact loopback `--remote-debugging-port`, and matching `--remote-allow-origins`. It injects the shared `dist/incodex-inject.js` over CDP and verifies the button/banner. No asar, clone, or codesign.

### 9. Leftover session dirs

Burn must retry late writers. A leftover whose name is not the session id is not burned. `owner.json` missing is not enough to delete a random folder.

### 10. Tests matching the wrong string

Help assertions must not match a temp directory named `incodex-install-…`. Pin the exact help corpus, not a loose `incodex` substring.
