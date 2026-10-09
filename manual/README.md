# Incodex public manual

Astro + Cloudflare Nimbus, published to https://daftAI2026.github.io/incodex/.
This package is separate from the Rust CLI and Electron Runtime. The repository's
AGENTS.md remains the agent contract. `docs/` is local research; private incident
records are not website content.

```bash
cd manual
npm ci
npm run check
npm run audit:security
npm run dev
```

The product uses TypeScript 7. The manual uses the latest stable TypeScript 6
supported by `@astrojs/check`, with its own package-lock.json. All other direct
website dependencies were set to their current npm latest releases when created.
Do not force unresolved peer dependencies. Review the Nimbus migration plan and
starter diffs when upgrading; keep nimbus.json as the reviewed scaffold baseline.

Write English and Chinese pages with matching slugs under src/content/docs/en and
src/content/docs/zh. Use public native `incodex` / `inc` examples. Update the manual
in the same PR as public behavior changes. `check:coverage` reads the native parser
and help to detect missing command pages and public help flags; `lint:docs` checks
content links, and `check:links` checks the built site under the GitHub Pages base.

`releases.json` lists documented stable releases, latest publication first, with
the exact tag commit and GitHub publication time. The header and site description
read its first entry. `check:release` verifies matching bilingual release pages,
index links, dates and home-page stable labels. It is separate from the website
package's version and the product's release-preparation files.

Follow [the public-manual skill](../.claude/skills/public-manual/SKILL.md) for product
changes and release synchronization. Prepare affected guides alongside a product
PR, labeling unreleased behavior; after the approved GitHub Release notes exist,
AI copies the published English and Chinese blocks into the respective release
pages, preserving wording, order, upgrade actions, limitations and acknowledgments.
GitHub Release is the single source for version notes; later approved corrections
are synchronized too. AI also updates the stable baseline, both release indexes,
the home pages, and affected guides against product evidence. This is skill-directed
synchronization, not an unattended importer. Wait for completed bilingual notes:
`release.yml` initially creates a Release without a notes body. The existing Pages
Action checks and deploys the merged manual changes; no additional Release Action
is required. Preserve historical release entries and experimental limits. A docs
correction can deploy without a CLI tag. Product publishing retains owner approval.

Nimbus installs and registers `@astrojs/mdx`; do not add a duplicate MDX integration.
Authored links use logical roots such as `/zh/releases/`; Nimbus applies the Pages
base. Static image URLs must include `/incodex/`. Check the built site for both.

The public-manual workflow checks PRs and builds/deploys the site after manual
changes reach main. Pages uses GitHub Actions as the source. Dependencies, build
output, npm caches, and preview screenshots are not committed.

## Images

public/images/window-guide-{en,zh}.svg are owned, explicitly labeled position
diagrams using the project's hat-glasses paths, not fabricated screenshots.
public/images/accessibility-guide.png is a reviewed permission-card screenshot
from the accepted macOS Runtime; it contains no account, conversation, or user path.
Do not copy private acceptance bundles or their logs into this package. Codex and
ChatGPT product names and official imagery belong to OpenAI; see the root NOTICE.

The UI components were scaffolded by @cloudflare/create-nimbus-docs 0.7.9 from
Cloudflare Nimbus (MIT): https://github.com/cloudflare/nimbus. See NOTICE for its
license. Visible files are maintained here; the Nimbus package provides the shared
content, validation, search, Markdown, and agent-index infrastructure.

## Dependency audit

`npm run audit:security` prints the full `npm audit --json` report, including
build/dev dependencies, and blocks all findings. The former temporary exception
for GHSA-ch52-4w7c-c8xp was retired after updating the locked
`http-cache-semantics` dependency to the official 4.3.0 release. No advisory
exception is active. The CLI and Electron Runtime retain their separate audits.

## Header release version

The header renders the documented release from `releases.json` first, then checks
GitHub's public latest-stable-release API in the browser without credentials.
It accepts only published stable tags with the three CLI assets and checksums.
A newer release links to its canonical GitHub notes; the documented baseline keeps
its local manual entry. Network errors or rate limits retain the rendered version.
The header update does not advance guide content: the release skill still updates
the documented baseline, approved bilingual notes and affected product guides.
