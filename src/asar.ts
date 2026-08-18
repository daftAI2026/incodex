import asar from "@electron/asar";
import { createHash } from "node:crypto";
import {
  cpSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  renameSync,
  rmSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { LOADER_NAME, INJECT_NAME, MAIN_NAME, PRELOAD_NAME, SAFE_HOME_NAME, IPC_GUARD_NAME, INSTANCE_NAME, MARKER_KEY } from "./paths";

type AsarApi = typeof asar & {
  getRawHeader: (p: string) => { header: unknown; headerString: string };
};

const asarApi = asar as AsarApi;

export function headerHash(asarPath: string): string {
  const raw = asarApi.getRawHeader(asarPath);
  return createHash("sha256").update(raw.headerString).digest("hex");
}

export type IncodexMarker = {
  originalMain?: string;
  installId?: string;
};

export function readPackageMain(asarPath: string): {
  main: string;
  alreadyPatched: boolean;
  installId: string | null;
} {
  const raw = JSON.parse(asar.extractFile(asarPath, "package.json").toString("utf8")) as {
    main?: string;
    [MARKER_KEY]?: IncodexMarker;
  };
  const marker = raw[MARKER_KEY];
  const original = marker?.originalMain;
  return {
    main: original ?? raw.main ?? "",
    alreadyPatched: Boolean(original),
    installId: marker?.installId || null,
  };
}

export async function patchAsar(options: {
  asarPath: string;
  loaderSource: string;
  injectSource: string;
  mainSource: string;
  preloadSource: string;
  safeHomeSource: string;
  ipcGuardSource: string;
  instanceSource: string;
  installId?: string;
}): Promise<{ hash: string; originalMain: string }> {
  const { main: originalMain, alreadyPatched } = readPackageMain(options.asarPath);
  if (!originalMain) throw new Error("package.json has no main");

  const work = mkdtempSync(join(tmpdir(), "incodex-asar-"));
  const extractDir = join(work, "src");
  const outAsar = join(work, "app.asar");
  const unpack = collectUnpackOptions(options.asarPath);

  try {
    asar.extractAll(options.asarPath, extractDir);
    const pkgPath = join(extractDir, "package.json");
    const pkg = JSON.parse(readFileSync(pkgPath, "utf8")) as Record<string, unknown> & {
      main?: string;
      [MARKER_KEY]?: IncodexMarker;
    };
    const keepMain = alreadyPatched ? (pkg[MARKER_KEY]?.originalMain ?? originalMain) : originalMain;
    pkg.main = LOADER_NAME;
    pkg[MARKER_KEY] = {
      originalMain: keepMain,
      ...(options.installId ? { installId: options.installId } : {}),
    };
    writeFileSync(pkgPath, `${JSON.stringify(pkg, null, 2)}\n`);
    writeFileSync(join(extractDir, LOADER_NAME), options.loaderSource);
    writeFileSync(join(extractDir, INJECT_NAME), options.injectSource);
    writeFileSync(join(extractDir, MAIN_NAME), options.mainSource);
    writeFileSync(join(extractDir, PRELOAD_NAME), options.preloadSource);
    writeFileSync(join(extractDir, SAFE_HOME_NAME), options.safeHomeSource);
    writeFileSync(join(extractDir, IPC_GUARD_NAME), options.ipcGuardSource);
    writeFileSync(join(extractDir, INSTANCE_NAME), options.instanceSource);

    await asar.createPackageWithOptions(extractDir, outAsar, {
      globOptions: { dot: true },
      ...unpack,
    });

    const staging = `${options.asarPath}.incodex-new`;
    cpSync(outAsar, staging);
    try {
      renameSync(staging, options.asarPath);
    } catch (error) {
      try {
        unlinkSync(staging);
      } catch {
        /* ignore */
      }
      throw error;
    }
    return { hash: headerHash(options.asarPath), originalMain: keepMain };
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
}

export async function restoreAsarMain(asarPath: string): Promise<void> {
  const { main, alreadyPatched } = readPackageMain(asarPath);
  if (!alreadyPatched) return;
  const work = mkdtempSync(join(tmpdir(), "incodex-asar-"));
  const extractDir = join(work, "src");
  const outAsar = join(work, "app.asar");
  const unpack = collectUnpackOptions(asarPath);
  try {
    asar.extractAll(asarPath, extractDir);
    const pkgPath = join(extractDir, "package.json");
    const pkg = JSON.parse(readFileSync(pkgPath, "utf8")) as Record<string, unknown>;
    pkg.main = main;
    delete pkg[MARKER_KEY];
    writeFileSync(pkgPath, `${JSON.stringify(pkg, null, 2)}\n`);
    rmSync(join(extractDir, LOADER_NAME), { force: true });
    rmSync(join(extractDir, INJECT_NAME), { force: true });
    rmSync(join(extractDir, MAIN_NAME), { force: true });
    rmSync(join(extractDir, PRELOAD_NAME), { force: true });
    rmSync(join(extractDir, SAFE_HOME_NAME), { force: true });
    rmSync(join(extractDir, IPC_GUARD_NAME), { force: true });
    rmSync(join(extractDir, INSTANCE_NAME), { force: true });
    await asar.createPackageWithOptions(extractDir, outAsar, {
      globOptions: { dot: true },
      ...unpack,
    });
    const staging = `${asarPath}.incodex-new`;
    cpSync(outAsar, staging);
    renameSync(staging, asarPath);
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
}

function collectUnpackOptions(asarPath: string): { unpack?: string; unpackDir?: string } {
  const sibling = `${asarPath}.unpacked`;
  if (!existsSync(sibling)) return {};
  const raw = asarApi.getRawHeader(asarPath);
  const covers = unpackCovers((raw.header as { files?: Record<string, unknown> }) ?? {}, "");
  const dirs = covers.filter((c) => c.type === "dir").map((c) => stripSlash(c.path));
  const files = covers.filter((c) => c.type === "file").map((c) => `**/${stripSlash(c.path)}`);
  return {
    ...(files.length > 0 ? { unpack: brace(files) } : {}),
    ...(dirs.length > 0 ? { unpackDir: brace(dirs) } : {}),
  };
}

function unpackCovers(
  node: Record<string, unknown>,
  prefix: string,
): { type: "dir" | "file"; path: string }[] {
  const files = (node as { files?: Record<string, Record<string, unknown>> }).files;
  if (!files) return [];
  const covers: { type: "dir" | "file"; path: string }[] = [];
  let total = 0;
  let unpacked = 0;
  const childCovers: { type: "dir" | "file"; path: string }[] = [];
  for (const [name, val] of Object.entries(files)) {
    const p = `${prefix}/${name}`;
    if (val.files) {
      childCovers.push(...unpackCovers(val, p));
      continue;
    }
    total += 1;
    if (val.unpacked) {
      unpacked += 1;
      childCovers.push({ type: "file", path: p });
    }
  }
  if (prefix && total > 0 && total === unpacked && childCovers.every((c) => c.type === "file")) {
    return [{ type: "dir", path: prefix }];
  }
  covers.push(...childCovers);
  return covers;
}

function stripSlash(path: string): string {
  return path.replace(/^\/+/, "");
}

function brace(patterns: string[]): string {
  return patterns.length === 1 ? patterns[0] : `{${patterns.join(",")}}`;
}

export function ensureDir(path: string): void {
  mkdirSync(path, { recursive: true });
}
