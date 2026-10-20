---
name: incodex
description: Drive the Incodex CLI (`incodex` / `inc`) safely, including requested source-built experimental deployments. Dry-run before patching Codex; never use the TTY menu or invent live-install authorization. Not for ordinary source edits.
---

# Using Incodex from an agent

Incodex patches a locally installed Codex / ChatGPT desktop app, or opens an isolated window without patching. Treat `install` and `uninstall` as destructive.

## Rules

1. **Preview before you patch.** `incodex install --dry-run` and `incodex uninstall --dry-run` first. Show the plan. Only then offer the real run.
2. **The user runs the destructive command**, unless they explicitly asked you to do it in the current turn. "Install Incodex" is such an ask; "how does Incodex work" is not.
3. **Never use the interactive menu.** Bare `incodex` in a TTY opens arrows and numbers. Agents pass a subcommand. Non-TTY with no args prints help; do not feed that into a menu.
4. **Never invent flags.** Surface is `incodex --help` and `incodex <command> --help`. There is no user-facing `--live`. Non-TTY install/uninstall require `--yes`.
5. **Do not touch `~/.codex` session databases.** Isolation is under `~/.incodex/sessions/`.
6. **`install.sh` only installs the CLI.** It must not run `incodex install`.

## What answers which question

| The user asks | Command |
|---|---|
| "Is it installed?" | `incodex status --json` |
| "What is wrong?" | `incodex doctor --json` |
| "Install into ChatGPT.app" | `incodex install --dry-run`, then `incodex install` (add `--yes` when stdin is not a TTY) |
| "Take it out of Codex" | `incodex uninstall --dry-run`, then `incodex uninstall` |
| "Open incognito without patching" | `incodex open --dry-run`, then `incodex open` |
| "Update the button logic only" | `incodex runtime` |
| "Update this CLI" | `inc update --dry-run`, then `inc update` |
| "Remove the CLI" | `incodex self-uninstall --dry-run`, then `incodex self-uninstall` |

## Notes

- `inc` is the same program as `incodex`.
- Default target is `/Applications/ChatGPT.app`. `--clone` and `--app` are exceptions.
- After an official Codex upgrade, `incodex install` must run again on the current official package. `incodex runtime` does not fix that.
- `incodex open` must not patch asar or resign.
- Do not claim the machine keeps no traces.

## Update handoff

For CLI/Runtime/helper updates and deployments, follow the cross-platform handoff contract in `RUNTIME.md`. Check the actual registered helper and the executable it selects on both platforms; disk publication and equal versions do not prove activation. Preserve compatible fixed helpers and existing macOS authorization identities. Distinguish publication, registration, actual execution and any native migration or platform acceptance still required; do not use uninstall/reinstall as upgrade evidence.

## Source-built experimental deployment

Use this section when the user asks to run a local experimental build. Source synchronization alone does not satisfy a deployment request.

1. Integrate the latest `main` as the base while preserving the experiment's Runtime, native helpers, signing policy and regression tests. Read the required Bun version from `.bun-version`; use that stable toolchain.
2. Keep the product SemVer aligned across `Cargo.toml`, `package.json` and the Runtime manifest. Do not add an RC suffix to product versions solely to label a private experiment.
3. Name the candidate `INCODEX-V<major>.<minor>-<MAC|WIN>-<CAPABILITY>-RC<N>`. The version line comes from the product baseline; platform and capability identify the experiment. For example, product `1.3.1` with a newly authorized macOS AX experiment line uses `INCODEX-V1.3-MAC-AX-RC1`.
4. When the owner selects a new major/minor baseline line, start at RC1 after checking the local frozen-candidate store and private acceptance registry for prior use. Do not carry the previous line's RC counter forward. Within the same line, increment RC for any source, build-input or payload change, including a patch-version upgrade. Never reuse or overwrite a frozen candidate. Confirm the intended line if the owner's request is ambiguous.
5. Run the applicable regression suites. Commit the integrated source, bind the Runtime manifest to its source commit, rebuild the native CLI, then freeze the clean checkout and CLI/Runtime/native hashes. Keep candidate identity and run results in private reports, not `AGENTS.md` or this skill.
6. Publish the exact frozen CLI and its bundled Runtime through the existing native paths. Verify the installed CLI hash and selected Runtime assets against the candidate. Do not deploy a main-only Runtime over an experimental install, or use stable `inc update` to install a local candidate.
7. CLI/Runtime publication does not replace the official package's loader or signing. If "deploy locally" leaves host mutation unclear, publish only the CLI/Runtime and clarify the host-install scope. If host installation is requested and the current official package needs injection, preview with the frozen CLI and use the existing `install` transaction under the live-install authorization above. Respect normal-exit safety and preserve existing registered signing identity; do not uninstall, reset permissions or rotate identity merely to deploy an update.

A local candidate is not a public tag or GitHub Release. Keep public publication behind the separate release authorization and runbook.
