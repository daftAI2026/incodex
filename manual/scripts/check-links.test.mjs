import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { parse } from 'parse5';
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

function attributes(node) {
  return Object.fromEntries((node.attrs ?? []).map(({ name, value }) => [name, value]));
}

function elements(node, predicate, matches = []) {
  if (predicate(node)) matches.push(node);
  for (const child of node.childNodes ?? []) elements(child, predicate, matches);
  return matches;
}

function textContent(node) {
  return elements(node, child => child.nodeName === '#text')
    .map(child => child.value)
    .join(' ')
    .replace(/\s+/g, ' ')
    .trim();
}

async function assertBuiltSidebar(locale) {
  const html = await readFile(new URL(`../dist/${locale.page}`, import.meta.url), 'utf8');
  const document = parse(html);
  const sidebars = elements(document, node => node.tagName === 'div' && 'data-nb-sidebar' in attributes(node));
  assert.equal(sidebars.length, 2, `${locale.code}: desktop and mobile sidebars render`);

  for (const [index, sidebar] of sidebars.entries()) {
    const topLevel = elements(sidebar, node => node.tagName === 'ul' && attributes(node).class?.split(/\s+/).includes('top-level'))[0];
    assert.ok(topLevel, `${locale.code}: sidebar ${index + 1} has its top-level list`);
    const labels = (topLevel.childNodes ?? [])
      .filter(node => node.tagName === 'li')
      .flatMap(item => elements(item, node => 'data-nb-sidebar-group-label' in attributes(node)))
      .map(textContent);

    assert.ok(!labels.includes(locale.languageGroup), `${locale.code}: no redundant language group`);
    for (const group of locale.businessGroups) {
      assert.ok(labels.includes(group), `${locale.code}: preserves ${group} group`);
    }

    const anchors = elements(sidebar, node => node.tagName === 'a');
    const localLinks = anchors.map(node => attributes(node).href).filter(href => href?.startsWith('/incodex/'));
    assert.ok(localLinks.length > 0, `${locale.code}: sidebar contains local navigation`);
    assert.ok(localLinks.every(href => href.startsWith(`/incodex/${locale.code}/`)), `${locale.code}: sidebar stays in its language`);
    assert.ok(anchors.some(node => {
      const attrs = attributes(node);
      return attrs.href === locale.currentPage && attrs['aria-current'] === 'page';
    }), `${locale.code}: current page stays active`);
  }

  const header = elements(document, node => node.tagName === 'header')[0];
  const languageLinks = elements(header, node => node.tagName === 'a' && 'hreflang' in attributes(node));
  assert.ok(languageLinks.some(node => attributes(node).href === locale.otherLanguagePage), `${locale.code}: header language switch stays paired`);
}

const builtSidebarLocales = [
  {
    code: 'en',
    page: 'en/commands/open/index.html',
    currentPage: '/incodex/en/commands/open/',
    languageGroup: 'English',
    businessGroups: ['Command reference', 'Release notes'],
    otherLanguagePage: '/incodex/zh/commands/open/',
  },
  {
    code: 'zh',
    page: 'zh/commands/open/index.html',
    currentPage: '/incodex/zh/commands/open/',
    languageGroup: '简体中文',
    businessGroups: ['命令参考', '版本说明'],
    otherLanguagePage: '/incodex/en/commands/open/',
  },
];

for (const locale of builtSidebarLocales) {
  test(`built ${locale.code} desktop and mobile sidebars show the selected language directly`, async () => {
    await assertBuiltSidebar(locale);
  });
}
