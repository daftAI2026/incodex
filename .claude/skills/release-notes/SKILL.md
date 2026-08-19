---
name: release-notes
description: Publish handwritten Incodex release notes for an existing vX.Y.Z tag. Bilingual Changelog / 更新日志, gh release edit, never create. Use only when the user asks to write or publish release notes. Not for tagging, code review, or ordinary PRs.
disable-model-invocation: true
---

# Incodex release notes

This runs **after** `.github/workflows/release.yml` has created the GitHub Release. The workflow builds binaries and attaches `SHA256SUMS` with `generate_release_notes: false`. Notes are a follow-up `gh release edit`. Never `gh release create`.

## Inputs

1. **Version**. Lowercase `v`, e.g. `v0.1.0`. Our workflow watches `v*`; a capital `V0.1.0` will not build.
2. **Title**. Default `v<version>`. A short codename is optional; ask before adding one. Version lives only in the title, not in the body h1.
3. **Commit range**. `git log <previous-tag>..v<version> --oneline`. If this is the first release, use `git log v<version> --oneline`.
4. **User-visible changes**. Read commit bodies, not just subjects. Install, uninstall, open, runtime, session burn, and CLI menu changes belong even when they are not bug-shaped.
5. **Reporters and contributors**. Merged PRs and issues in the range. Exclude repo owners and bots. Keep it short: `@a · @b`.
6. **Release exists**. `gh release view v<version> --json id,name` must be non-empty. If it is empty, wait for the workflow. Do not create a competing release.

If there is no previous release, use the Format section below as the template. If there is one, also read it: `gh release view --json tagName,body`.

## Format

```
<div align="center">
  <img src="https://raw.githubusercontent.com/daftAI2026/incodex/main/assets/hat-glasses.svg" alt="Incodex" width="96" />
  <h1 style="margin: 12px 0 6px;">Incodex</h1>
  <p><em>Incognito toggle for Codex desktop.</em></p>
</div>

### Changelog

1. **<English headline>**: <one-sentence English elaboration>.
2. ...

### 更新日志

1. **<中文 headline>**：<一句中文说明>。
2. ...

### Thanks

Issue reporters and PR contributors this cycle: @handle1 · @handle2.
```

Omit the Thanks block when the cycle has no external reporters or contributors.

### Rules

- Body h1 is `Incodex`. Version stays in `--title`.
- English block first, 中文 second. Same numbered order. Same item count.
- Order by user-perceived impact, not commit chronology. Headline change first; install/signing/runtime hardening after.
- No em dash. No inline PR numbers. No inline `@handle` except in Thanks.
- No emoji in the body. Section headers stay plain `### Changelog` / `### 更新日志` / `### Thanks`.
- Every command named in the notes must exist in HEAD (`incodex --help` / `src/parse-cli.ts`). Do not advertise `--live` or `--confirm-live`.
- Do not claim “no traces on the machine”.
- Do not mention third-party CLIs by name.

## Publish

Draft first. After the user approves:

```bash
gh release edit v<version> --repo daftAI2026/incodex \
  --title "v<version>" \
  --notes-file <path-to-draft>
```

Then `gh release view v<version> --web`.

## When NOT to act

- Mentions notes in passing: draft only, do not `gh release edit`.
- Release does not exist yet: wait; do not `gh release create`.
- No explicit publish / 提交: stop after the draft.
