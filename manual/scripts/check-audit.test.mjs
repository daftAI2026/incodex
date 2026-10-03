import assert from 'node:assert/strict';
import { test } from 'node:test';
import { assessAudit, verifyScope, policy } from './check-audit.mjs';

const advisory = {
  source: 1240991,
  name: 'http-cache-semantics', dependency: 'http-cache-semantics',
  url: 'https://github.com/advisories/GHSA-ch52-4w7c-c8xp',
  severity: 'high', range: '<=4.2.0',
};
const row = (name, via) => ({ name, severity: 'high', via, nodes: [`node_modules/${name}`], fixAvailable: { name: 'astro', version: '2.10.9', isSemVerMajor: true } });
const report = () => ({
  auditReportVersion: 2,
  vulnerabilities: {
    'http-cache-semantics': row('http-cache-semantics', [structuredClone(advisory)]),
    astro: row('astro', ['http-cache-semantics']),
    '@astrojs/mdx': row('@astrojs/mdx', ['astro']),
    '@cloudflare/nimbus-docs': row('@cloudflare/nimbus-docs', ['@astrojs/mdx', 'astro']),
  },
  metadata: { vulnerabilities: { info: 0, low: 0, moderate: 0, high: 4, critical: 0, total: 4 } },
});
const scope = () => ({
  installedVersions: structuredClone(policy.versions),
  lockedVersions: structuredClone(policy.versions),
  lockedSources: structuredClone(policy.lockedSources),
  configHash: policy.configHash,
  routeHashes: structuredClone(policy.routeHashes),
  consumerHash: policy.consumerHash,
  remoteAssetsAbsent: true,
  staticArtifact: true,
});
const now = new Date('2026-10-03T00:00:00Z');

test('retains only the owner-approved fixed advisory chain under the reviewed scope', () => {
  verifyScope(scope());
  assert.deepEqual(assessAudit(report(), scope(), now), { excepted: 4 });
});
test('the current npm suggestion is the reviewed Astro downgrade, not a cache-library fix', () => {
  const r = report();
  r.vulnerabilities['http-cache-semantics'].fixAvailable = { name: 'astro', version: '2.10.9', isSemVerMajor: true };
  assert.deepEqual(assessAudit(r, scope(), now), { excepted: 4 });
});
test('a clean report needs no exception even after expiry', () => {
  assert.deepEqual(assessAudit({ auditReportVersion: 2, vulnerabilities: {}, metadata: { vulnerabilities: { total: 0 } } }, {}, new Date('2030-01-01')), { excepted: 0 });
});
test('other advisories remain blocking even inside the allowed package', () => {
  const r = report(); r.vulnerabilities.astro.via.push({ ...advisory, url: 'https://github.com/advisories/GHSA-other' });
  assert.throws(() => assessAudit(r, scope(), now));
});
test('unknown or incomplete dependency chains remain blocking', () => {
  const r = report(); delete r.vulnerabilities['@astrojs/mdx'];
  assert.throws(() => assessAudit(r, scope(), now));
  r.vulnerabilities.unreviewed = row('unreviewed', ['http-cache-semantics']);
  assert.throws(() => assessAudit(r, scope(), now));
});
test('changed advisory identity, severity or affected range remains blocking', () => {
  for (const field of ['source', 'severity', 'range', 'name', 'dependency']) {
    const r = report(); r.vulnerabilities['http-cache-semantics'].via[0][field] = 'changed';
    assert.throws(() => assessAudit(r, scope(), now));
  }
});
test('cyclic via references and extra installation paths remain blocking', () => {
  const r = report(); r.vulnerabilities.astro.via = ['@astrojs/mdx'];
  assert.throws(() => assessAudit(r, scope(), now));
  const other = report(); other.vulnerabilities.astro.nodes.push('node_modules/nested/node_modules/astro');
  assert.throws(() => assessAudit(other, scope(), now));
});
test('malformed reports and inconsistent totals remain blocking', () => {
  for (const r of [null, {}, { error: { code: 'EAUDIT' } }, { ...report(), auditReportVersion: 1 }]) {
    assert.throws(() => assessAudit(r, scope(), now));
  }
  const r = report(); r.metadata.vulnerabilities.total = 3;
  assert.throws(() => assessAudit(r, scope(), now));
});
test('unexpected severity totals cannot hide an additional finding', () => {
  const r = report(); r.metadata.vulnerabilities.critical = 1;
  assert.throws(() => assessAudit(r, scope(), now));
});
test('the temporary exception expires rather than silently becoming policy', () => {
  assert.throws(() => assessAudit(report(), scope(), new Date('2026-10-18T00:00:00Z')));
});
test('changed installed or locked versions require a fresh review', () => {
  for (const field of ['installedVersions', 'lockedVersions']) {
    const s = scope(); s[field].astro = '7.3.6';
    assert.throws(() => verifyScope(s));
  }
});
test('an available official fix ends the exception immediately', () => {
  for (const fix of [true, { name: 'http-cache-semantics', version: '4.2.1', isSemVerMajor: false }]) {
    const r = report(); r.vulnerabilities['http-cache-semantics'].fixAvailable = fix;
    assert.throws(() => assessAudit(r, scope(), now));
  }
});
test('changed lockfile sources or integrity require a fresh review', () => {
  const s = scope(); s.lockedSources = { 'http-cache-semantics': { resolved: 'https://unreviewed.test/package.tgz', integrity: 'changed' } };
  assert.throws(() => verifyScope(s));
});
test('changed config, routes or cache consumer requires a fresh review', () => {
  for (const field of ['configHash', 'consumerHash']) {
    const s = scope(); s[field] = 'changed'; assert.throws(() => verifyScope(s));
  }
  const s = scope(); s.routeHashes['src/pages/new-server.ts'] = 'unknown';
  assert.throws(() => verifyScope(s));
  const changed = scope(); changed.routeHashes['src/pages/robots.txt.ts'] = 'changed';
  assert.throws(() => verifyScope(changed));
});
test('remote images and a server artifact invalidate the static-only exception', () => {
  for (const field of ['remoteAssetsAbsent', 'staticArtifact']) {
    const s = scope(); s[field] = false; assert.throws(() => verifyScope(s));
  }
});
