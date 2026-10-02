# Incodex public manual

Astro + Cloudflare Nimbus, published to https://daftAI2026.github.io/incodex/.
This package is separate from the Rust CLI and Electron Runtime. The repository's
AGENTS.md remains the agent contract. `docs/` is local research; private incident
records are not website content.

```bash
cd manual
npm ci
npm run check
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

The initial manual describes stable 1.2.1. The header carries its version; update
it and these pages when documented behavior ships. Unreleased or experimentally
verified capabilities must be labeled. A docs correction can deploy independently
without a CLI tag. Product release publishing keeps its existing owner approval.

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
