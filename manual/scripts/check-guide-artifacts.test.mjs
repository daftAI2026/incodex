/**
 * [INPUT]: 读取双语窗口示意、权限说明和构建后的页面。
 * [OUTPUT]: 验证入口位置、权限列表、编辑链接与稳定版导航顺序。
 * [POS]: scripts 的用户指南回归门；源码约束语义，dist 约束实际页面。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { test } from 'node:test';
import { parse } from 'parse5';

const manualRoot = new URL('../', import.meta.url);

async function readManual(path) {
  return readFile(new URL(path, manualRoot), 'utf8');
}

function textContent(node) {
  const parts = [];
  const visit = current => {
    if (current.nodeName === '#text') parts.push(current.value);
    for (const child of current.childNodes ?? []) visit(child);
  };
  visit(node);
  return parts.join(' ').replace(/\s+/g, ' ').trim();
}

function elements(node, predicate, matches = []) {
  if (predicate(node)) matches.push(node);
  for (const child of node.childNodes ?? []) elements(child, predicate, matches);
  return matches;
}

function attributes(node) {
  return Object.fromEntries((node.attrs ?? []).map(({ name, value }) => [name, value]));
}

function diagramCopy(svg) {
  return new Map([...svg.matchAll(/<text\b[^>]*data-copy-role="([^"]+)"[^>]*>([\s\S]*?)<\/text>/g)]
    .map(([, role, copy]) => [role, copy.replace(/<[^>]+>/g, '').replaceAll('&amp;', '&').trim()]));
}

const diagrams = [
  {
    locale: 'en',
    path: 'public/images/window-guide-en.svg',
    expected: {
      toolbar: /leading edge of Search's toolbar action group/i,
      sidebar: /optional.*pinned plugins.*supported layout is recognized/i,
      banner: /home notification.*private window/i,
      lifecycle: /close.*clears.*session.*minimiz.*keeps/i,
    },
  },
  {
    locale: 'zh',
    path: 'public/images/window-guide-zh.svg',
    expected: {
      toolbar: /搜索工具栏操作组最前/,
      sidebar: /可选.*插件固定区首位.*识别到支持布局/,
      banner: /通知条只显示在私密窗口首页/,
      lifecycle: /正常关闭.*清理隔离会话.*最小化.*保留会话/,
    },
  },
];

for (const diagram of diagrams) {
  test(`${diagram.locale} window diagram distinguishes installed and private controls`, async () => {
    const svg = await readManual(diagram.path);
    const copy = diagramCopy(svg);
    assert.deepEqual([...copy.keys()].sort(), ['banner', 'lifecycle', 'sidebar', 'toolbar']);
    for (const [role, pattern] of Object.entries(diagram.expected)) {
      assert.match(copy.get(role) ?? '', pattern, `${diagram.locale} ${role} callout`);
    }
    assert.match(svg, /<title\b[^>]*>[^<]*<\/title>/);
    assert.match(svg, /<desc\b[^>]*>[^<]*<\/desc>/);
    assert.match(svg, /position diagram|位置示意/i);
    assert.match(svg, /not a screenshot|非截图/i);
  });
}

const permissionPages = [
  {
    locale: 'en',
    path: 'src/content/docs/en/permissions.mdx',
    heading: /^## Set up the installed app permissions$/m,
    accessibility: /Accessibility.*Device Control and Data Access.*macOS 27.*Accessibility.*macOS 26/s,
    screen: /Screen Recording.*Screen & System Audio Recording.*macOS 27.*Screen Recording.*macOS 26/s,
  },
  {
    locale: 'zh',
    path: 'src/content/docs/zh/permissions.mdx',
    heading: /^## 设置已安装应用权限$/m,
    accessibility: /辅助功能.*设备控制和数据访问.*macOS 27.*辅助功能（无障碍）.*macOS 26/s,
    screen: /屏幕录制.*录屏与系统录音.*macOS 27.*屏幕录制.*macOS 26/s,
  },
];

for (const page of permissionPages) {
  test(`${page.locale} permission instructions match each Allow row to its Settings list`, async () => {
    const source = await readManual(page.path);
    const setup = source.split(/^## (?:Screen Recording and `perm`|屏幕录制与 `perm`)$/m)[0];
    assert.match(source, page.heading);
    assert.doesNotMatch(source, /Stable v1\.5\.1|稳定版 v1\.5\.1/);
    assert.match(setup, page.accessibility, `${page.locale} Accessibility list names`);
    assert.match(setup, page.screen, `${page.locale} Screen Recording list names`);
    assert.match(setup, /System Settings|系统设置/);
  });
}

for (const locale of ['en', 'zh']) {
  test(`built ${locale} guide edit link points to its source file once`, async () => {
    const html = await readManual(`dist/${locale}/windows/index.html`);
    const document = parse(html);
    const anchors = elements(document, node => node.tagName === 'a');
    const editLabel = locale === 'en' ? 'Edit this page' : '编辑本页';
    const edit = anchors.find(node => textContent(node) === editLabel);
    const localePath = locale === 'en' ? 'en' : 'zh';
    assert.ok(edit, `${locale}: page edit link exists`);
    assert.equal(
      attributes(edit).href,
      `https://github.com/daftAI2026/incodex/edit/main/manual/src/content/docs/${localePath}/windows.mdx`,
      `${locale}: edit link points to the source file once`,
    );
  });

  test(`built ${locale} sidebar lists the current stable release first`, async () => {
    const html = await readManual(`dist/${locale}/windows/index.html`);
    const document = parse(html);
    const sidebars = elements(document, node => node.tagName === 'div' && 'data-nb-sidebar' in attributes(node));
    assert.ok(sidebars.length > 0, `${locale}: sidebar exists`);
    const currentSidebar = sidebars[0];
    const releaseLabel = locale === 'en' ? 'Release notes' : '版本说明';
    const labelNode = elements(currentSidebar, node => 'data-nb-sidebar-group-label' in attributes(node))
      .find(node => textContent(node) === releaseLabel);
    assert.ok(labelNode, `${locale}: release group exists`);
    let releaseGroup = labelNode;
    while (releaseGroup && !('data-nb-sidebar-group' in attributes(releaseGroup))) releaseGroup = releaseGroup.parentNode;
    assert.ok(releaseGroup, `${locale}: release group has a container`);
    const releaseLinks = elements(releaseGroup, node => node.tagName === 'a')
      .map(node => attributes(node).href)
      .filter(href => /\/releases\/v\d+\.\d+\.\d+\/$/.test(href));
    assert.match(releaseLinks[0] ?? '', /\/releases\/v1\.5\.8\/$/, `${locale}: current stable release is first`);
  });
}
