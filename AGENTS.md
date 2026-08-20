# Incodex Agent Guide

This file is the shared source of truth for any AI agent working on this repo (Claude Code, Codex, Grok, etc.). `CLAUDE.md` is a symlink to this file. Put machine-specific or personal overrides in `AGENTS.local.md` / `CLAUDE.local.md`; both are gitignored.

## Project

Incodex adds a Chrome-style incognito window to a locally installed OpenAI Codex desktop app (`ChatGPT.app`, `com.openai.codex`). It is an unofficial add-on. The installer patches the official bundle. `incodex open` is a second launch path: it does not patch or resign the official app, but that isolated window still gets the hat-glasses control and banner by injecting the same `inject.js` over a localhost debug port.

Safety rules matter more than speed. Treat installer, signing, session cleanup, and IPC as the dangerous surface.

## Product Direction

Users launch the official Codex icon as usual. After `incodex install`, a hat-glasses control sits left of Search. Click or `Shift+Command+N` opens a second isolated Codex window: same login, language, and base settings; no old chats; close burns that temp home.

`incodex open` is the other launch path: spawn the official binary with an isolated home, do not copy/patch/resign the official app, then inject the **same** `inject.js` (hat-glasses + banner) through Chrome DevTools Protocol. CDP is not the Dock / `install` entry.

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
- Do not add Overlay, an independent Session Agent, LaunchAgent auto-repair, runtime pubkeys, or Homebrew core. Own tap is `daftAI2026/homebrew-tap`; bump it from `release.yml`. Do not open a Homebrew/homebrew-core PR.
- Do not use CDP as the everyday Dock / `install` launch path. `incodex open` may start the official binary with `--remote-debugging-port` on `127.0.0.1` and inject `dist/incodex-inject.js`. Do not clone the official app for `open`. Do not copy AGPL injector scripts.
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
- `src/cli.ts` / `src/parse-cli.ts` are the TypeScript golden/reference CLI. Keep their public language aligned with the native implementation; do not put asar or codesign logic in either router.
- `src/install.ts` / `src/uninstall.ts` / `src/recover.ts` are the TypeScript reference mutation paths while the compatibility window remains open.
- `crates/incodex-cli` is the native CLI: `parse.rs` owns its command language, while `install.rs` and `open.rs` dispatch dangerous operations through the lower crates.
- `crates/incodex-transaction` owns native mutation locks, durable journals, rollback, and recovery. `crates/incodex-asar` and `crates/incodex-macos` own ASAR and macOS signing/plist mechanics.
- `crates/incodex-core/src/session.rs` owns native `open` session create/burn; it must stay behaviorally aligned with `src/runtime/incodex-safe-home.cts` without sharing language-specific code.
- `src/packaged-runtime.ts` reads committed `dist/` (or `INCODEX_DIST`). The installer must not `spawn bun src/build-runtime.ts`.
- `src/runtime/*.cts` is Electron-side source. `bun run build:runtime` emits portable `dist/*.cjs` with `__dirname`, never a machine-absolute path.
- `src/runtime/incodex-loader.cts` is the only file that belongs in official asar. Everything else loads from `~/.incodex/runtime/` after hash check and fail-opens to official main.
- `install.sh` installs the CLI binary only. It must verify `SHA256SUMS` and must not run `incodex install`.
- `docs/` is gitignored local research. Do not commit it. The Native CLI integration section below is the committed release boundary.
- `.claude/skills/` is the agent skill tree. `.agents/skills/<name>` must stay a symlink to `../../.claude/skills/<name>`.

## Native CLI integration

Rust workspace now lives on `main`. The migration passed `tests/cli-golden.test.ts`, same-fixture TypeScript/Rust comparisons, the 10 MB size gate, the 50 ms product cold-start probe, and manual TTY/open verification. Stable release assets still come from the Bun-compiled TypeScript CLI until the compatibility cutover PR; source integration is not distribution cutover.

### Branching and TDD

1. New Rust CLI PRs target `main`. One review-sized change per PR.
2. Each behavior change starts with a failing `cargo test` repro commit, followed by the implementation commit. Do not land tests and code in the same first commit.
3. Keep the TypeScript CLI as the golden/reference implementation until the compatibility release has shipped and the legacy window closes. Do not rewrite product language independently in Rust.
4. Rust `install` / `uninstall` / `recover` / `open` / `status` / `doctor` must remain aligned with `tests/cli-golden.test.ts` and the same-fixture parity suite.

### Runtime boundary

- Electron Runtime stays TypeScript (`src/runtime/*.cts` → `dist/*.cjs`) and is still built by Bun; Rust embeds committed `dist/` artifacts.
- Incognito-window hover (hat-glasses → circle-x) is Runtime, not Rust UI code.
- `open` uses the official binary plus an isolated Chromium/CODEX_HOME pair and localhost CDP injection. It must not patch ASAR, clone, or re-sign the app.
- Do not add ratatui, cursive, crossterm, or an AGPL ASAR crate. The native menu is the existing numbered/arrow UI implemented directly with termios.

### Release cutover

1. The compatibility release will build Rust into the stable `incodex-darwin-arm64` / `incodex-darwin-x64` names.
2. That release also keeps explicitly named legacy Bun assets for one version cycle. `install.sh` and the own Homebrew tap continue selecting only the stable names; do not add a Rust/Bun selector or fallback.
3. A later release stops publishing new legacy assets. Never delete old release assets that remain useful for rollback.
4. Until the compatibility cutover PR merges, `install.sh`, `daftAI2026/homebrew-tap`, and `release.yml` still distribute the Bun-compiled CLI. Do not open a Homebrew/homebrew-core PR.

### Rust commands

```bash
cargo run -p incodex-cli -- --help
cargo run -p incodex-cli -- open --dry-run
cargo test --workspace --release
```

Do not add ratatui, cursive, crossterm, or an AGPL asar crate. The native menu is the existing numbered/arrow UI implemented directly with termios.

## Commands

```bash
bun install --frozen-lockfile
bun link
bun run check
bun run build:runtime
bun src/cli.ts --help
bun src/cli.ts install --dry-run
INCODEX_DEV_HOT=1 bun run deploy:runtime
cargo test --workspace --release
```

Public docs use `incodex` / `inc`. Use `bun src/cli.ts` only inside this repo.

## Critical Safety Rules

- Official install/uninstall default to `/Applications/ChatGPT.app`. That is intentional. Confirm on TTY; require `--yes` without a TTY.
- Never write a second installer that shells out to `install --live --confirm-live`.
- Only sign what must be signed. Leave official CUA sidecars official.
- Do not enable required GitHub reviews. Do not force-push `main`.
- Pin GitHub Actions to a 40-character commit SHA with a version comment: `uses: owner/repo@<sha> # vX.Y.Z`. Do not leave floating `@v4` tags.
- Official CLI packages are git tags `vX.Y.Z`. Follow `.claude/skills/release-flow/SKILL.md`, then `.claude/skills/release-notes/SKILL.md`. Do not `gh release create` and do not turn `generate_release_notes` back on.
- Route Electron Runtime session create/burn through `src/runtime/incodex-safe-home.cts` and native CLI session create/burn through `crates/incodex-core/src/session.rs`. Keep their safety contract aligned; do not fork a third implementation.
- `incodex open` must not patch asar, resign, or clone the official app. It spawns the official binary with `--user-data-dir`, `CODEX_HOME`, `CODEX_ELECTRON_USER_DATA_PATH`, and a localhost `--remote-debugging-port`, then injects the shared `inject.js`.
- A second instance needs that Chromium user-data-dir pair. `CODEX_HOME` alone is swallowed by SingletonLock.

## Working Rules

- Tests first. Add a failing test that states the bug or contract, then implement until it passes. Do not write tests to match already-written code.
- If you touch `src/runtime` or `src/build-runtime.ts`, run `bun run build:runtime` and commit matching `dist/` files.
- Keep Chinese and English user-facing copy in `incognito-copy.ts` together.
- One review-sized change per PR. Open the PR and merge when CI is green unless the user says otherwise. Rust CLI PRs target `main`.
- Do not add AI attribution trailers to commits.
