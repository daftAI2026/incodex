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
  const audit = spawnSync(process.platform === 'win32' ? 'npm.cmd' : 'npm', ['audit', '--json'], {
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
