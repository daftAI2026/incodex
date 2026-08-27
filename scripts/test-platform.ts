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
