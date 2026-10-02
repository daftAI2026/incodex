import { defineConfig } from "astro/config";
import tailwindcss from "@tailwindcss/vite";
import nimbus, {
  defineConfig as defineNimbusConfig,
} from "@cloudflare/nimbus-docs";
import { tableScroll } from "@cloudflare/nimbus-docs/markdown";
import releases from "./releases.json";

const nimbusConfig = defineNimbusConfig({
  site: "https://daftAI2026.github.io",
  title: "Incodex",
  description: `A private window for Codex. Installation, commands, and practical guides for Incodex ${releases[0].version}.`,
  locale: "en",
  sidebar: { items: [
    { label: "English", autogenerate: { directory: "en" } },
    { label: "简体中文", autogenerate: { directory: "zh" } },
  ] },
  github: "https://github.com/daftAI2026/incodex",
  editPattern: "https://github.com/daftAI2026/incodex/edit/main/manual/src/content/docs/{path}",
  socialImage: "/social.svg",
  socialImageAlt: "Incodex · A private window for Codex",
});

export default defineConfig({
  base: "/incodex",
  trailingSlash: "always",
  // nimbus:adapter
  output: "static",
  // Tailwind v4 via its Vite plugin (the integration Astro recommends for
  // Tailwind v4 — replaces the PostCSS plugin, which doesn't build under
  // Astro 7's Vite 8 bundler).
  vite: {
    plugins: [tailwindcss()],
  },
  // Hover-prefetch link targets so full-page navigations feel instant without
  // a client-side router.
  prefetch: {
    prefetchAll: true,
    defaultStrategy: "hover",
  },
  integrations: [
    nimbus(nimbusConfig, {
      // Authoring rules are opt-in by design — your repo, your taste. The
      // two below are the load-bearing pair: frontmatter has to validate
      // against the content schema for the page to render properly, and
      // broken internal links are 404s for your readers. Add the others
      // (heading hierarchy, code-block language, style, etc.) when you're
      // ready to enforce them — see `nimbus-docs lint --help`.
      rules: {
        "nimbus/frontmatter-shape": "error",
        "nimbus/internal-link": "error",
      },
      // Wrap wide tables so they scroll instead of overflowing the page
      // (styled by `.nb-table-scroll` in src/styles/prose.css).
      markdown: {
        hastPlugins: [tableScroll()],
      },
    }),
  ],
});
