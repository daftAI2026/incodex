import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync, readdirSync, existsSync } from 'node:fs';
import { join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const root = fileURLToPath(new URL('../', import.meta.url));
export const policy = JSON.parse(readFileSync(join(root, 'audit-exception.json'), 'utf8'));
const expectedVia = {
  astro: ['http-cache-semantics'],
  '@astrojs/mdx': ['astro'],
  '@cloudflare/nimbus-docs': ['@astrojs/mdx', 'astro'],
};
const hash = (file) => createHash('sha256').update(readFileSync(file)).digest('hex');
function files(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const file = join(directory, entry.name);
    assert(!entry.isSymbolicLink(), `Unexpected symlink: ${relative(root, file)}`);
    return entry.isDirectory() ? files(file) : [file];
  });
}

export function verifyScope(scope) {
  assert.deepEqual(scope.installedVersions, policy.versions, 'Installed dependency versions changed');
  assert.deepEqual(scope.lockedVersions, policy.versions, 'Locked dependency versions changed');
  assert.deepEqual(scope.lockedSources, policy.lockedSources, 'Locked dependency sources or integrity changed');
  assert.equal(scope.configHash, policy.configHash, 'Reviewed static configuration changed');
  assert.deepEqual(scope.routeHashes, policy.routeHashes, 'Reviewed prerendered route set changed');
  assert.equal(scope.consumerHash, policy.consumerHash, 'Reviewed cache consumer changed');
  assert.equal(scope.remoteAssetsAbsent, true, 'Remote image processing invalidates this exception');
  assert.equal(scope.staticArtifact, true, 'A static build with no server artifact is required');
}

function loadScope() {
  const lock = JSON.parse(readFileSync(join(root, 'package-lock.json'), 'utf8'));
  const names = Object.keys(policy.versions);
  const installedVersions = Object.fromEntries(names.map((name) => [name,
    JSON.parse(readFileSync(join(root, 'node_modules', name, 'package.json'), 'utf8')).version]));
  const lockedVersions = Object.fromEntries(names.map((name) => [name, lock.packages[`node_modules/${name}`]?.version]));
  const lockedSources = Object.fromEntries(names.map((name) => [name,
    Object.fromEntries(['resolved', 'integrity'].map((key) => [key, lock.packages[`node_modules/${name}`]?.[key]]))]));
  const routes = files(join(root, 'src/pages'));
  const authored = files(join(root, 'src')).filter((file) => /\.(astro|mdx?|[cm]?jsx?|tsx?)$/.test(file));
  // Conservative: introducing the image API or remote image syntax needs a new review.
  const remote = /astro:assets|!\[[^\]]*\]\(\s*https?:|<(?:img|Image|Picture)\b[^>]*\bsrc\s*=\s*["']https?:|url\(\s*["']?https?:/is;
  const dist = join(root, 'dist');
  const artifact = existsSync(dist) ? files(dist) : [];
  return {
    installedVersions, lockedVersions, lockedSources,
    configHash: hash(join(root, 'astro.config.ts')),
    routeHashes: Object.fromEntries(routes.map((file) => [relative(root, file).replaceAll('\\', '/'), hash(file)])),
    consumerHash: hash(join(root, 'node_modules/astro/dist/assets/build/remote.js')),
    remoteAssetsAbsent: authored.every((file) => !remote.test(readFileSync(file, 'utf8'))),
    staticArtifact: existsSync(join(dist, 'index.html')) &&
      !['server', 'client', '_worker.js'].some((name) => existsSync(join(dist, name))) &&
      artifact.every((file) => !/\.(?:cjs|mjs)$/.test(file)),
  };
}

export function assessAudit(report, scope, now = new Date()) {
  assert(report && report.auditReportVersion === 2 && !report.error, 'Invalid npm audit report');
  assert(report.vulnerabilities && typeof report.vulnerabilities === 'object' && !Array.isArray(report.vulnerabilities));
  const entries = Object.entries(report.vulnerabilities);
  assert.equal(report.metadata?.vulnerabilities?.total, entries.length, 'Audit total is inconsistent');
  assert.equal(['info', 'low', 'moderate', 'high', 'critical'].reduce((sum, level) =>
    sum + (report.metadata.vulnerabilities[level] ?? 0), 0), entries.length, 'Audit severity totals are inconsistent');
  if (entries.length === 0) return { excepted: 0 };
  assert(Number.isFinite(now.getTime()) && now <= new Date(policy.expires), 'Temporary advisory exception expired');
  verifyScope(scope);
  assert.deepEqual(entries.map(([name]) => name).sort(), Object.keys(policy.versions).sort(), 'Unreviewed vulnerability chain');
  assert.equal(report.metadata.vulnerabilities.high, 4, 'Audit severity/count changed');
  for (const [name, entry] of entries) {
    assert.equal(entry.name, name);
    assert.equal(entry.severity, 'high', 'Unreviewed severity');
    assert.deepEqual(entry.nodes, [`node_modules/${name}`], 'Unreviewed installation path');
    if (name !== 'http-cache-semantics') {
      assert.deepEqual([...entry.via].sort(), [...expectedVia[name]].sort(), 'Unreviewed advisory source');
      continue;
    }
    assert.equal(entry.via.length, 1, 'Additional advisory');
    // npm currently suggests downgrading Astro to 2.10.9, not a fixed cache package.
    assert.deepEqual(entry.fixAvailable, policy.reviewedFixRecommendation,
      'The fix recommendation changed; review the official fix and remove this exception');
    const advisory = entry.via[0];
    assert.equal(advisory.source, 1240991);
    assert.equal(advisory.url, policy.advisory);
    assert.equal(advisory.name, name);
    assert.equal(advisory.dependency, name);
    assert.equal(advisory.severity, 'high');
    assert.equal(advisory.range, '<=4.2.0');
  }
  return { excepted: entries.length };
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
  const scope = Object.keys(report.vulnerabilities ?? {}).length ? loadScope() : {};
  const result = assessAudit(report, scope);
  if (result.excepted) {
    console.error(`WARNING: ${policy.advisory} remains unfixed. Owner-approved static-build exception applies to this one advisory (${result.excepted} derived findings), through ${policy.expires}.`);
  } else {
    console.log('\nDependency audit is clean; no exception used.');
  }
}
if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try { main(); } catch (error) { console.error(`Manual audit blocked: ${error.message}`); process.exitCode = 1; }
}
