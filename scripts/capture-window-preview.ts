import { readFileSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dir, "..");
const sourceDir = join(root, "src/runtime/capture-window");
const backgroundAssetDirectory = join(root, "assets/capture-backgrounds");
const build = await Bun.build({
  entrypoints: [join(sourceDir, "preview.ts")],
  minify: false,
  sourcemap: "inline",
  target: "browser",
});

if (!build.success) {
  for (const log of build.logs) console.error(log);
  process.exit(1);
}

const javaScript = await build.outputs[0].text();
const html = readFileSync(join(sourceDir, "preview.html"), "utf8");
const css = readFileSync(join(sourceDir, "capture-window.css"), "utf8");
const colorPopoverCss = readFileSync(join(sourceDir, "color-popover.css"), "utf8");
const port = Number.parseInt(process.env.INCODEX_CAPTURE_PREVIEW_PORT ?? "4173", 10);

const server = Bun.serve({
  fetch(request): Response {
    const path = new URL(request.url).pathname;
    if (path === "/preview.js") {
      return new Response(javaScript, { headers: { "content-type": "text/javascript; charset=utf-8" } });
    }
    if (path === "/capture-window.css") {
      return new Response(css, { headers: { "content-type": "text/css; charset=utf-8" } });
    }
    if (path === "/color-popover.css") {
      return new Response(colorPopoverCss, {
        headers: { "content-type": "text/css; charset=utf-8" },
      });
    }
    if (path.startsWith("/capture-backgrounds/")) {
      const assetName = path.slice("/capture-backgrounds/".length);
      if (!/^[a-z-]+\.jpg$/.test(assetName)) return new Response("Not found", { status: 404 });
      return new Response(readFileSync(join(backgroundAssetDirectory, assetName)), {
        headers: { "content-type": "image/jpeg" },
      });
    }
    return new Response(html, { headers: { "content-type": "text/html; charset=utf-8" } });
  },
  hostname: "127.0.0.1",
  port,
});

console.log(`Capture Window preview ready on 127.0.0.1:${server.port}`);
