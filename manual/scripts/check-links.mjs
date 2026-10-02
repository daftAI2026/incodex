import { readFile, readdir, stat } from 'node:fs/promises';
import assert from 'node:assert/strict';

const dist = new URL('../dist/', import.meta.url);
const origin = new URL('https://daftAI2026.github.io').origin;
const base = '/incodex/';
const files = await readdir(dist, { recursive: true });
const html = files.filter(file => file.endsWith('.html'));
let links = 0;
for (const file of html) {
  const source = (await readFile(new URL(file, dist), 'utf8')).replace(/<link\b[^>]*\brel="canonical"[^>]*>/g, '');
  const from = new URL(base + file.replace(/index\.html$/, ''), origin);
  for (const match of source.matchAll(/(?:href|src)=(?:"([^"<>]+)"|'([^'<>]+)'|([^\s>]+))/g)) {
    const raw = match[1] ?? match[2] ?? match[3];
    if (/^(data:|mailto:|tel:|javascript:)/.test(raw)) continue;
    const url = new URL(raw.replaceAll('&amp;', '&'), from);
    if (url.origin !== origin) continue;
    assert(url.pathname.startsWith(base), `${file}: link escapes GitHub Pages base: ${raw}`);
    let target = decodeURIComponent(url.pathname.slice(base.length));
    if (!target || target.endsWith('/')) target += 'index.html';
    const path = new URL(target, dist);
    try { await stat(path); } catch { assert.fail(`${file}: missing link or asset ${raw}`); }
    if (url.hash && target.endsWith('.html')) {
      const body = await readFile(path, 'utf8');
      const id = decodeURIComponent(url.hash.slice(1));
      assert(body.includes(`id="${id}"`) || body.includes(`id=${id}>`) || body.includes(`id=${id} `), `${file}: missing fragment ${raw}`);
    }
    links++;
  }
}
assert(links > html.length, 'HTML link inventory was not recognized');
for (const resource of ['llms.txt', 'llms-full.txt', 'pagefind/pagefind.js', 'en/commands/open/index.md', 'zh/commands/open/index.md']) {
  await stat(new URL(resource, dist));
}
console.log(`Built manual: ${html.length} HTML pages, ${links} local links/assets and agent/search endpoints checked`);
