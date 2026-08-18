# Contributing

Incodex modifies a locally installed Electron app. Treat installer, signing,
session cleanup, and IPC as the dangerous surface.

## Requirements

- macOS for install / uninstall / codesign work
- [Bun](https://bun.sh) **1.3.14** (see `.bun-version`)
- A local Codex / ChatGPT desktop app only if you are running install tests

## Setup

```bash
git clone https://github.com/daftAI2026/incodex.git
cd incodex
bun install --frozen-lockfile
bun run check
```

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

## Pull requests

- One review-sized change per PR
- Do not target `/Applications/ChatGPT.app` unless the change is specifically
  about `--live`
- Do not commit `node_modules`
- If you touch `src/runtime` or `src/build-runtime.ts`, run `bun run build:runtime`
  and commit the matching `dist/` files
- Keep Chinese and English user-facing copy in `incognito-copy.ts` together

## Destructive CLI

`install`, `uninstall`, and `recover` require an explicit target:

```text
--clone | --live | --app <path>
```

`--live` also requires `--confirm-live`. Do not reintroduce a default that
points a destructive command at `/Applications/ChatGPT.app`.
