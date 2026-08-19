# Tests

Unit tests live next to the code they cover (`src/*.test.ts`). This directory
holds cross-cutting fixtures and engineering checks.

| path | purpose |
| --- | --- |
| `src/*.test.ts` | unit tests (CLI, backup identity, symlink, ASAR, IPC) |
| `tests/supported-builds.test.ts` | UI adapter is not pinned to one Codex build |
| `tests/runtime-manifest.test.ts` | committed runtime metadata |
| `tests/cli-golden.test.ts` | freeze CLI stdout/stderr/exit/JSON/TTY for the Rust port |

ASAR fixture coverage is in `src/asar-unpack.test.ts` and `src/asar-upgrade.test.ts`.
Install-fault and privacy-forensics cases still belong here once they exist.
