---
name: release-flow
description: Incodex CLI release runbook. Tag vX.Y.Z, wait for release.yml to attach binaries and SHA256SUMS, then hand off to release-notes. Use when assessing or executing an Incodex release. Not for release-note copy alone or ordinary code review.
---

# Incodex CLI release flow

Tag-driven. `.github/workflows/release.yml` watches `'v*'` (lowercase). It compiles `incodex-darwin-arm64` and `incodex-darwin-x64`, writes `SHA256SUMS`, attests, and creates a GitHub Release **without notes**.

## Channels

| Channel | What ships | Trigger |
|---|---|---|
| GitHub stable | two macOS binaries + `SHA256SUMS` | Push `vX.Y.Z` |
| `install.sh` | downloads `releases/latest` | Same release |
| Source | `git pull` + `bun link` | No tag |
| Homebrew | not this flow | Parked |

Restate which channels this run will touch before acting. Do not invent a tap or npm publish.

## Pre-flight

1. `package.json` `"version"` matches the tag without the `v`.
2. `git status -s` is empty except intentionally staged release work.
3. `git log origin/main..HEAD --oneline` is only what you intend to ship.
4. `bun run check` exits 0.
5. `release.yml` still has `generate_release_notes: false`.

## Tag and publish

```bash
git push origin main
git tag v<version>
git push origin v<version>
```

Wait for the `release` workflow. Then:

```bash
gh release view v<version> --json assets --jq '.assets[].name'
```

Must list both architecture binaries **and** `SHA256SUMS`. `install.sh` is fail-closed on a missing or mismatched checksum; that is a release blocker, not cosmetic.

Script-install smoke after assets exist: install the previous binary (or first-time `install.sh`), run `incodex --version`, then `incodex update` if this is not 0.1.0.

Then load `.claude/skills/release-notes/SKILL.md` and draft notes. Do not announce until notes are published.

## Pitfalls

- **`gh release create` conflicts with the workflow.** Notes use `gh release edit`.
- **Tag prefix is case-sensitive.** `v0.1.0` triggers the workflow. `V0.1.0` does not.
- **Do not retag the same version.** If the build is bad: `gh release delete v<old> --cleanup-tag`, delete the local tag, bump `package.json`, commit, tag `v<new>`.
- **Homebrew is out of scope.** Do not open a core or tap PR from this flow.
