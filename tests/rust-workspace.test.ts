import { describe, expect, test } from "bun:test";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dir, "..");

const WORKSPACE_MEMBERS = [
  "crates/incodex-cli",
  "crates/incodex-core",
  "crates/incodex-transaction",
  "crates/incodex-macos",
  "crates/incodex-runtime-bundle",
  "crates/incodex-asar",
];

const TUI_CRATES = [
  "ratatui",
  "cursive",
  "cursive_core",
  "tui",
  "tui-rs",
  "inquire",
  "dialoguer",
  "crossterm",
  "termion",
  "tuirs",
];

function read(rel: string): string {
  return readFileSync(join(root, rel), "utf8");
}

function cargoFiles(): string[] {
  const files = ["Cargo.toml"];
  if (existsSync(join(root, "Cargo.lock"))) files.push("Cargo.lock");
  for (const member of WORKSPACE_MEMBERS) {
    files.push(`${member}/Cargo.toml`);
  }
  return files.filter((rel) => existsSync(join(root, rel)));
}

describe("Rust workspace", () => {
  test("root Cargo.toml is a MIT workspace with the planned crates", () => {
    const cargo = read("Cargo.toml");
    expect(cargo).toContain("[workspace]");
    expect(cargo).toMatch(/license\s*=\s*"MIT"/);
    for (const member of WORKSPACE_MEMBERS) {
      expect(cargo).toContain(`"${member}"`);
      expect(existsSync(join(root, member, "Cargo.toml"))).toBe(true);
    }
  });

  test("every crate is MIT and incodex-cli builds the incodex binary", () => {
    for (const member of WORKSPACE_MEMBERS) {
      const cargo = read(`${member}/Cargo.toml`);
      expect(cargo).toMatch(/license\.workspace\s*=\s*true|license\s*=\s*"MIT"/);
      expect(cargo.toLowerCase()).not.toContain("agpl");
    }
    const cli = read("crates/incodex-cli/Cargo.toml");
    expect(cli).toContain('name = "incodex"');
  });

  test("workspace does not pull a TUI crate or an AGPL asar crate", () => {
    const texts = cargoFiles().map((rel) => read(rel).toLowerCase());
    expect(texts.length).toBeGreaterThan(0);
    for (const text of texts) {
      expect(text).not.toContain("agpl");
      for (const name of TUI_CRATES) {
        expect(text.includes(name)).toBe(false);
      }
    }
  });

  test("package.json version matches the workspace version", () => {
    const pkg = JSON.parse(read("package.json")) as { version: string; license: string };
    const cargo = read("Cargo.toml");
    expect(pkg.license).toBe("MIT");
    expect(cargo).toContain(`version = "${pkg.version}"`);
  });

  test("CI runs cargo test on macos", () => {
    const ci = read(".github/workflows/ci.yml");
    expect(ci).toContain("cargo test");
    expect(ci).toMatch(/runs-on:\s*macos-latest/);
    const uses = ci.split("\n").filter((line) => line.includes("uses:"));
    expect(uses.length).toBeGreaterThan(0);
    for (const line of uses) {
      expect(line).toMatch(/uses:\s+\S+@[0-9a-f]{40}\s+#\s+\S+/);
    }
  });
});
