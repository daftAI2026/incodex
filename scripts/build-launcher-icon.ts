/**
 * [INPUT]: 依赖自有胶囊/字样SVG、布局、Apple Icon Composer及Swift图像导出。
 * [OUTPUT]: 生成独立透明INCODEX铭牌PNG；不重建冻结的旧代ICNS。
 * [POS]: 显式开发资产构建，不读取宿主；用户启动时只调用内嵌原生合成。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const root = resolve(import.meta.dir, "..");
const output = resolve(process.argv[2] ?? join(root, "crates/incodex-cli/assets/launcher-wordmark-badge.png"));
const ictool = process.argv[3] ?? "/Applications/Xcode.app/Contents/Applications/Icon Composer.app/Contents/Executables/ictool";
const temporary = mkdtempSync(join(tmpdir(), "incodex-wordmark-build-"));
function run(command: string, args: string[]): void {
  const result = spawnSync(command, args, { encoding: "utf8", timeout: 120_000 });
  if (result.status !== 0) throw new Error(`${command}: ${result.error?.message ?? result.stderr}`);
}
function group(image: string, glass: boolean) {
  return {
    layers: [{ "image-name": image, glass, position: { scale: 1, "translation-in-points": [0, 0] } }],
    shadow: { kind: "neutral", opacity: 0 },
    specular: false,
    translucency: { enabled: false, value: 0 },
  };
}
try {
  for (const color of ["black", "white"]) {
    const document = join(temporary, `${color}.icon`);
    const assets = join(document, "Assets");
    mkdirSync(assets, { recursive: true });
    copyFileSync(join(root, "assets/launcher-wordmark.svg"), join(assets, "word.svg"));
    copyFileSync(join(root, "assets/launcher-badge.svg"), join(assets, "capsule.svg"));
    writeFileSync(join(assets, "matte.svg"), `<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024"><rect width="1024" height="1024" fill="${color}"/></svg>`);
    const capsule = {
      ...group("capsule.svg", true), specular: true,
      opacity: 0.85, "blur-material": 0.12,
      translucency: { enabled: true, value: 0.85 },
    };
    writeFileSync(join(document, "icon.json"), JSON.stringify({
      fill: { solid: "srgb:1,1,1,0" },
      groups: [group("word.svg", false), capsule, group("matte.svg", false)],
      "supported-platforms": { squares: ["macOS"] },
    }));
    run(ictool, [document, "--export-image", "--output-file", join(temporary, `${color}.png`),
      "--platform", "macOS", "--rendition", "Default", "--width", "1024", "--height", "1024", "--scale", "2"]);
  }
  run("swift", [join(root, "scripts/extract-launcher-badge.swift"), join(temporary, "black.png"),
    join(temporary, "white.png"), join(root, "assets/launcher-badge-layout.json"), output]);
  process.stdout.write(`Generated ${output}\n`);
} finally {
  rmSync(temporary, { recursive: true, force: true });
}
