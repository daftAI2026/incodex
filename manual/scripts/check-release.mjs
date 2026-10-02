import { readFile } from 'node:fs/promises';
import assert from 'node:assert/strict';

const root = new URL('../', import.meta.url);
const releases = JSON.parse(await readFile(new URL('releases.json', root), 'utf8'));
assert(releases.length > 0, 'The manual needs a published stable release baseline');
const versions = new Set();
let previousDate = Infinity;
for (const release of releases) {
  assert.match(release.version, /^\d+\.\d+\.\d+$/);
  assert.equal(release.tag, `v${release.version}`);
  assert.match(release.commit, /^[a-f0-9]{40}$/);
  const date = Date.parse(release.publishedAt);
  assert(Number.isFinite(date), 'Invalid publication date');
  assert(date <= previousDate, 'Release entries must list the latest publication first');
  previousDate = date;
  assert(!versions.has(release.version), 'Duplicate release');
  versions.add(release.version);
  for (const locale of ['en', 'zh']) {
    const index = await readFile(new URL(`src/content/docs/${locale}/releases/index.mdx`, root), 'utf8');
    const page = await readFile(new URL(`src/content/docs/${locale}/releases/${release.tag}.mdx`, root), 'utf8');
    assert(index.includes(`/${locale}/releases/${release.tag}/`), 'Release omitted from index');
    assert(page.includes(`https://github.com/daftAI2026/incodex/releases/tag/${release.tag}`), 'Missing canonical release');
    assert(page.includes(release.publishedAt.slice(0, 10)), 'Missing actual release date');
  }
}
const latest = releases[0];
for (const locale of ['en', 'zh']) {
  const home = await readFile(new URL(`src/content/docs/${locale}/index.mdx`, root), 'utf8');
  assert(home.includes(`**Incodex ${latest.version}**`), 'Home differs from stable baseline');
  assert(home.includes(`/${locale}/releases/`), 'Home must link to release history');
  const index = await readFile(new URL(`src/content/docs/${locale}/releases/index.mdx`, root), 'utf8');
  assert(index.includes(`**${latest.tag}**`), 'Release index differs from stable baseline');
}
console.log(`Release baseline: ${latest.tag}, ${releases.length} bilingual release entries`);
