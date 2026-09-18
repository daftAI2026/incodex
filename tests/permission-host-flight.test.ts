import { expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { advancePermissionSpring, createPermissionSpring, samplePermissionFlightAtProgress } from "../src/runtime/incodex-permission-motion.cts";

test("the native CLI flight preserves the existing integrator and geometry", () => {
  const root = join(import.meta.dir, "..");
  const temporary = mkdtempSync(join(tmpdir(), "incodex-native-host-flight-"));
  const main = join(temporary, "main.swift");
  const binary = join(temporary, "check");
  writeFileSync(main, `import AppKit
import Foundation
@main struct Check {
 @MainActor static func main() throws {
  var spring = PermissionHostSpring()
  let samples = (0...120).map { spring.advance(to: Double($0) / 60) }
  let geometry = [0.0, 0.25, 0.5, 0.75, 1.0].map { progress -> [Double] in
   let sample = permissionHostFlightSample(source: CGRect(x: 10,y: 20,width: 518,height: 80), sourceRadius: 12, target: CGRect(x: 300,y: 500,width: 400,height: 180), targetRadius: 14, progress: progress)
   return [sample.bounds.minX, sample.bounds.minY, sample.bounds.width, sample.bounds.height, sample.cornerRadius]
  }
  print(String(data: try JSONSerialization.data(withJSONObject: ["samples": samples, "geometry": geometry]), encoding: .utf8)!)
 }
}`);
  const compiled = spawnSync("xcrun", ["swiftc", "-parse-as-library", "native/macos/permission-views.swift", "native/macos/permission-host-flight.swift", main, "-o", binary], { cwd: root, encoding: "utf8", timeout: 60_000 });
  expect(compiled.stderr).toBe("");
  expect(compiled.status).toBe(0);
  const ran = spawnSync(binary, [], { encoding: "utf8", timeout: 10_000 });
  expect(ran.status).toBe(0);
  const actual = JSON.parse(ran.stdout);
  const spring = createPermissionSpring();
  for (let i = 0; i <= 120; i++) expect(actual.samples[i]).toBeCloseTo(advancePermissionSpring(spring, i / 60), 12);
  for (const [index, progress] of [0, .25, .5, .75, 1].entries()) {
    const sample = samplePermissionFlightAtProgress({ x: 10, y: 20, width: 518, height: 80, radius: 12 }, { x: 300, y: 500, width: 400, height: 180, radius: 14 }, progress);
    expect(actual.geometry[index]).toEqual([sample.bounds.x, sample.bounds.y, sample.bounds.width, sample.bounds.height, sample.cornerRadius]);
  }
}, 90_000);
