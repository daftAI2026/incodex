/**
 * [INPUT]: 读取双语指南图、当前行为文案、权限说明和构建页面。
 * [OUTPUT]: 验证图片引用、版本边界、权限名和稳定版导航。
 * [POS]: scripts 的用户指南回归门；源码约束语义，dist 约束实际页面。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import assert from 'node:assert/strict';
import { readdir, readFile } from 'node:fs/promises';
import { test } from 'node:test';
import { parse } from 'parse5';

const manualRoot = new URL('../', import.meta.url);

async function readManual(path) {
  return readFile(new URL(path, manualRoot), 'utf8');
}

async function listMdx(directory) {
  const entries = await readdir(new URL(`${directory}/`, manualRoot), { withFileTypes: true });
  const nested = await Promise.all(entries.map(async entry => {
    const path = `${directory}/${entry.name}`;
    if (entry.isDirectory() && entry.name !== 'releases') return listMdx(path);
    return entry.isFile() && entry.name.endsWith('.mdx') ? [path] : [];
  }));
  return nested.flat();
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
      sidebar: /optional.*sidebar entry.*supported sidebar slot/i,
      banner: /home notification.*private window/i,
      lifecycle: /close.*clears.*session.*minimiz.*keeps/i,
    },
    numbered: {
      toolbar: /^1\./,
      sidebar: /^2\./,
      banner: /^3\./,
    },
    markers: /aria-label="Marker 1, toolbar control"[\s\S]*aria-label="Marker 2, optional sidebar entry"[\s\S]*aria-label="Marker 3, private home notice"/,
  },
  {
    locale: 'zh',
    path: 'public/images/window-guide-zh.svg',
    expected: {
      toolbar: /搜索工具栏操作组最前/,
      sidebar: /支持布局.*可选侧栏入口/,
      banner: /通知条只显示在私密窗口首页/,
      lifecycle: /正常关闭.*清理隔离会话.*最小化.*保留会话/,
    },
    numbered: {
      toolbar: /^1\./,
      sidebar: /^2\./,
      banner: /^3\./,
    },
    markers: /aria-label="编号 1，工具栏入口"[\s\S]*aria-label="编号 2，可选侧栏入口"[\s\S]*aria-label="编号 3，私密首页通知"/,
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
    for (const [role, pattern] of Object.entries(diagram.numbered)) {
      assert.match(copy.get(role) ?? '', pattern, `${diagram.locale} ${role} callout number`);
    }
    assert.match(svg, diagram.markers, `${diagram.locale} markers identify each callout target`);
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
    accessibility: /Accessibility.*Device Control and Data Access.*macOS 27\+.*macOS 26.*Accessibility/s,
    screen: /Screen Recording.*macOS 27\+ and macOS 26 lists are \*\*Screen & System Audio Recording\*\*.*macOS 26 guide.*bundled row name \*\*Screen Recording\*\*/s,
    caption: /macOS 27\+ labels.*Older guides keep their bundled row names/,
    official: /support\.apple\.com\/guide\/mac-help\/mh43185\/26\/mac\/26.*support\.apple\.com\/guide\/mac-help\/control-access-screen-system-audio-recording-mchld6aa7d23\/26\/mac\/26/s,
    noOldNameClaim: /macOS 26 and earlier use Accessibility and Screen Recording/,
  },
  {
    locale: 'zh',
    path: 'src/content/docs/zh/permissions.mdx',
    heading: /^## 设置已安装应用权限$/m,
    accessibility: /辅助功能.*设备控制和数据访问.*macOS 27\+.*macOS 26.*辅助功能/s,
    screen: /屏幕录制.*macOS 27\+ 和 macOS 26 列表名均为“录屏与系统录音”。macOS 26 向导.*内置行名“屏幕录制”/s,
    caption: /macOS 27\+ 行名.*旧版向导保留内置行名/,
    official: /support\.apple\.com\/zh-cn\/guide\/mac-help\/mh43185\/26\/mac\/26.*support\.apple\.com\/zh-cn\/guide\/mac-help\/mchld6aa7d23\/26\/mac\/26/s,
    noOldNameClaim: /macOS 26 及更早版本显示“辅助功能”和“屏幕录制”/,
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
    assert.match(setup, page.caption, `${page.locale} image caption distinguishes guide labels`);
    assert.match(setup, page.official, `${page.locale} links to the official macOS 26 list names`);
    assert.doesNotMatch(setup, page.noOldNameClaim, `${page.locale} does not generalize older System Settings names`);
    assert.match(setup, /System Settings|系统设置/);
  });
}

for (const locale of ['en', 'zh']) {
  test(`${locale} image guides use semantic captions and keep diagrams inline`, async () => {
    for (const page of ['index', 'launchers', 'permissions', 'windows']) {
      const source = await readManual(`src/content/docs/${locale}/${page}.mdx`);
      assert.match(source, /<figure>\s*<img\b[\s\S]*?<figcaption>[\s\S]*?<\/figcaption>\s*<\/figure>/,
        `${locale}/${page}: image has a semantic caption`);
    }

    const windows = await readManual(`src/content/docs/${locale}/windows.mdx`);
    const diagramPath = `window-guide-${locale}.svg`;
    assert.match(windows, new RegExp(`<img\\b[^>]*src="/incodex/images/${diagramPath}"`),
      `${locale}: the complete position diagram remains inline`);
    assert.doesNotMatch(windows, /(?:full-size diagram|完整示意图)/i,
      `${locale}: do not add a redundant full-size link`);

    const html = await readManual(`dist/${locale}/windows/index.html`);
    const document = parse(html);
    const anchors = elements(document, node => node.tagName === 'a');
    assert.ok(!anchors.some(node => /(?:full-size diagram|完整示意图)/i.test(textContent(node))),
      `${locale}: built HTML has no redundant full-size link`);
    assert.ok(elements(document, node => node.tagName === 'img')
      .some(node => attributes(node).src === `/incodex/images/${diagramPath}`),
    `${locale}: built HTML includes the complete diagram`);
  });
}

for (const locale of ['en', 'zh']) {
  test(`${locale} current guides describe behavior without release-history labels`, async () => {
    const files = await listMdx(`src/content/docs/${locale}`);
    const [{ version: stableVersion }] = JSON.parse(await readManual('releases.json'));
    const stablePattern = stableVersion.replaceAll('.', '\\.');
    const historyLabel = locale === 'en'
      ? /\b(?:added|fixed|available)\s+in(?:\s+incodex)?\s+v\d+\.\d+\.\d+|\bsince\s+(?:incodex\s+)?v\d+\.\d+\.\d+/i
      : /(?:v\d+\.\d+\.\d+\s*(?:已修复|新增|起)|(?:自|从)\s*(?:Incodex\s*)?v\d+\.\d+\.\d+\s*(?:起|提供))/i;
    const staleStableLabel = new RegExp(
      `${locale === 'en' ? '\\bStable' : '稳定版'}\\s+v(?!${stablePattern})\\d+\\.\\d+\\.\\d+`, 'i',
    );

    for (const path of files) {
      const source = await readManual(path);
      assert.doesNotMatch(source, historyLabel, `${path}: current guides do not repeat release chronology`);
      assert.doesNotMatch(source, staleStableLabel, `${path}: stable-version labels match the public baseline`);
    }
  });
}

for (const page of [
  {
    locale: 'en',
    path: 'src/content/docs/en/windows.mdx',
    oldLabels: /(?:Added in v1\.3\.0|Additional sidebar entry \(v1\.3\.0\)|pinned-plugin group|sidebar-rail slot)/i,
    behavior: /optional sidebar entry[\s\S]*supported layouts[\s\S]*left navigation rail[\s\S]*toolbar control remains available[\s\S]*verified on macOS[\s\S]*not yet been verified in a running Windows Store Codex window/i,
  },
  {
    locale: 'zh',
    path: 'src/content/docs/zh/windows.mdx',
    oldLabels: /(?:v1\.3\.0 新增|新增侧栏入口（v1\.3\.0）|原生插件固定区|侧栏位置)/,
    behavior: /可选侧栏入口[\s\S]*支持(?:该入口的)?布局[\s\S]*左侧导航栏[\s\S]*工具栏入口[\s\S]*macOS 已验证[\s\S]*尚未在运行中的 Windows Store Codex 窗口验证/,
  },
]) {
  test(`${page.locale} sidebar instructions describe user behavior and retain verification limits`, async () => {
    const source = await readManual(page.path);
    assert.doesNotMatch(source, page.oldLabels, `${page.locale}: no release label or layout implementation detail`);
    assert.match(source, page.behavior, `${page.locale}: preserve fallback and platform verification boundary`);

    const html = await readManual(`dist/${page.locale}/windows/index.html`);
    const document = parse(html);
    assert.match(textContent(document), page.behavior, `${page.locale}: built page presents the same behavior`);
  });
}

const launcherImageCases = [
  {
    locale: 'en',
    launcherPath: 'src/content/docs/en/launchers.mdx',
    windowsPath: 'src/content/docs/en/windows.mdx',
    currentHeading: /^### Current stable v1\.5\.8 entry$/m,
    candidateHeading: /^## Unreleased: move the entry to `\/Applications`$/m,
    candidateEnd: /^## Host-based launcher icon$/m,
    currentPath: /`~\/Applications\/ChatGPT - Inc\.app`/,
    candidatePath: /`\/Applications\/ChatGPT - Inc\.app`/,
    candidateImage: 'launcher-applications-entry.png',
    cropImage: 'host-hat-control.png',
    finderImage: 'launcher-finder-entry.png',
  },
  {
    locale: 'zh',
    launcherPath: 'src/content/docs/zh/launchers.mdx',
    windowsPath: 'src/content/docs/zh/windows.mdx',
    currentHeading: /^### 当前稳定版 v1\.5\.8 入口$/m,
    candidateHeading: /^## 未发布：入口将迁至 `\/Applications`$/m,
    candidateEnd: /^## 宿主图标启动器$/m,
    currentPath: /`~\/Applications\/ChatGPT - Inc\.app`/,
    candidatePath: /`\/Applications\/ChatGPT - Inc\.app`/,
    candidateImage: 'launcher-applications-entry.png',
    cropImage: 'host-hat-control.png',
    finderImage: 'launcher-finder-entry.png',
  },
];

for (const page of launcherImageCases) {
  test(`built ${page.locale} launcher separates stable and unpublished Finder examples`, async () => {
    const source = await readManual(page.launcherPath);
    const currentStart = source.search(page.currentHeading);
    const candidateStart = source.search(page.candidateHeading);
    const candidateEnd = source.slice(candidateStart).search(page.candidateEnd);
    assert.ok(currentStart >= 0, `${page.locale}: stable entry heading exists`);
    assert.ok(candidateStart > currentStart, `${page.locale}: migration follows stable instructions`);
    assert.ok(candidateEnd > 0, `${page.locale}: candidate section has a boundary`);

    const stableSection = source.slice(currentStart, candidateStart);
    const candidateSection = source.slice(candidateStart, candidateStart + candidateEnd);
    assert.match(stableSection, page.currentPath, `${page.locale}: stable path remains user-scoped`);
    assert.match(stableSection, new RegExp(page.finderImage));
    assert.doesNotMatch(stableSection, new RegExp(page.candidateImage));
    assert.match(candidateSection, page.candidatePath, `${page.locale}: candidate path is system-wide`);
    assert.match(candidateSection, new RegExp(page.candidateImage));
    assert.doesNotMatch(candidateSection, /Go to Folder|前往 › 前往文件夹/,
      `${page.locale}: current stable Finder steps stay outside the candidate section`);

    const html = await readManual(`dist/${page.locale}/launchers/index.html`);
    const document = parse(html);
    const images = elements(document, node => node.tagName === 'img').map(node => attributes(node).src);
    assert.equal(images.filter(src => src === `/incodex/images/${page.finderImage}`).length, 1,
      `${page.locale}: stable Finder example remains available once`);
    assert.equal(images.filter(src => src === `/incodex/images/${page.candidateImage}`).length, 1,
      `${page.locale}: built page publishes the candidate Finder image once`);
    await readFile(new URL(`dist/images/${page.candidateImage}`, manualRoot));
  });

  test(`built ${page.locale} window guide shows the real toolbar crop before its diagram`, async () => {
    const source = await readManual(page.windowsPath);
    const cropAt = source.indexOf(page.cropImage);
    const diagramAt = source.indexOf(`window-guide-${page.locale}.svg`);
    assert.ok(cropAt >= 0 && cropAt < diagramAt,
      `${page.locale}: actual toolbar crop precedes the position diagram`);
    const html = await readManual(`dist/${page.locale}/windows/index.html`);
    const document = parse(html);
    const images = elements(document, node => node.tagName === 'img').map(node => attributes(node).src);
    const cropSrc = `/incodex/images/${page.cropImage}`;
    const diagramSrc = `/incodex/images/window-guide-${page.locale}.svg`;
    assert.ok(images.indexOf(cropSrc) >= 0 && images.indexOf(cropSrc) < images.indexOf(diagramSrc),
      `${page.locale}: built HTML preserves screenshot-before-diagram order`);
    await readFile(new URL(`dist/images/${page.cropImage}`, manualRoot));
  });
}

test('README distinguishes verified v1.5.8 Finder use from the unpublished system-path candidate', async () => {
  const readme = await readManual('../README.md');
  assert.doesNotMatch(readme, /installed Finder display\/click acceptance is still pending/i);
  assert.match(readme, /stable v1\.5\.8[\s\S]{0,180}user-folder launcher[\s\S]{0,180}visible in Finder/i);
  assert.match(readme, /macOS `open` reuses the live Codex host when asked to open that entry/i);
  assert.match(readme, /unreleased[\s\S]{0,220}(?:candidate|migration)[\s\S]{0,220}(?:not|has not)[\s\S]{0,100}stable/i);
});

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

test('image captions use compact muted spacing without restyling code figures', async () => {
  const styles = await readManual('src/styles/prose.css');
  const captionRule = styles.match(/\.docs-content\s+figure:has\(>\s*img\)\s*>\s*figcaption\s*\{([^}]+)\}/);
  assert.ok(captionRule, 'image figures have a dedicated caption rule');
  assert.match(captionRule[1], /margin-top:\s*\.5rem/);
  assert.match(captionRule[1], /font-size:\s*\.8125rem/);
  assert.match(captionRule[1], /color:\s*var\(--nb-muted-foreground\)/);
  assert.match(styles, /\.docs-content\s+\.nb-code-figure\s*\{/,
    'image caption styling does not replace the existing code figure rule');
});
