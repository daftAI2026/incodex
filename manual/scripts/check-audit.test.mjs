import assert from 'node:assert/strict';
import { test } from 'node:test';
import { assessAudit } from './check-audit.mjs';

const clean = () => ({ auditReportVersion: 2, vulnerabilities: {}, metadata: { vulnerabilities: { total: 0 } } });
test('accepts a clean complete dependency report', () => {
  assert.deepEqual(assessAudit(clean()), { excepted: 0 });
});
test('the retired exception cannot accept its former advisory after an official fix', () => {
  const report = clean();
  report.vulnerabilities['http-cache-semantics'] = { severity: 'high', via: [{ url: 'https://github.com/advisories/GHSA-ch52-4w7c-c8xp' }] };
  report.metadata.vulnerabilities = { total: 1, high: 1 };
  assert.throws(() => assessAudit(report), /no exception is active/);
});
test('blocks findings at every severity including build dependencies', () => {
  for (const severity of ['info', 'low', 'moderate', 'high', 'critical']) {
    const report = clean();
    report.vulnerabilities['build-dependency'] = { severity };
    report.metadata.vulnerabilities = { total: 1, [severity]: 1 };
    assert.throws(() => assessAudit(report));
  }
});
test('rejects malformed reports and inconsistent totals', () => {
  for (const report of [null, {}, { ...clean(), error: { code: 'EAUDIT' } }, { ...clean(), auditReportVersion: 1 },
    { ...clean(), vulnerabilities: [] }, { ...clean(), metadata: { vulnerabilities: { total: 1, high: 1 } } },
    { ...clean(), metadata: { vulnerabilities: { total: 0, critical: 1 } } }]) {
    assert.throws(() => assessAudit(report));
  }
});
