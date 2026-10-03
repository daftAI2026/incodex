import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { checkBuiltLinks } from './check-links.mjs';

const requiredFiles = [
  'llms.txt',
  'llms-full.txt',
  'pagefind/pagefind.js',
  'en/commands/open/index.md',
  'zh/commands/open/index.md',
];

async function checkFixture(html, targetFiles) {
  const root = await mkdtemp(join(tmpdir(), 'incodex-check-links-'));
  const dist = join(root, 'dist');
  try {
    await mkdir(dist, { recursive: true });
    await writeFile(join(dist, 'index.html'), `${html}\n<a href="/incodex/llms.txt">Agent docs</a>`);
    for (const relative of [...requiredFiles, ...targetFiles]) {
      const target = join(dist, relative);
      await mkdir(dirname(target), { recursive: true });
      await writeFile(target, 'fixture');
    }
    return await checkBuiltLinks(dist);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}

test('only real HTML attributes count, not comments, inline JS or fake script markup', async () => {
  const result = await checkFixture(`<!doctype html>
    <link rel="canonical" href="https://daftAI2026.github.io/outside/">
    <a href="/incodex/docs/">Docs</a>
    <img src="/incodex/assets/pixel.svg">
    <!-- <a href="/incodex/comment-ghost/">not real</a> -->
    <script>
      e.href=a.url;
      const fake = '<img src="/incodex/script-ghost.svg">';
    </script>`, ['docs/index.html', 'assets/pixel.svg']);
  assert.equal(result.links, 3);
});

test('unquoted attributes and HTML character references resolve like browser links', async () => {
  const result = await checkFixture(`<!doctype html>
    <a href=/incodex/encoded&#x2F;page/>Encoded path</a>
    <img src=/incodex/assets/pixel.svg>
    <a href="/incodex/search/?q=one&amp;path=two">Search</a>`, [
    'encoded/page/index.html', 'assets/pixel.svg', 'search/index.html',
  ]);
  assert.equal(result.links, 4);
});

test('canonical links are omitted regardless of attribute order and quoting', async () => {
  const result = await checkFixture(`<!doctype html>
    <link href=/outside/canonical/ rel=canonical>
    <a href=/incodex/docs/>Docs</a>
    <img src=/incodex/assets/pixel.svg>`, ['docs/index.html', 'assets/pixel.svg']);
  assert.equal(result.links, 3);
});
