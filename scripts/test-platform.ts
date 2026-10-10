/**
 * [INPUT]: 依赖 宿主平台与 package 测试命令。
 * [OUTPUT]: 串行运行 shared 和当前宿主测试门。
 * [POS]: 本地检查选路；不代替另一平台实机验收。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
const platformSuite =
  process.platform === "win32"
    ? "test:windows"
    : process.platform === "darwin"
      ? "test:macos"
      : null;

if (!platformSuite) {
  throw new Error(`unsupported test platform: ${process.platform}`);
}

for (const suite of ["test:shared", platformSuite]) {
  const result = Bun.spawnSync({
    cmd: [process.execPath, "run", suite],
    stdin: "inherit",
    stdout: "inherit",
    stderr: "inherit",
  });
  if (result.exitCode !== 0) process.exit(result.exitCode);
}
