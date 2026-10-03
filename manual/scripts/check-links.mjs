import { readFile, readdir, stat } from 'node:fs/promises';
import assert from 'node:assert/strict';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { collectHtmlLinks } from './html-links.mjs';

const defaultDist = new URL('../dist/', import.meta.url);
const origin = new URL('https://daftAI2026.github.io').origin;
const base = '/incodex/';
const requiredResources = [
  'llms.txt',
  'llms-full.txt',
  'pagefind/pagefind.js',
  'en/commands/open/index.md',
  'zh/commands/open/index.md',
];

export async function checkBuiltLinks(dist = defaultDist) {
  const distUrl = dist instanceof URL ? dist : pathToFileURL(`${resolve(dist)}/`);
  const files = await readdir(distUrl, { recursive: true });
  const html = files.filter(file => file.endsWith('.html'));
  const documents = new Map();
  let links = 0;

  async function documentAt(path) {
    const key = path.href;
    if (!documents.has(key)) {
      const source = await readFile(path, 'utf8');
      documents.set(key, collectHtmlLinks(source));
    }
    return documents.get(key);
  }

  for (const file of html) {
    const sourcePath = new URL(file, distUrl);
    const { links: pageLinks } = await documentAt(sourcePath);
    const from = new URL(base + file.replace(/index\.html$/, ''), origin);

    for (const raw of pageLinks) {
      if (/^(data:|mailto:|tel:|javascript:)/i.test(raw)) continue;
      const url = new URL(raw, from);
      if (url.origin !== origin) continue;
      assert(url.pathname.startsWith(base), `${file}: link escapes GitHub Pages base: ${raw}`);
      let target = decodeURIComponent(url.pathname.slice(base.length));
      if (!target || target.endsWith('/')) target += 'index.html';
      const path = new URL(target, distUrl);
      try {
        await stat(path);
      } catch {
        assert.fail(`${file}: missing link or asset ${raw}`);
      }
      if (url.hash && target.endsWith('.html')) {
        const { ids } = await documentAt(path);
        const id = decodeURIComponent(url.hash.slice(1));
        assert(ids.has(id), `${file}: missing fragment ${raw}`);
      }
      links++;
    }
  }

  assert(links > html.length, 'HTML link inventory was not recognized');
  for (const resource of requiredResources) await stat(new URL(resource, distUrl));
  return { htmlPages: html.length, links };
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
  const { htmlPages, links } = await checkBuiltLinks();
  console.log(`Built manual: ${htmlPages} HTML pages, ${links} local links/assets and agent/search endpoints checked`);
}
