# Contributing

Incodex modifies a locally installed Electron app. Treat installer, signing,
session cleanup, and IPC as the dangerous surface.

## Requirements

- macOS for install / uninstall / codesign work
- [Bun](https://bun.sh) **1.3.14** (see `.bun-version`)
- A local Codex / ChatGPT desktop app only if you are running install tests
- [rustup](https://rustup.rs/) for native CLI work; `rust-toolchain.toml`
  installs and selects the pinned compiler

## Setup

```bash
git clone https://github.com/daftAI2026/incodex.git
cd incodex
bun install --frozen-lockfile
bun run check
```

The package no longer exposes a Bun CLI entry point. Use the native Rust binary for product commands; Bun remains responsible for Electron Runtime build and test tooling.

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

## Native CLI

Rust CLI source is on `main`. The migration gates are complete, and v0.3.1 published the native Rust CLI under the stable asset names. It is the only product CLI; legacy v1 fixture/proof coverage is native Rust and the remaining TypeScript source belongs to Electron Runtime/forensics.

```bash
git fetch origin
git checkout main
git pull --ff-only
git checkout -b feat/your-change
# Rust CLI PRs use base `main`.
```

Each behavior change starts with a failing `cargo test` repro, followed by the implementation. Run `cargo test --workspace --release`; keep the native CLI contract and legacy fixture/proof suites green. The retired TypeScript CLI and mutation implementation are not test oracles.

```bash
cargo run -p incodex-cli -- --help
cargo run -p incodex-cli -- status --json
cargo test --workspace --release
```

## Rust toolchain upgrades

`rust-toolchain.toml` is the single compiler source of truth for local builds,
CI, and release builds. The workspace `rust-version` records the supported Rust
minor release for every crate; it is inherited by each crate manifest. Incodex
ships binaries and does not maintain a separate older-compiler compatibility
matrix, so the pinned minor is also the declared minimum.

Upgrade Rust in a dedicated PR. Change the exact channel in
`rust-toolchain.toml` and update `rust-version` when the pinned minor changes.
Then run:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --release --locked
bun run check
```

Do not create a tag or GitHub Release as part of a toolchain upgrade PR.

Do not add a TUI crate or an AGPL ASAR crate. Electron Runtime stays TypeScript and Bun-built; Rust embeds committed `dist/`. Incognito hover remains shared Runtime behavior, not a separate Rust UI implementation.

Release integration and release publication remain intentionally separate. The workflow cross-compiles Rust into the stable names; `install.sh` and the own tap consume only those names. New releases do not publish new legacy Bun CLI assets, while existing old Release assets remain untouched for rollback. Merging release code never authorizes creating a tag or Release.

## Pull requests

- One review-sized change per PR
- Rust CLI PRs use base `main`
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

The Rust parser owns the product command language. Do not restore a TypeScript router
or add a second installer around the native CLI.
