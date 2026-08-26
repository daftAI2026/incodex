---
name: incodex
description: Drive the Incodex CLI (`incodex` / `inc`) safely. Dry-run before patching Codex, never use the TTY menu from an agent, never invent --yes on live install unless the user asked this turn. Use before running incodex on a user's Mac. Not for editing Incodex source.
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
