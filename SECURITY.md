# Security Policy

Incodex opens isolated windows in a locally installed Codex / ChatGPT desktop
app. Its optional integration patches the app bundle on macOS and registers
a per-user launcher integration on Windows without modifying the Store package.
A bug can damage an installation, leak a temporary session, or send IPC from
the wrong frame. Treat installer, signing, session cleanup, and IPC changes
as security-sensitive.

## Supported versions

Only the latest tagged release is supported. Development commits on `main` are
not a support channel.

## Reporting a vulnerability

Do **not** open a public issue for an exploitable bug.

Use [GitHub Security Advisories](https://github.com/daftAI2026/incodex/security/advisories/new)
so the report stays private until a fix is ready.

Include:

- Incodex version or commit
- Codex / ChatGPT desktop version and build (macOS bundle version/build or Windows Store package full name)
- Operating system version and architecture
- Command and arguments used, such as `open`, `install`, or `update`; include `--clone` or `--app` if used on macOS
- Steps to reproduce
- What an attacker could do

We will acknowledge the report and say whether it is in scope.

## Out of scope

- Bugs that only exist in the official OpenAI Codex app
- Asking us to preserve a valid OpenAI code signature after the asar changes
- Social-engineering a user into approving an explicitly requested installation
