<!--
[INPUT]: 默认自动检查与macOS本地Security实签环境。
[OUTPUT]: 提供Runtime测试分层与显式本地实签验收命令。
[POS]: 测试导航；区分代码回归与环境相关验收。
[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
-->
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

## Runtime contract and OS integration

Main-entry VM contracts control directory notifications explicitly while running
the real permission responder and filesystem operations. Real OS watcher delivery
remains covered by the Node subprocess scenarios in
`src/runtime/permission-probe.test.ts`, including withheld-event negative control,
one-shot retirement and ordinary responder reuse. Neither suite substitutes for
the completed local host acceptance.

## Local certificate-signing acceptance

Default automated checks do not require certificate signing to work in a hosted
runner's Keychain environment. The real certificate-signing cases remain intact
as explicit local acceptance. They create their own temporary identities and
synthetic apps; they do not need a developer's existing certificate or modify the
installed ChatGPT app.

On a macOS development machine, run both commands when changing certificate
signing, component designated requirements, or signer-bound update registration:

```sh
INCODEX_LOCAL_SIGNING_ACCEPTANCE=1 bun test src/runtime/remote-key-compat.test.ts
cargo test -p incodex-cli --release --locked --test macos_signing_context --test macos_signing_registration -- --ignored
```

The Bun command enables one real certificate host-identity case and retains the
other six compatibility cases. The Rust command runs four ignored real-signing
cases: component/DR continuity, signature rejection, explicit-epoch identity
promotion, and rejection of signature/material downgrade. These assertions are
not executed by the default CI gate. Registration schema/CAS, signing policies,
private identity storage, ad-hoc signing, transactions and the other compatibility
cases remain in the default automated suites.

A passing default CI run does not prove certificate-signing compatibility on
every macOS version, TCC permission continuity, or a real official-app update.
Hosted-runner signing failures are not declared fixed by this separation. Never
upload a user's private signing assets or change system trust to make CI pass.
