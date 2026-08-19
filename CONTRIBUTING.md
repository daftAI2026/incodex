# Contributing

Incodex modifies a locally installed Electron app. Treat installer, signing,
session cleanup, and IPC as the dangerous surface.

## Requirements

- macOS for install / uninstall / codesign work
- [Bun](https://bun.sh) **1.3.14** (see `.bun-version`)
- A local Codex / ChatGPT desktop app only if you are running install tests
- Rust / Cargo only if you are working on the native CLI experiment (`exp/rust-cli`)

## Setup

```bash
git clone https://github.com/daftAI2026/incodex.git
cd incodex
bun install --frozen-lockfile
bun link
bun run check
```

`bun link` puts `incodex` and `inc` on PATH. They run the same `src/cli.ts`.

`check` runs typecheck, lint, unit tests, and a deterministic `dist` rebuild.

## Scripts

| script | purpose |
| --- | --- |
| `bun run typecheck` | `tsc` on TypeScript and runtime CJS |
| `bun run lint` | Biome on `src` |
| `bun run test` | `bun test` |
| `bun run audit` | `bun audit` |
| `bun run build:runtime` | write committed `dist/` artifacts |
| `bun run check:dist` | rebuild `dist/` and fail on drift |
| `bun run check` | all of the above except audit |
| `bun run deploy:runtime` | copy runtime to `~/.incodex` (dev only) |

`build:runtime` must not write `~/.incodex`. Use `INCODEX_DEV_HOT=1` plus
`deploy:runtime` when you want a hot-loaded override.

## Runtime JavaScript

Runtime sources are TypeScript CommonJS (`src/runtime/*.cts`). `build:runtime`
runs `tsc` to emit portable CJS into `dist/*.cjs`. The emitted files must use
`__dirname` at runtime and must not contain a machine-specific absolute path.

## Native CLI experiment

Shipped CLI binaries still come from `main` (Bun compile). Native Rust work is
on `exp/rust-cli`. Do not open crate PRs against `main`.

```bash
git fetch origin
git checkout exp/rust-cli
git pull --ff-only
git checkout -b feat/your-step
# Open the PR with base exp/rust-cli, not main.
```

When `main` has moved, merge it into `exp/rust-cli` before starting a new step.
Merge `exp/rust-cli` into `main` only after the native CLI matches
`tests/cli-golden.test.ts` and a same-fixture comparison of install / uninstall /
recover / open has passed. Until then, do not change `install.sh`, the Homebrew
tap, or `release.yml` to ship a Rust binary.

`cargo test --workspace --release` is the Rust check on that branch. Do not add
a TUI crate or an AGPL asar crate. Electron Runtime stays TypeScript. The
incognito-window hover (hat → close icon) is Runtime work on `main`, not a crate.

The step list is in `AGENTS.md` (Native CLI experiment). One step per PR into
`exp/rust-cli`. Each crate PR starts with a failing `cargo test` repro, then
the implementation.

## Pull requests

- One review-sized change per PR
- Rust crate PRs use base `exp/rust-cli` until that branch merges to `main`
- Do not target `/Applications/ChatGPT.app` unless the change is specifically
  about installing into the official app
- Do not commit `node_modules`
- If you touch `src/runtime` or `src/build-runtime.ts`, run `bun run build:runtime`
  and commit the matching `dist/` files
- Keep Chinese and English user-facing copy in `incognito-copy.ts` together

## Destructive CLI

`incodex install` and `incodex uninstall` default to `/Applications/ChatGPT.app`.
That is the product path. `--clone` patches a copy;
`--app <path>` patches a specific bundle.

On a terminal, official install/uninstall prints a plan and asks once.
Without a terminal they require `--yes`. `--dry-run` and `--clone` do not ask.

`--live` and `--confirm-live` still parse as hidden aliases (`--live` means the
official app, `--confirm-live` means `--yes`). Do not document them as the
user-facing command.

Do not add a second installer that shells out to the old flag string.
