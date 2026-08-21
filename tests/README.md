# Tests

Unit tests live next to the code they cover (`src/*.test.ts`). This directory
holds cross-cutting fixtures and engineering checks.

The native CLI contract tests are the product behavior source of truth. The
legacy TypeScript surface is exercised only through frozen disk fixtures.

| path | purpose |
| --- | --- |
| `src/*.test.ts` | unit tests (CLI, backup identity, symlink, ASAR, IPC) |
| `tests/supported-builds.test.ts` | UI adapter is not pinned to one Codex build |
| `tests/runtime-manifest.test.ts` | committed runtime metadata |
| `tests/native-cli-contract.test.ts` | guards the native-only product CLI boundary and frozen legacy fixture scope |
| `tests/rust-workspace.test.ts` | MIT workspace, no TUI/AGPL crates, CI `cargo test` |
| `crates/incodex-cli/tests/probe.rs` | native help/version, size, and cold-start contract |
| `crates/incodex-cli/tests/readonly.rs` | native command help, JSON, status, doctor, and parse-error contract |
| `crates/incodex-cli/tests/native_contract.rs` | native mutation fixture and non-TTY progress contract |
| `crates/incodex-cli/tests/support/native_tty.rs` | native menu, confirmation, lifecycle, and TTY progress contract |
| `crates/incodex-cli/tests/legacy_typescript.rs` | frozen TypeScript v1 disk fixture reader; never starts the old CLI |

ASAR fixture coverage is in `src/asar-unpack.test.ts` and `src/asar-upgrade.test.ts`.
Install-fault and privacy-forensics coverage is in `src/install-fault.test.ts` and `src/forensics.test.ts`.
