import { describe, expect, test } from "bun:test";
import { formatVersionReport } from "./version-report";

describe("formatVersionReport", () => {
  test("prints version, machine, install channel, and shell", () => {
    const text = formatVersionReport({
      version: "0.1.0",
      macos: "15.4",
      architecture: "arm64",
      kernel: "24.4.0",
      sip: "Enabled",
      diskFree: "177.88GB",
      install: "Script",
      shell: "/bin/zsh",
    });
    expect(text).toContain("Incodex version 0.1.0");
    expect(text).toContain("macOS: 15.4");
    expect(text).toContain("Architecture: arm64");
    expect(text).toContain("Kernel: 24.4.0");
    expect(text).toContain("SIP: Enabled");
    expect(text).toContain("Disk Free: 177.88GB");
    expect(text).toContain("Install: Script");
    expect(text).toContain("Shell: /bin/zsh");
    expect(text.endsWith("\n")).toBe(true);
  });
});
