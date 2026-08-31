---
name: release-flow
description: Incodex CLI release runbook. Tag vX.Y.Z, wait for release.yml to attach binaries and SHA256SUMS, then hand off to release-notes. Use when assessing or executing an Incodex release. Not for release-note copy alone or ordinary code review.
---

# Incodex CLI release flow

Tag-driven. `.github/workflows/release.yml` watches `'v*'` (lowercase). It builds the native Rust CLI as `incodex-darwin-arm64`, `incodex-darwin-x64`, and `incodex-windows-x64.exe` on native macOS/Windows runners, writes one `SHA256SUMS`, attests, and creates a GitHub Release **without notes**. Bun still builds/checks the embedded Electron Runtime; Rust owns the product and legacy fixture/proof tests and produces the release CLI assets.

## Channels

| Channel | What ships | Trigger |
|---|---|---|
| GitHub stable | two native Rust macOS binaries + one Windows x64 binary + `SHA256SUMS` | Push `vX.Y.Z` |
| `install.sh` | downloads `releases/latest` | Same release |
| `install.ps1` | downloads the Windows x64 asset from `releases/latest` | Same release |
| Source | `git pull` + `cargo install --locked --path crates/incodex-cli` | No tag |
| Homebrew tap | `daftAI2026/homebrew-tap` formula urls/shas | Same release, `update-formula` job |

Restate which channels this run will touch before acting. Do not open a Homebrew/homebrew-core PR. Do not publish npm.

## Prepare a version

```bash
bun run release:prepare -- <version>
```

This synchronizes `package.json`, the Cargo workspace and lockfile, the committed Runtime manifest, and both README output examples. Review and commit those changes before tagging. The command never commits, tags, or publishes.

## Pre-flight

1. `package.json` `"version"` matches the tag without the `v`.
2. `git status -s` is empty except intentionally staged release work.
3. `git log origin/main..HEAD --oneline` is only what you intend to ship.
4. `bun run check` exits 0.
5. `cargo test --locked --workspace --release` exits 0.
6. `release.yml` still has `generate_release_notes: false` and contains no Bun CLI compile step or legacy Bun asset.
7. Before pushing the tag, audit `README.md` and `README_CN.md` against the behavior being released, including feature wording, command examples, output examples, and version references.
8. Before pushing the tag, draft the bilingual title and notes with `.claude/skills/release-notes/SKILL.md`. Publication still waits until the workflow has created the Release.

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

Must list both macOS architecture binaries, `incodex-windows-x64.exe`, **and** `SHA256SUMS`. Both platform installers fail closed on a missing or mismatched checksum; that is a release blocker, not cosmetic.

The `update-formula` job clones `daftAI2026/homebrew-tap`, runs `scripts/update-homebrew-tap-formula.sh`, and commits `incodex <version>` / `Automated release via GitHub Actions`. It needs repository secret `HOMEBREW_TAP_TOKEN` (contents write on the tap). Missing checksums or a missing token fail the job; the GitHub Release itself already exists. Do not bump Homebrew/homebrew-core.

Install smoke after assets exist: on macOS, install the previous binary when one exists (otherwise use a clean first-time `install.sh`), run `incodex --version`, then run `inc update` and verify the new version. On Windows, run `install.ps1` in a clean user-scoped prefix, verify both launchers and the installed EXE, then exercise the managed `inc update` path and Runtime synchronization.

Then load `.claude/skills/release-notes/SKILL.md` and draft notes. After `gh release edit`, run `bash .claude/skills/release-notes/scripts/post-reactions.sh v<version>`. Do not announce until notes and reactions are published.

## Pitfalls

- **`gh release create` conflicts with the workflow.** Notes use `gh release edit`.
- **Tag prefix is case-sensitive.** `v0.1.0` triggers the workflow. `V0.1.0` does not.
- **Do not retag the same version.** If the build is bad: `gh release delete v<old> --cleanup-tag`, delete the local tag, bump `package.json`, commit, tag `v<new>`.
- **Homebrew core is out of scope.** The tap bump is this flow. Do not open a Homebrew/homebrew-core PR.
