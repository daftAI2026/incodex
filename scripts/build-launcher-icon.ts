/**
 * [INPUT]: 依赖共享帽子SVG、开发机rsvg-convert与系统iconutil。
 * [OUTPUT]: 生成CLI嵌入的macOS彩色启动器ICNS，不触碰宿主。
 * [POS]: 显式开发资产构建；用户启动时不运行此脚本或渲染工具。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const root = join(import.meta.dir, "..");
const output = join(root, "crates/incodex-cli/assets/incodex-launcher.icns");
const temporary = mkdtempSync(join(tmpdir(), "incodex-launcher-icon-"));
const iconset = join(temporary, "IncodexLauncher.iconset");
const sizes = [16, 32, 128, 256, 512];
const hat = readFileSync(join(root, "assets/hat-glasses.svg"), "utf8")
  .trim()
  .replace('<svg ', '<svg x="160" y="160" ')
  .replace('width="24" height="24"', 'width="704" height="704"');
const artwork = `<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024" color="#20354a">
<defs><linearGradient id="background" x1="0" y1="0" x2="1" y2="1"><stop stop-color="#a8e6cf"/><stop offset="1" stop-color="#b5b9ee"/></linearGradient></defs>
<rect x="64" y="64" width="896" height="896" rx="208" fill="url(#background)"/>
${hat}
</svg>`;

function run(command: string, args: string[]) {
  const result = spawnSync(command, args, { stdio: "inherit" });
  if (result.error || result.status !== 0) {
    throw new Error(`Icon build failed: ${command}; install librsvg on the development Mac if missing.`, {
      cause: result.error,
    });
  }
}

try {
  mkdirSync(iconset);
  const source = join(temporary, "launcher.svg");
  writeFileSync(source, artwork);
  for (const size of sizes) {
    for (const scale of [1, 2]) {
      const pixels = size * scale;
      const name = `icon_${size}x${size}${scale === 2 ? "@2x" : ""}.png`;
      run("rsvg-convert", ["--width", String(pixels), "--height", String(pixels), "--output", join(iconset, name), source]);
    }
  }
  const built = join(temporary, "IncodexLauncher.icns");
  run("/usr/bin/iconutil", ["--convert", "icns", "--output", built, iconset]);
  copyFileSync(built, output);
  process.stdout.write(`Built ${output}\n`);
} finally {
  rmSync(temporary, { recursive: true, force: true });
}
