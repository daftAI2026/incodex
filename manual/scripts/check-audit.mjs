/**
 * [INPUT]: 依赖 npm 完整依赖审计、当前 npm CLI 入口和子进程退出状态。
 * [OUTPUT]: 校验完整审计报告并拒绝任何级别漏洞，不设置例外。
 * [POS]: 手册发布安全门；跨平台调用 npm，审计规则独立于执行入口。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const root = fileURLToPath(new URL('../', import.meta.url));

export function assessAudit(report) {
  assert(report && report.auditReportVersion === 2 && !report.error, 'Invalid npm audit report');
  assert(report.vulnerabilities && typeof report.vulnerabilities === 'object' && !Array.isArray(report.vulnerabilities));
  const entries = Object.entries(report.vulnerabilities);
  assert.equal(report.metadata?.vulnerabilities?.total, entries.length, 'Audit total is inconsistent');
  assert.equal(['info', 'low', 'moderate', 'high', 'critical'].reduce((sum, level) =>
    sum + (report.metadata.vulnerabilities[level] ?? 0), 0), entries.length, 'Audit severity totals are inconsistent');
  assert.equal(entries.length, 0, 'Dependency vulnerabilities block publication; no exception is active');
  return { excepted: 0 };
}

function main() {
  // Keep the full dependency audit, including build/dev dependencies, in CI logs.
  const npmEntry = process.env.npm_execpath;
  const command = npmEntry ? process.execPath : process.platform === 'win32' ? 'cmd.exe' : 'npm';
  const args = npmEntry ? [npmEntry, 'audit', '--json'] : process.platform === 'win32'
    ? ['/d', '/c', 'npm audit --json'] : ['audit', '--json'];
  const audit = spawnSync(command, args, {
    cwd: root, encoding: 'utf8', timeout: 30_000, maxBuffer: 8 * 1024 * 1024,
  });
  process.stdout.write(audit.stdout ?? '');
  process.stderr.write(audit.stderr ?? '');
  assert(!audit.error && [0, 1].includes(audit.status), 'npm audit failed to produce a usable report');
  const report = JSON.parse(audit.stdout);
  assert.equal(audit.status, report.metadata?.vulnerabilities?.total ? 1 : 0, 'Audit exit status contradicts report');
  assessAudit(report);
  console.log('\nDependency audit is clean; no exception used.');
}
if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try { main(); } catch (error) { console.error(`Manual audit blocked: ${error.message}`); process.exitCode = 1; }
}
