# Incodex Agent Guide

This file is the shared source of truth for any AI agent working on this repo (Claude Code, Codex, Grok, etc.). `CLAUDE.md` is a symlink to this file. Put machine-specific or personal overrides in `AGENTS.local.md` / `CLAUDE.local.md`; both are gitignored.

## Project

Incodex adds a Chrome-style incognito window to a locally installed OpenAI Codex desktop app (`ChatGPT.app`, `com.openai.codex`). It is an unofficial add-on. The installer patches the official bundle; the CLI can also launch an isolated window without patching.

Safety rules matter more than speed. Treat installer, signing, session cleanup, and IPC as the dangerous surface.

## Product Direction

Users launch the official Codex icon as usual. After `incodex install`, a hat-glasses control sits left of Search. Click or `Shift+Command+N` opens a second isolated Codex window: same login, language, and base settings; no old chats; close burns that temp home. `incodex open` is the no-patch path.

### What Incodex Should Do

- Keep the official icon and bundle id. Do not invent a second app the user launches every day.
- Isolate incognito data under `~/.incodex/sessions/`. Do not write or delete `~/.codex` session databases.
- Copy only `auth.json` and `config.toml` into the isolated home.
- Burn on a normal close. Do not claim “no traces on the machine” unless forensics say so (`absolutePrivacyClaimAllowed()` is false).
- Default `incodex install` / `uninstall` to `/Applications/ChatGPT.app`. `--clone` and `--app` are exceptions.
- Keep `--help` and a TTY menu. Non-TTY with no args prints help. Destructive commands print a plan; TTY asks once; non-TTY requires `--yes`.
- Update Incodex-only code with `incodex runtime` (writes `~/.incodex/runtime/`, no resign). After an official Codex upgrade, the user must run `incodex install` again on the **current** official package.

### What Incodex Should Not Do

- Do not delete, archive, or rewrite `~/.codex` session DBs.
- Do not change the bundle id or force a re-login.
- Do not restore a valid OpenAI signature after asar changes. Appshot (智能快照) is a hard triangle; document it, do not fake Team ID `2DC432GLL2`.
- Do not sign vendor CUA sidecars. Stash them, `--deep` the rest, restore, then outer `signOne`.
- Do not default live-patch from `install.sh` / `bun link` / `incodex update`. Those only manage the CLI.
- Do not add Overlay, CDP-as-launcher, an independent Session Agent, LaunchAgent auto-repair, runtime pubkeys, or Homebrew core. P4 (own tap) is parked.
- Do not add `--yes` as a hidden alias that agents will hallucinate onto live patching beyond the documented flag. `--confirm-live` stays a hidden compat alias of `--yes`.
- Do not write tests after the implementation to match it. Write a failing repro first.

### Product Decision Filter

1. Does it belong to install, uninstall, runtime, open, status, doctor, recover, update, or self-uninstall?
2. Can a user preview it with `--dry-run` if it is destructive?
3. Does it touch `~/.codex` sessions, OpenAI signatures, or `/Applications/ChatGPT.app` without an explicit command?
4. Would “don’t do this” plus a README note be enough?

If the answer is no or unclear, decline or narrow.

## Repository Map

- `AGENTS.md` is the contract. `CLAUDE.md` must stay a symlink to it.
- `src/cli.ts` is the router: parse, menu, confirm, dispatch. Do not put asar or codesign logic here.
- `src/parse-cli.ts` is the only user-facing command language. One parser for `incodex`, `inc`, and `bun src/cli.ts`.
- `src/install.ts` / `src/uninstall.ts` / `src/recover.ts` mutate the app bundle.
- `src/packaged-runtime.ts` reads committed `dist/` (or `INCODEX_DIST`). The installer must not `spawn bun src/build-runtime.ts`.
- `src/runtime/*.cts` is Electron-side source. `bun run build:runtime` emits portable `dist/*.cjs` with `__dirname`, never a machine-absolute path.
- `src/runtime/incodex-loader.cts` is the only file that belongs in official asar. Everything else loads from `~/.incodex/runtime/` after hash check and fail-opens to official main.
- `install.sh` installs the CLI binary only. It must verify `SHA256SUMS` and must not run `incodex install`.
- `docs/` is gitignored local research. Do not commit it.

## Commands

```bash
bun install --frozen-lockfile
bun link
bun run check
bun run build:runtime
bun src/cli.ts --help
bun src/cli.ts install --dry-run
INCODEX_DEV_HOT=1 bun run deploy:runtime
```

Public docs use `incodex` / `inc`. Use `bun src/cli.ts` only inside this repo.

## Critical Safety Rules

- Official install/uninstall default to `/Applications/ChatGPT.app`. That is intentional. Confirm on TTY; require `--yes` without a TTY.
- Never write a second installer that shells out to `install --live --confirm-live`.
- Only sign what must be signed. Leave official CUA sidecars official.
- Do not enable required GitHub reviews. Do not force-push `main`.
- Pin GitHub Actions to a 40-character commit SHA with a version comment: `uses: owner/repo@<sha> # vX.Y.Z`. Do not leave floating `@v4` tags.
- Official CLI packages are git tags `vX.Y.Z`. The release workflow builds binaries and creates the GitHub Release with empty notes. Write the changelog by hand afterwards with `gh release edit`; do not `gh release create` and do not turn `generate_release_notes` back on.
- Route session create/burn through `src/runtime/incodex-safe-home.cts`. Do not copy those functions into the CLI.
- `incodex open` must not patch asar or resign. It spawns the official binary with `--user-data-dir`, `CODEX_HOME`, and `CODEX_ELECTRON_USER_DATA_PATH`.
- A second instance needs that Chromium user-data-dir pair. `CODEX_HOME` alone is swallowed by SingletonLock.

## Working Rules

- Tests first. Add a failing test that states the bug or contract, then implement until it passes. Do not write tests to match already-written code.
- If you touch `src/runtime` or `src/build-runtime.ts`, run `bun run build:runtime` and commit matching `dist/` files.
- Keep Chinese and English user-facing copy in `incognito-copy.ts` together.
- One review-sized change per PR. Open the PR and merge when CI is green unless the user says otherwise.
- Do not add AI attribution trailers to commits.
