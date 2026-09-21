import { expect, test } from "bun:test";
import { mkdtempSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { join } from "node:path";
import { tmpdir } from "node:os";

test.skipIf(process.platform !== "darwin")("OSA presenter handles async ready, events, state and idempotent close without UI", () => {
  const directory = mkdtempSync(join(tmpdir(), "incodex-osa-contract-"));
  const harness = join(directory, "contract.swift");
  const output = join(directory, "contract");
  writeFileSync(harness, `
import Foundation
enum PermissionHostOSASource {
 static let runtime = """
 let ready=false,closed=false,queue=[];
 function configure(json){const c=JSON.parse(json);if(c.copy.title!=='fixture')throw Error('copy');return true;}
 function present(){Promise.resolve().then(()=>{ready=true;queue.push({type:'allow'});});return false;}
 function isready(){return ready&&!closed;}
 function drainEvents(){return JSON.stringify(queue.splice(0));}
 function setstate(json){if(JSON.parse(json).state==='error')throw Error('fixture error');return true;}
 function close(){closed=true;return true;}
 """
}
@main @MainActor struct Contract {
 static func main(){
 var events=[String](), errors=[String]()
 let presenter=PermissionHostPresenter(copy:["title":"fixture"],layoutDirection:"leftToRight",onEvent:{events.append($0)},onError:{errors.append($0)})
 precondition(presenter.present())
 RunLoop.main.run(until:Date(timeIntervalSinceNow:0.1))
 precondition(events == ["allow"])
 presenter.setState("awaiting-user",message:nil)
 precondition(errors.isEmpty)
 presenter.setState("error",message:nil)
 precondition(errors.count == 1)
 presenter.close();presenter.close()
 precondition(!presenter.present())
 print("PASS OSA protocol adapter")
 }
}
`);
  const built = spawnSync("xcrun", ["swiftc", join(import.meta.dir, "../native/macos/permission-host-osa.swift"), harness, "-o", output], { encoding: "utf8" });
  expect(built.status, built.stderr).toBe(0);
  const result = spawnSync(output, [], { encoding: "utf8", timeout: 10_000 });
  expect(result.status, result.stderr).toBe(0);
  expect(result.stdout).toContain("PASS OSA protocol adapter");
}, 120_000);
