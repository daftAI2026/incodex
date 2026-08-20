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
- `src/cli.ts` is the router: parse, menu, confirm, dispatch. Do not put asar or codesign logic here.
- `src/parse-cli.ts` is the only user-facing command language. One parser for `incodex`, `inc`, and `bun src/cli.ts`.
- `src/install.ts` / `src/uninstall.ts` / `src/recover.ts` mutate the app bundle.
- `src/packaged-runtime.ts` reads committed `dist/` (or `INCODEX_DIST`). The installer must not `spawn bun src/build-runtime.ts`.
- `src/runtime/*.cts` is Electron-side source. `bun run build:runtime` emits portable `dist/*.cjs` with `__dirname`, never a machine-absolute path.
- `src/runtime/incodex-loader.cts` is the only file that belongs in official asar. Everything else loads from `~/.incodex/runtime/` after hash check and fail-opens to official main.
- `install.sh` installs the CLI binary only. It must verify `SHA256SUMS` and must not run `incodex install`.
- `docs/` is gitignored local research. Do not commit it. The Native CLI experiment section below is the committed copy of that plan.
- `.claude/skills/` is the agent skill tree. `.agents/skills/<name>` must stay a symlink to `../../.claude/skills/<name>`.

## Native CLI experiment

Shipped releases are the TypeScript CLI on `main`. Native Rust CLI work lives on `exp/rust-cli` until it is proven. Do not send crate PRs at `main`.

### Branching

1. `main` stays the product. Homebrew, `install.sh`, and `release.yml` still ship the Bun-compiled binary.
2. `exp/rust-cli` is the integration branch. It started from `main` after the golden CLI contract (PR #66, `tests/cli-golden.test.ts`).
3. Open crate PRs with `--base exp/rust-cli`. One step per PR. Merge that PR into `exp/rust-cli` when CI is green. Each crate PR starts with a failing `cargo test` repro commit, then the implementation commit that makes those tests pass. Do not land tests and code in the same first commit.
4. When `main` moves (runtime, hover, golden tests), merge `main` into `exp/rust-cli`. Rebase topic branches onto `exp/rust-cli`, not onto `main`.
5. Merge `exp/rust-cli` into `main` only after all of these hold:
   - native `install` / `uninstall` / `recover` / `open` / `status` / `doctor` match `tests/cli-golden.test.ts`
   - same-fixture comparison of plan output, asar, manifest, runtime hash, codesign, install / uninstall / recover
   - uncompressed binary ≤ 10 MB (10–15 MB needs a written reason); record `--version` cold start against 50 ms
6. Until that merge, do not point `install.sh`, the Homebrew tap, or `release.yml` at a Rust asset.

### What stays on `main`

- TypeScript CLI (`src/cli.ts`, `src/parse-cli.ts`, install / recover / open).
- Electron Runtime (`src/runtime/*.cts` → `dist/*.cjs`). Bun still builds it; Rust will embed `dist/` later.
- Incognito-window hover (hat-glasses → circle-x). That is Runtime, not a crate.

### Steps on `exp/rust-cli` (one PR each)

1. Golden CLI — already on `main` (#66). Align Rust to those tests. Do not rewrite the product language.
2. Workspace + size probe: `crates/incodex-cli`, `incodex-core`, `incodex-transaction`, `incodex-macos`, `incodex-runtime-bundle`, `incodex-asar`. MIT. No AGPL asar crate. No TUI crate. `cargo test` on macOS. Measure uncompressed size and `--version` cold start.
3. Read-only `status` / `doctor` aligned to golden.
4. `open` (no asar, no resign, no clone). CleanupResult: only say removed when the directory is gone. Product follow-up: CDP-inject the shared `inject.js` into that window (`docs/rust-cli/方案.md`, `open` CDP section).
5. Transaction v2 in Rust only. Do not rebuild journal schema v2 / fsync in TypeScript.
6. Native ASAR: own MIT crate; `@electron/asar` is a test oracle only.
7. `install` / `uninstall` / `recover`. After this, Release can stop attaching Bun-compiled binaries.
8. Parallel comparison, native numbered/arrow TTY menu, then merge `exp/rust-cli` to `main` after manual UI verification.

### Commands on `exp/rust-cli`

```bash
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
# On exp/rust-cli only:
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
- Route session create/burn through `src/runtime/incodex-safe-home.cts`. Do not copy those functions into the CLI.
- `incodex open` must not patch asar, resign, or clone the official app. It spawns the official binary with `--user-data-dir`, `CODEX_HOME`, `CODEX_ELECTRON_USER_DATA_PATH`, and a localhost `--remote-debugging-port`, then injects the shared `inject.js`.
- A second instance needs that Chromium user-data-dir pair. `CODEX_HOME` alone is swallowed by SingletonLock.

## Working Rules

- Tests first. Add a failing test that states the bug or contract, then implement until it passes. Do not write tests to match already-written code.
- If you touch `src/runtime` or `src/build-runtime.ts`, run `bun run build:runtime` and commit matching `dist/` files.
- Keep Chinese and English user-facing copy in `incognito-copy.ts` together.
- One review-sized change per PR. Open the PR and merge when CI is green unless the user says otherwise. Rust crate PRs target `exp/rust-cli` until that branch merges to `main`.
- Do not add AI attribution trailers to commits.
