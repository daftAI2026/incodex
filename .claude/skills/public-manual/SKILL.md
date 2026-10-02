---
name: public-manual
description: Maintain and publish the bilingual Incodex Astro/Nimbus manual and release history when public product capabilities change or a stable release ships. Does not tag or publish CLI releases.
---

# Incodex public manual

Source is `manual/`; public site is https://daftai2026.github.io/incodex/.
Use `manual/README.md` for build details. The site's TypeScript 6/npm lockfile is independent of product TypeScript 7/Bun. Nimbus registers MDX itself; do not add a second MDX integration.

## Bind content to product evidence

- Read the latest stable GitHub Release, its exact tag commit, and the changed implementation/tests. Use native `parse.rs` and `help.rs` for commands/flags; use accepted app evidence for UI and lifecycle claims. A green unit suite alone is not real-window acceptance.
- Compare the release range and current guide pages. Update both `src/content/docs/en/` and `zh/` with matching slugs: command reference, relevant task guides, permissions/privacy, troubleshooting, and images as needed. CLI examples use `incodex`/`inc`; public update guidance uses `inc update`.
- Guides describe the published stable baseline in `manual/releases.json`. Mark unreleased behavior explicitly or keep a future page `draft: true`; do not advance the stable label before the release exists. Preserve experimental and unverified limits from the release evidence.
- Release entries describe changes, upgrade actions and known limitations at publication. Do not silently rewrite an old entry to imply later fixes shipped earlier. Correct factual errors explicitly. Older releases without a site entry remain accessible through the GitHub history link.
- Private docs and raw acceptance logs are research, not public copy. Use reviewed, redacted real screenshots or clearly labeled position diagrams; never invent a screenshot or expose account names, chats, auth/config secrets, machine paths, or debug endpoints.

## During a product change

Update affected bilingual guides in the product PR, labeling not-yet-released features. Prepare the eventual GitHub Release notes from user-visible behavior under the release-notes skill, not commit subjects alone; the website entry will copy the approved published notes. Documentation corrections may deploy independently and need no CLI version bump.

## Release notes source and synchronization

GitHub Release is the single source of truth for each version's release notes. AI may draft the bilingual notes under `../release-notes/SKILL.md`; after owner approval and publication, AI copies those published notes into the website. Do not author an independent website changelog from commits or summarize away published changes, upgrade actions, known limitations, or contributor acknowledgments. Transfer the English and Chinese blocks to their respective pages, preserving their wording and order; adapting headings, links and the shared logo layout is allowed.

The current workflow is skill-directed AI synchronization, not unattended import. The existing `Public manual` Action validates and deploys merged manual changes; no additional Release-triggered Action is needed. `release.yml` initially creates the Release without a notes body, so a `release: published` event alone is not a signal that the bilingual notes are ready. Wait for the approved English and Chinese blocks to be present before synchronizing.

If approved GitHub notes are corrected later, resynchronize the corresponding historical pages. If a factual error is discovered, follow the release-notes approval rules to correct the canonical Release first; do not silently give the website a conflicting account. Product guides are maintained separately: review implementation and accepted behavior to decide which instructions change; copying notes does not update the full manual.

## At a stable release

1. Follow `../release-flow/SKILL.md` and `../release-notes/SKILL.md` for the existing owner-approved CLI publication. Prepare manual changes alongside release preparation, keeping future content draft or explicitly unreleased until publication.
2. After the GitHub Release and approved bilingual notes exist, read them with `gh release view vX.Y.Z --json tagName,publishedAt,body,isPrerelease,isDraft`. Confirm the body contains the completed approved bilingual notes and verify the exact tag commit. An empty or incomplete body leaves website synchronization pending. Do not list drafts or prereleases as stable.
3. Add `en/releases/vX.Y.Z.mdx` and `zh/releases/vX.Y.Z.mdx`, set explicit frontmatter `slug: "en/releases/vX.Y.Z"` / `"zh/releases/vX.Y.Z"` (automatic file IDs strip dots), link them from both release indexes, and link the canonical GitHub Release for downloads. Copy the published language blocks as specified above, retaining any required loader reinstall and remaining limitations. Use the actual GitHub publication date, not the synchronization date. Do not manufacture a skipped version.
4. Prepend the actual stable version, tag, commit and UTC `publishedAt` to `manual/releases.json`. The header and site description read this file. Update both home-page baseline labels, release-index latest labels, and affected guides; remove superseded unreleased labels. The website package's version is not the CLI release authority and needs no bump for editorial updates.
5. Run the checks below; merge the docs PR under the repo's normal CI/merge instructions. Wait for the exact merged commit's `Public manual` build/deploy and inspect the live release page in both languages. A CLI release is not fully documented until this site sync is deployed; report any failed or pending site stage separately.

## Check and publish the site

```bash
cd manual
npm ci
npm run check
npm audit --omit=dev
```

`check` covers native commands/help flags, stable release consistency, Astro types, build, Nimbus lint, and built URLs/assets/fragments under `/incodex/`. It does not prove the prose or product UI correct. Review the changed claims against evidence.

For layout or components, use a scripted browser preview of the affected desktop/mobile pages, language switch, search, and new images. For prose-only changes, inspect the rendered affected pages and Markdown output; do not repeat unrelated product lifecycle acceptance. Use scripts/CDP for Codex, not CUA host operations.

`.github/workflows/public-manual.yml` deploys main automatically when manual changes merge. Workflow reruns for an existing authorized site update may use `workflow_dispatch`; do not create a CLI tag to publish docs. Confirm live URLs, the displayed stable version, and the exact deployment commit. Report a CI/review failure honestly; never record an unperformed review as clean.
