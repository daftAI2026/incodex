import assert from 'node:assert/strict';
import { test } from 'node:test';
import { selectStableRelease, updateReleaseBadge } from '../src/lib/latest-release.ts';

const baseline = 'v1.3.0';
const currentHref = '/incodex/zh/releases/v1.3.0/';
const released = () => ({
  tag_name: 'v1.4.0', draft: false, prerelease: false, published_at: '2026-10-04T10:00:00Z',
  html_url: 'https://github.com/daftAI2026/incodex/releases/tag/v1.4.0',
  assets: ['incodex-darwin-arm64', 'incodex-darwin-x64', 'incodex-windows-x64.exe', 'SHA256SUMS'].map(name => ({ name, size: 123, state: 'uploaded' })),
});
const badge = () => ({ textContent: baseline, href: currentHref });

test('the version badge follows a newer published stable GitHub release', async () => {
  const link = badge();
  let requested;
  const ok = await updateReleaseBadge(link, baseline, async (url, options) => {
    requested = [url, options];
    return { ok: true, json: async () => released() };
  });
  assert.equal(ok, true);
  assert.equal(link.textContent, 'v1.4.0');
  assert.equal(link.href, released().html_url);
  assert.equal(requested[0], 'https://api.github.com/repos/daftAI2026/incodex/releases/latest');
  assert.equal(requested[1].credentials, 'omit');
});
test('the synchronized release keeps its local manual entry', async () => {
  const r = released(); r.tag_name = baseline; r.html_url = `https://github.com/daftAI2026/incodex/releases/tag/${baseline}`;
  const link = badge();
  assert.equal(await updateReleaseBadge(link, baseline, async () => ({ ok: true, json: async () => r })), true);
  assert.equal(link.href, currentHref);
});
test('drafts, prereleases, malformed or older tags cannot replace the baseline', () => {
  for (const change of [{ draft: true }, { prerelease: true }, { tag_name: 'v1.4.0-rc.1' }, { tag_name: 'bad' }, { tag_name: 'v1.2.1', html_url: 'https://github.com/daftAI2026/incodex/releases/tag/v1.2.1' }, { published_at: null }]) {
    assert.equal(selectStableRelease({ ...released(), ...change }, baseline), null);
  }
  assert.equal(selectStableRelease(null, baseline), null);
});
test('a missing published binary/checksum or untrusted release URL is rejected', () => {
  const r = released(); r.assets.pop(); assert.equal(selectStableRelease(r, baseline), null);
  const empty = released(); empty.assets[0].size = 0; assert.equal(selectStableRelease(empty, baseline), null);
  assert.equal(selectStableRelease({ ...released(), html_url: 'https://untrusted.test/release' }, baseline), null);
});
test('API rate limits, network errors and bad JSON retain the rendered baseline', async () => {
  for (const fetcher of [async () => ({ ok: false }), async () => { throw new Error('offline'); }, async () => ({ ok: true, json: async () => { throw new Error('bad JSON'); } })]) {
    const link = badge(); assert.equal(await updateReleaseBadge(link, baseline, fetcher), false);
    assert.deepEqual(link, badge());
  }
});

test('assets that are still uploading cannot announce a release', () => {
  for (const state of ['new', 'starter', undefined]) {
    const release = released(); release.assets[0].state = state;
    assert.equal(selectStableRelease(release, baseline), null);
  }
});
