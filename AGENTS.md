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
- Keep `inc update` as the public stable-update entry point and the only command named in update notices. Route by the running binary's install channel: Homebrew performs bounded `brew update` (metadata refresh is best-effort) followed by bounded `brew upgrade incodex` (failure output is preserved), reports the installed version through `brew list` with the public CLI probe as fallback, and clears the notice cache; script installs fetch the latest stable release, run its pinned installer, verify the installed binary version, and retain the main-branch compatibility-installer fallback; source checkouts receive explicit `git pull` / `cargo install` guidance. The menu launches update-notice refresh in a detached internal worker so the next invocation can consume the cache even when the current menu exits immediately; it checks the stable release first, consults the Homebrew formula only when that release is newer, and clears stale notices when refresh cannot prove an update. Both mutable update paths use a channel-stable transaction target lock so concurrent updates cannot cross generations. Regression coverage lives in `crates/incodex-cli/tests/update.rs` and `crates/incodex-cli/tests/support/update_menu.rs`.

### What Incodex Should Not Do

- Do not delete, archive, or rewrite `~/.codex` session DBs.
- Do not change the bundle id or force a re-login.
- Do not restore a valid OpenAI signature after asar changes. Appshot (智能快照) is a hard triangle; document it, do not fake Team ID `2DC432GLL2`.
- Do not sign vendor CUA sidecars. Stash them, `--deep` the rest, restore, then outer `signOne`.
- Do not default live-patch from `install.sh` / `inc update`. Those only manage the CLI.
- Do not add Overlay, an independent Session Agent, LaunchAgent auto-repair, runtime pubkeys, or Homebrew core. Own tap is `daftAI2026/homebrew-tap`; bump it from `release.yml`. Do not open a Homebrew/homebrew-core PR.
- Do not use CDP as the everyday Dock / `install` launch path. `incodex open` may start the official binary with `--remote-debugging-port` on `127.0.0.1` and inject `dist/incodex-inject.js`. Do not clone the official app for `open`. Do not copy AGPL injector scripts.
- Do not add hidden confirmation aliases beyond the native parser's tested compatibility surface.
- Do not write tests after the implementation to match it. Write a failing repro first.

### Product Decision Filter

1. Does it belong to install, uninstall, runtime, open, status, doctor, recover, update, or self-uninstall?
2. Can a user preview it with `--dry-run` if it is destructive?
3. Does it touch `~/.codex` sessions, OpenAI signatures, or `/Applications/ChatGPT.app` without an explicit command?
4. Would “don’t do this” plus a README note be enough?

If the answer is no or unclear, decline or narrow.

## Repository Map

- `AGENTS.md` is the contract. `CLAUDE.md` must stay a symlink to it.
- The TypeScript product router, parser, mutation implementation, and old Runtime publishers have been retired. Rust owns the product CLI and native mutation path; legacy TypeScript v1 disk compatibility is limited to the Rust `legacy_typescript.rs` reader and `legacy_proof.rs` safety fixtures.
- `crates/incodex-cli` is the native CLI: `parse.rs` owns its command language, while `install.rs` and `open.rs` dispatch dangerous operations through the lower crates.
- `crates/incodex-transaction` owns native mutation locks, durable journals, rollback, and recovery. `crates/incodex-asar` and `crates/incodex-macos` own ASAR and macOS signing/plist mechanics.
- `crates/incodex-core/src/session.rs` owns native `open` session create/burn; it must stay behaviorally aligned with `src/runtime/incodex-safe-home.cts` without sharing language-specific code.
- `src/build-runtime.ts` builds committed `dist/` artifacts. Native `incodex-runtime-bundle` publishes them; the installer must not rebuild Runtime by spawning Bun.
- `src/runtime/*.cts` is Electron-side source. `bun run build:runtime` emits portable `dist/*.cjs` with `__dirname`, never a machine-absolute path.
- `src/runtime/incodex-loader.cts` is the only file that belongs in official asar. Everything else loads from `~/.incodex/runtime/` after hash check and fail-opens to official main.
- `install.sh` installs the CLI binary only. It must verify `SHA256SUMS` and must not run `incodex install`.
- `docs/` is gitignored local research. Do not commit it. The Native CLI integration section below is the committed release boundary.
- `.claude/skills/` is the agent skill tree. `.agents/skills/<name>` must stay a symlink to `../../.claude/skills/<name>`.

## Native CLI integration

Rust workspace now lives on `main`. The migration passed the native Rust contract suites, the frozen legacy fixture reader, the 10 MB size gate, the 50 ms product cold-start probe, and manual TTY/open verification. The v0.3.1 compatibility release published the native Rust CLI under the stable asset names. The Rust CLI is the sole product CLI. Bun remains responsible for building the embedded Electron Runtime and its checks, not for producing or launching a shipped TypeScript CLI binary.

The native Rust contract tests are the product behavior source of truth. The remaining TypeScript source is limited to Electron Runtime and forensics; legacy v1 fixture/proof material lives in Rust and must not be launched as a product CLI or used as an output oracle.

### Branching and TDD

1. New Rust CLI PRs target `main`. One review-sized change per PR.
2. Each behavior change starts with a failing `cargo test` repro commit, followed by the implementation commit. Do not land tests and code in the same first commit.
3. The retired TypeScript product router and mutation implementation must not return. Keep legacy v1 compatibility only in the named Rust fixture/proof modules; do not create new TypeScript parity paths.
4. Rust `install` / `uninstall` / `recover` / `open` / `status` / `doctor` are the product paths. Preserve their proven safety contracts without creating new TypeScript parity work.

### Windows adaptation boundary

- Windows support is under development and is not a public product claim until the corresponding behavior has passed real Windows app and lifecycle verification.
- The Windows Rust boundary exposes parsing, help, version reporting, and the native `open` path. Every other mutating product command fails closed before creating Incodex state until that command lands behind its own failing Windows test.
- Windows CI intentionally tests the supported `incodex-core` and `incodex-cli` library/binary surface. The ASAR mutation, transaction, Runtime publishing, and macOS integration crates remain outside that target until a real Windows responsibility exists; do not add placeholder Windows implementations merely to make `cargo test --workspace` compile there.
- Windows `open` is one trust pipeline: discover the current user's `OpenAI.Codex` Store package without hardcoded install paths; create a current-user-only session that rejects reparse ancestry, copies only `auth.json` / `config.toml`, and persists directory plus owner-process identity; launch suspended into a kill-on-close Job Object; prove the IPv4 loopback CDP listener belongs to that Job; inject the committed shared Runtime; then burn normal sessions or sweep proven-dead owners while retaining every uncertain cleanup state.
- Preserve the shared parser and command names. Add Windows behavior inside the Rust product CLI, one review-sized capability at a time; do not create a second CLI or Runtime.
- Treat Store/AppX installation, Authenticode, reparse points, ACLs, process trees, and updater behavior as evidence-driven Windows boundaries. Do not implement Windows `install` / `uninstall` without separate repository-owner approval.

### Runtime boundary

- Electron Runtime stays TypeScript (`src/runtime/*.cts` → `dist/*.cjs`) and is still built by Bun; Rust embeds committed `dist/` artifacts.
- Incognito-window hover (hat-glasses → circle-x) is Runtime, not Rust UI code.
- `open` uses the official binary plus an isolated Chromium/CODEX_HOME pair and localhost CDP injection. It must not patch ASAR, clone, or re-sign the app.
- Do not add ratatui, cursive, crossterm, or an AGPL ASAR crate. The native menu is the existing numbered/arrow UI implemented directly with termios.

### Release cutover

1. `release.yml` cross-compiles Rust into the stable `incodex-darwin-arm64` / `incodex-darwin-x64` names, then publishes only those binaries and `SHA256SUMS`.
2. `install.sh` and the own Homebrew tap continue selecting only the stable names; do not add a Rust/Bun selector or fallback.
3. New releases do not publish legacy Bun CLI assets. Never delete old Release assets that remain useful for rollback.
4. Merging release-pipeline code is not permission to create a tag or GitHub Release. Obtain explicit repository-owner confirmation immediately before publishing. Do not open a Homebrew/homebrew-core PR.

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
bun run check
bun run build:runtime
INCODEX_DEV_HOT=1 bun run deploy:runtime
cargo test --workspace --release
```

Public docs use the native `incodex` / `inc` binaries. Bun is retained for Electron Runtime build/checks, not as a CLI entry point.

## Critical Safety Rules

- Official install/uninstall default to `/Applications/ChatGPT.app`. That is intentional. Confirm on TTY; require `--yes` without a TTY.
- Never write a second installer or restore a TypeScript router around the native CLI.
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
