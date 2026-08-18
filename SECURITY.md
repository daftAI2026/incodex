# Security Policy

Incodex patches a locally installed Codex / ChatGPT desktop app. A bug can
replace the official bundle, leak a temporary session, or send IPC from the
wrong frame. Please treat installer, signing, session cleanup, and IPC changes
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
- Codex / ChatGPT desktop version and build (`CFBundleShortVersionString`, `CFBundleVersion`)
- macOS version and architecture
- Whether the install was `--clone`, `--live`, or `--app`
- Steps to reproduce
- What an attacker could do

We will acknowledge the report and say whether it is in scope.

## Out of scope

- Bugs that only exist in the official OpenAI Codex app
- Asking us to preserve a valid OpenAI code signature after the asar changes
- Social-engineering a user into running `install --live`
