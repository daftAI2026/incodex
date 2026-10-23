/**
 * [INPUT]: 读取双语窗口 SVG 与原生裁剪的 Finder 图片。
 * [OUTPUT]: 验证入口属于左侧栏，通知在输入框上方。
 * [POS]: 公共图示回归门，防止文字正确但位置示意错误。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { test } from 'node:test';
import { parse } from 'parse5';

const root = new URL('../', import.meta.url);
const flatten = node => [node, ...(node.childNodes ?? []).flatMap(flatten)];
for (const locale of ['en', 'zh']) {
  test(`${locale} diagram follows observed sidebar and home-composer geometry`, async () => {
    const svg = await readFile(new URL(`public/images/window-guide-${locale}.svg`, root), 'utf8');
    const nodes = flatten(parse(svg));
    const bounds = role => {
      const matches = nodes.filter(node => node.attrs?.some(a => a.name === 'data-layout-role' && a.value === role));
      assert.equal(matches.length, 1, `${role} must have one position`);
      const attrs = Object.fromEntries(matches[0].attrs.map(a => [a.name, a.value]));
      return Object.fromEntries(['x', 'y', 'width', 'height'].map(k => [k, Number(attrs[k])]));
    };
    for (const mode of ['normal', 'private']) {
      const sidebar = bounds(`${mode}-sidebar`);
      const toolbar = bounds(`${mode}-toolbar-hat`);
      const rail = bounds(`${mode}-rail-hat`);
      assert.ok(toolbar.x >= sidebar.x && toolbar.x + toolbar.width <= sidebar.x + sidebar.width, 'toolbar hat belongs inside the left sidebar header, not the workspace');
      assert.ok(toolbar.y < sidebar.y + 70, 'toolbar hat is at the sidebar top');
      assert.ok(rail.x + rail.width < sidebar.x && rail.y > toolbar.y, 'optional hat is below the header in the narrow left rail');
    }
    const banner = bounds('private-notice');
    const composer = bounds('private-composer');
    assert.equal(banner.x, composer.x, 'notice and composer share the home content column');
    assert.equal(banner.width, composer.width);
    assert.ok(banner.y + banner.height <= composer.y, 'home notice is above the composer');
    assert.ok(composer.y - banner.y - banner.height < 70, 'notice stays near the composer, not at the page top');
  });
}
test('Finder crop contains only the two adjacent host and launcher entries', async () => {
  const image = await readFile(new URL('public/images/launcher-applications-entry.png', root));
  assert.equal(image.readUInt32BE(16), 470);
  assert.equal(image.readUInt32BE(20), 216);
});
