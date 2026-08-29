import { describe, expect, test } from "bun:test";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dir, "..");

const WORKSPACE_MEMBERS = [
  "crates/incodex-cli",
  "crates/incodex-core",
  "crates/incodex-transaction",
  "crates/incodex-macos",
  "crates/incodex-runtime-assets",
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

const RUST_WORKFLOWS = [".github/workflows/ci.yml", ".github/workflows/release.yml"];

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

  test("pins one exact Rust toolchain across workspace and release paths", () => {
    const toolchain = read("rust-toolchain.toml");
    const channel = toolchain.match(/^channel\s*=\s*"([^"]+)"\s*$/m)?.[1];
    expect(channel).toMatch(/^\d+\.\d+\.\d+$/);

    const cargo = read("Cargo.toml");
    const msrv = channel?.split(".").slice(0, 2).join(".");
    expect(cargo).toMatch(new RegExp(`^rust-version\\s*=\\s*"${msrv}"\\s*$`, "m"));
    for (const member of WORKSPACE_MEMBERS) {
      expect(read(`${member}/Cargo.toml`)).toMatch(
        /^rust-version\.workspace\s*=\s*true\s*$/m,
      );
    }

    for (const workflowPath of RUST_WORKFLOWS) {
      const workflow = read(workflowPath);
      expect(workflow).toContain("actions-rust-lang/setup-rust-toolchain@");
      expect(workflow).not.toMatch(/^\s*toolchain\s*:/m);
    }
  });

  test("documents the Rust toolchain upgrade procedure", () => {
    const contributing = read("CONTRIBUTING.md");
    expect(contributing).toMatch(/^## Rust toolchain upgrades\s*$/m);
    expect(contributing).toContain("rust-toolchain.toml");
    expect(contributing).toContain("rust-version");
    expect(contributing).toContain("cargo fmt --all -- --check");
    expect(contributing).toContain(
      "cargo clippy --workspace --all-targets --locked -- -D warnings",
    );
    expect(contributing).toContain("cargo test --workspace --release --locked");
  });

  test("source install docs require the pinned Rust toolchain", () => {
    for (const path of ["README.md", "README_CN.md"]) {
      const readme = read(path);
      expect(readme).toContain("rust-toolchain.toml");
      expect(readme).not.toMatch(/requires stable Rust|需要稳定版 Rust/);
    }
    expect(read("CONTRIBUTING.md")).not.toMatch(/Rust \*\*\d+\.\d+\.\d+\*\*/);
  });

  test("CI runs the locked Rust quality gates on macos", () => {
    const ci = read(".github/workflows/ci.yml");
    expect(ci).toContain("cargo fmt --all -- --check");
    expect(ci).toContain(
      "cargo clippy --workspace --all-targets --locked -- -D warnings",
    );
    expect(ci).toContain("cargo test --workspace --release --locked");
  });

  test("Windows CI runs every supported Windows contract suite", () => {
    const ci = read(".github/workflows/ci.yml");
    expect(ci).toContain("bun run test:shared");
    expect(ci).toContain("bun run test:windows");
    for (const crate of ["incodex-core", "incodex-cli"]) {
      const directory = join(root, "crates", crate, "tests");
      const suites = readdirSync(directory)
        .filter((name) => name.startsWith("windows_") && name.endsWith(".rs"))
        .map((name) => name.slice(0, -3));
      expect(suites.length).toBeGreaterThan(0);
      for (const suite of suites) {
        expect(ci).toContain(
          `cargo test -p ${crate} --test ${suite} --release --locked`,
        );
      }
    }
  });

  test("Windows terminal flows reuse the shared progress lifecycle", () => {
    const lib = read("crates/incodex-cli/src/lib.rs");
    const windowsOpen = read("crates/incodex-cli/src/windows_open.rs");
    const windowsStatus = read("crates/incodex-cli/src/windows_status.rs");
    const windowsDoctor = read("crates/incodex-cli/src/windows_doctor.rs");

    expect(lib).toContain("pub mod spinner;");
    expect(lib).not.toMatch(
      /#\[cfg\(not\(target_os = "windows"\)\)\]\s*pub mod spinner;/,
    );
    expect(windowsOpen).toContain("Spinner::start(UI_READY_WAIT_MESSAGE)");
    expect(windowsOpen).toContain("Spinner::start(WAITING_MESSAGE)");
    expect(windowsOpen).toContain("Spinner::start(REMOVING_SESSION_MESSAGE)");
    expect(windowsStatus).toContain("Spinner::start(STATUS_PROGRESS_MESSAGE)");
    expect(windowsDoctor).toContain("Spinner::start(DOCTOR_PROGRESS_MESSAGE)");
  });

  test("terminal reports keep one shared formatter and main's final spacing", () => {
    const version = read("crates/incodex-cli/src/version.rs");
    const lib = read("crates/incodex-cli/src/lib.rs");
    const openCommand = read("crates/incodex-cli/src/open_command.rs");
    const windowsOpen = read("crates/incodex-cli/src/windows_open.rs");
    const windowsStatus = read("crates/incodex-cli/src/windows_status.rs");
    const windowsDoctor = read("crates/incodex-cli/src/windows_doctor.rs");

    expect(version.match(/pub fn format_version_report/g) ?? []).toHaveLength(1);
    expect(lib).toContain("print_terminal_report");
    expect(windowsStatus).toContain("print_terminal_report");
    expect(windowsDoctor).toContain("print_terminal_report");
    expect(openCommand).toContain("print_terminal_result");
    expect(windowsOpen).toContain("print_terminal_result");
  });
});
