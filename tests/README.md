# Tests

Unit tests live next to the code they cover (`src/*.test.ts`). This directory
holds cross-cutting fixtures and engineering checks.

The native CLI contract tests are the product behavior source of truth. The
TypeScript product router and mutation implementation are retired; legacy disk
fixture/proof coverage lives in Rust and never starts a TypeScript CLI.

| path | purpose |
| --- | --- |
| `src/*.test.ts` | Electron Runtime, forensics, locale, compatibility, and IPC |
| `tests/supported-builds.test.ts` | UI adapter is not pinned to one Codex build |
| `tests/runtime-manifest.test.ts` | committed runtime metadata |
| `tests/native-cli-contract.test.ts` | guards the native-only product CLI boundary and legacy fixture scope |
| `tests/package-retirement.test.ts` | guards removal of the TypeScript package router while preserving Runtime/build scripts |
| `tests/legacy-retirement.test.ts` | guards removal of mutation/publisher sources and their active import/documentation graph |
| `tests/rust-workspace.test.ts` | MIT workspace, no TUI/AGPL crates, CI `cargo test` |
| `crates/incodex-cli/tests/probe.rs` | native help/version, size, and cold-start contract |
| `crates/incodex-cli/tests/readonly.rs` | native command help, JSON, status, doctor, and parse-error contract |
| `crates/incodex-cli/tests/native_contract.rs` | native mutation fixture and non-TTY progress contract |
| `crates/incodex-cli/tests/support/native_tty.rs` | native menu, confirmation, lifecycle, and TTY progress contract |
| `crates/incodex-cli/tests/legacy_typescript.rs` | frozen TypeScript v1 disk fixture reader; never starts a TypeScript CLI |

Native ASAR and transaction fixture coverage is in `crates/incodex-asar/tests/fixtures.rs`
and `crates/incodex-cli/tests/`; the Rust ASAR differential oracle still invokes the
repository's `@electron/asar` package. Privacy-forensics coverage is in `src/forensics.test.ts`.
