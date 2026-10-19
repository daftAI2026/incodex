/**
 * [INPUT]: 依赖 Nimbus 内容集合与 Astro schema。
 * [OUTPUT]: 导出公开指南与内容片段集合。
 * [POS]: 站点内容入口；架构地图不进入公开路由。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { defineCollection } from "astro:content";
// `z` re-exported from `astro:content` is deprecated; import it from
// `astro/zod` (the pattern nimbus-docs' own schema helpers document).
import { z } from "astro/zod";
import { docsCollection, partialsCollection } from "@cloudflare/nimbus-docs/content";

export const collections = {
  docs: defineCollection(
    docsCollection({
      // 架构地图不是用户指南；保留普通 Markdown 与 MDX 页面。
      pattern: "**/!(CLAUDE|AGENTS).{md,mdx}",
      schemaFields: {
        // Nimbus docs are agent-friendly by default. Set `audience: human`
        // to flag a page that's written primarily for human readers.
        audience: z.literal("human").optional(),
      },
    }),
  ),
  partials: defineCollection(partialsCollection()),
};
