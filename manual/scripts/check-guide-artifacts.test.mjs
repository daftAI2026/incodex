/**
 * [INPUT]: 读取双语指南图、当前行为文案、权限说明和构建页面。
 * [OUTPUT]: 验证双语指南、发布说明副本、权限名和签名授权合同；历史内容与当前稳定基线分开。
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

const deviceKeyApprovalContracts = [
  {
    locale: 'en',
    activation: /Compatible Runtime updates activate the signing adapter in the current host without requiring a host restart\./,
    readiness: /A visible hat button or Runtime update notice does not prove the native adapter is ready or Keychain access is authorized\./,
    continuity: /Authorization continuity after a real official Codex update has not been tested\./,
    repeatedPrompt: /Do not treat a repeated prompt for the same unchanged key\/helper pair as normal\./,
    creationBoundary: /These startup instructions describe key creation, not the separate device-key signing path below\./,
  },
  {
    locale: 'zh',
    activation: /兼容的 Runtime 更新会在当前宿主中激活签名适配器，不要求重启宿主。/,
    readiness: /按钮出现或 Runtime 更新提示不代表原生适配器已就绪或密钥已授权。/,
    continuity: /真实官方 Codex 更新后的授权连续性尚未测试。/,
    repeatedPrompt: /相同密钥与 helper 未变却再次弹窗时，不要视为正常。/,
    creationBoundary: /这些启动步骤只描述密钥创建，不适用于下方独立的设备密钥签名路径。/,
  },
];

for (const contract of deviceKeyApprovalContracts) {
  test(`${contract.locale} device-key guide preserves the activation and approval boundary`, async () => {
    const permissions = await readManual(`src/content/docs/${contract.locale}/permissions.mdx`);
    const troubleshooting = await readManual(`src/content/docs/${contract.locale}/troubleshooting.mdx`);
    assert.match(permissions, contract.activation, `${contract.locale}: compatible Runtime updates activate in the current host`);
    assert.match(permissions, contract.readiness, `${contract.locale}: UI does not prove native or Keychain readiness`);
    assert.match(permissions, contract.continuity, `${contract.locale}: official-update authorization continuity stays untested`);
    assert.match(troubleshooting, contract.creationBoundary, `${contract.locale}: key-creation restart steps stay separate`);
    assert.match(troubleshooting, contract.repeatedPrompt, `${contract.locale}: repeated approval prompts are not normalized`);

    const permissionsHtml = await readManual(`dist/${contract.locale}/permissions/index.html`);
    const troubleshootingHtml = await readManual(`dist/${contract.locale}/troubleshooting/index.html`);
    assert.match(textContent(parse(permissionsHtml)), contract.activation);
    assert.match(textContent(parse(permissionsHtml)), contract.readiness);
    assert.match(textContent(parse(permissionsHtml)), contract.continuity);
    assert.match(textContent(parse(troubleshootingHtml)), contract.creationBoundary);
    assert.match(textContent(parse(troubleshootingHtml)), contract.repeatedPrompt);
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

const launcherCases = [
  {
    locale: 'en',
    homePath: 'src/content/docs/en/index.mdx',
    launcherPath: 'src/content/docs/en/launchers.mdx',
    commandPath: 'src/content/docs/en/commands/launch.mdx',
    windowsPath: 'src/content/docs/en/windows.mdx',
    migrationHeading: /^### Migrate older user-folder entries$/m,
    defaultPath: /`\/Applications\/ChatGPT - Inc\.app`/,
    builtDefault: /\/Applications\/ChatGPT - Inc\.app/,
    legacyPaths: /`~\/Applications\/ChatGPT - Inc\.app`[\s\S]*`~\/Applications\/Incodex Launcher\.app`/,
    commandMigration: /migrat(?:e|es|ion)[\s\S]{0,180}`~\/Applications\/ChatGPT - Inc\.app`[\s\S]*`~\/Applications\/Incodex Launcher\.app`/i,
    dockBehavior: /follows the selected ChatGPT or Codex Dock icon style[\s\S]*falls back to the host's file icon/i,
    builtDockBehavior: /follows the selected ChatGPT or Codex Dock icon style[\s\S]*falls back to the host[’']s file icon/i,
    refreshBehavior: /Run `inc update`, then `incodex launch`[\s\S]*does not watch for icon changes in the background/i,
    builtRefreshBehavior: /Run\s+inc update\s*, then\s+incodex launch[\s\S]*does not watch for changes in the background/i,
    noRestart: /does not require re-signing, reinstalling, or restarting the host/i,
    legacySafety: /does not create `\/Applications`, fall back to a user entry, or request `sudo`/i,
    currentImage: 'launcher-applications-entry.png',
    obsoleteImage: 'launcher-finder-entry.png',
    cropImage: 'host-hat-control.png',
  },
  {
    locale: 'zh',
    homePath: 'src/content/docs/zh/index.mdx',
    launcherPath: 'src/content/docs/zh/launchers.mdx',
    commandPath: 'src/content/docs/zh/commands/launch.mdx',
    windowsPath: 'src/content/docs/zh/windows.mdx',
    migrationHeading: /^### 迁移旧用户目录入口$/m,
    defaultPath: /`\/Applications\/ChatGPT - Inc\.app`/,
    builtDefault: /\/Applications\/ChatGPT - Inc\.app/,
    legacyPaths: /`~\/Applications\/ChatGPT - Inc\.app`[\s\S]*`~\/Applications\/Incodex Launcher\.app`/,
    commandMigration: /(?:迁移|移除)[\s\S]{0,180}`~\/Applications\/ChatGPT - Inc\.app`[\s\S]*`~\/Applications\/Incodex Launcher\.app`/,
    dockBehavior: /跟随所选 ChatGPT 或 Codex Dock 图标样式[\s\S]*图标资源不可用时.*回退到宿主文件图标/,
    builtDockBehavior: /跟随 ChatGPT 或 Codex 所选的 Dock 图标样式[\s\S]*图标资源不可用时.*回退到宿主文件图标/,
    refreshBehavior: /运行 `inc update`，再运行 `incodex launch`[\s\S]*不会在后台监听图标变化/,
    builtRefreshBehavior: /运行\s+inc update\s*，再运行\s+incodex launch[\s\S]*不会在后台监听变化/,
    noRestart: /不需要重签、重装或重启宿主/,
    legacySafety: /不会创建 `\/Applications`、回退到用户目录或请求 `sudo`/,
    currentImage: 'launcher-applications-entry.png',
    obsoleteImage: 'launcher-finder-entry.png',
    cropImage: 'host-hat-control.png',
  },
];

for (const page of launcherCases) {
  test(`built ${page.locale} launcher documents the shared Applications entry`, async () => {
    const home = await readManual(page.homePath);
    const source = await readManual(page.launcherPath);
    const command = await readManual(page.commandPath);
    const migrationAt = source.search(page.migrationHeading);

    assert.match(home, page.defaultPath, `${page.locale}: home names the shared launcher path`);
    assert.match(source, page.defaultPath, `${page.locale}: launcher guide names the shared path`);
    assert.match(command, page.defaultPath, `${page.locale}: command guide names the shared path`);
    assert.match(command, page.commandMigration, `${page.locale}: command guide explains exact migration sources`);
    assert.ok(migrationAt > 0, `${page.locale}: old user entries are isolated in a migration section`);
    assert.match(source.slice(migrationAt), page.legacyPaths,
      `${page.locale}: migration lists both recognized old user paths`);
    assert.doesNotMatch(source.slice(0, migrationAt), /`~\/Applications\//,
      `${page.locale}: user-folder paths are not current creation instructions`);
    assert.match(source, page.dockBehavior, `${page.locale}: selected Dock style and fallback are explicit`);
    assert.match(source, page.refreshBehavior, `${page.locale}: refresh is user-triggered, not watched`);
    assert.match(source, page.noRestart, `${page.locale}: host lifecycle requirements are accurate`);
    assert.match(source, page.legacySafety, `${page.locale}: unsafe fallback and privilege escalation are excluded`);
    assert.match(source, new RegExp(page.currentImage));
    assert.doesNotMatch(source, new RegExp(page.obsoleteImage),
      `${page.locale}: current guide does not reuse the old user-folder screenshot`);
    assert.doesNotMatch(home, /Unreleased|未发布/,
      `${page.locale}: home does not present the current path as an unpublished candidate`);

    const homeHtml = await readManual(`dist/${page.locale}/index.html`);
    const commandHtml = await readManual(`dist/${page.locale}/commands/launch/index.html`);
    const homeText = textContent(parse(homeHtml));
    assert.match(homeText, page.builtDefault,
      `${page.locale}: built home names the shared launcher path`);
    assert.match(homeText, page.builtDockBehavior,
      `${page.locale}: built home describes Dock icon selection and fallback`);
    assert.match(homeText, page.builtRefreshBehavior,
      `${page.locale}: built home describes explicit refresh`);
    assert.match(homeText, page.locale === 'en'
      ? /does not require re-signing, reinstalling, or restarting the host/i
      : /无须重签、重装或重启宿主/,
    `${page.locale}: built home states host lifecycle requirements`);
    assert.match(textContent(parse(commandHtml)), page.builtDefault,
      `${page.locale}: built command guide names the shared launcher path`);

    const html = await readManual(`dist/${page.locale}/launchers/index.html`);
    const document = parse(html);
    const images = elements(document, node => node.tagName === 'img').map(node => attributes(node).src);
    assert.equal(images.filter(src => src === `/incodex/images/${page.currentImage}`).length, 1,
      `${page.locale}: built page shows the current shared entry once`);
    assert.equal(images.filter(src => src === `/incodex/images/${page.obsoleteImage}`).length, 0,
      `${page.locale}: built page omits the obsolete user-folder screenshot`);
    await readFile(new URL(`dist/images/${page.currentImage}`, manualRoot));
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

test('README documents the shared Applications launcher and migration-only user paths', async () => {
  const readme = await readManual('../README.md');
  assert.doesNotMatch(readme, /installed Finder display\/click acceptance is still pending/i);
  assert.match(readme, /`\/Applications\/ChatGPT - Inc\.app`/);
  assert.match(readme, /migrates exact Incodex-owned entries from `~\/Applications`, preserving unknown or other-user entries/i);
  assert.match(readme, /follows the host's selected ChatGPT\/Codex Dock style[\s\S]{0,160}if the selection is unavailable, it uses the host's file icon/i);
  assert.match(readme, /Run `inc update`[\s\S]{0,120}then `incodex launch`/i);
  assert.match(readme, /without re-signing, reinstalling, or restarting Codex/i);
});

const v1510ReleaseCopy = {
  en: `### Changelog

1. **macOS Remote Control authorization**: Device-key signing uses a fixed helper whose identity is retained across compatible Runtime updates. A complete signing request can wait for system approval without the five-minute cutoff; denial and cancellation remain failures.
2. **Separate Keychain guidance**: Guidance for default new macOS installs now distinguishes the Codex Storage Key prompt from the Remote Control \`incodex-device-key-helper\` prompt. Enter your Mac login password only in the macOS dialog. Always Allow applies only to the current key/helper pair; a different key or helper identity may need approval again.
3. **Hat-glasses entry returns to the updated Sidebar**: The entry now appears first in the recognized Sidebar, and its tooltip reuses Codex's Tooltip component, layer, and appearance.

Upgrade with \`inc update\` to publish the updated Runtime.`,
  zh: `### 更新日志

1. **macOS 远控授权**：远控签名使用固定的设备密钥 helper，兼容的 Runtime 更新不更换其身份。完整签名请求提交后，等待系统授权不再被五分钟期限提前终止；拒绝与取消仍按原结果结束。
2. **区分钥匙串授权提示**：macOS 默认新安装的说明现在区分 Codex Storage Key 与远控 \`incodex-device-key-helper\` 的授权提示。Mac 登录密码只输入 macOS 系统对话框。“始终允许”仅适用于当前密钥与 helper；密钥或 helper 身份变化后仍可能需要重新授权。
3. **新版侧栏恢复帽子入口**：帽子入口现位于已识别新版侧栏的首位；提示框复用 Codex Tooltip 组件、层级与外观。

运行 \`inc update\` 以发布更新后的 Runtime。`,
};

const v159ReleaseCopy = {
  en: `### Changelog

1. **Launcher follows the selected Dock icon**: The Finder launcher mirrors the ChatGPT or Codex icon style selected by the host. If that icon resource is unavailable, it falls back to the host's file icon.
2. **Shared Applications launcher**: The launcher now publishes to \`/Applications/ChatGPT - Inc.app\`. After publishing the shared entry, it migrates exact Incodex-owned entries from \`~/Applications\` and preserves unknown or other-user entries.
3. **Updated bilingual manual**: The manual explains the launcher icon and migration path, and refreshes the window guide diagrams.

Upgrade with \`inc update\`, then run \`incodex launch\` to refresh the Finder entry. No host re-sign, reinstall, or restart is required.`,
  zh: `### 更新日志

1. **启动器跟随所选 Dock 图标**：Finder 启动器镜像 ChatGPT 或 Codex 宿主所选的图标样式。所选图标资源不可用时，回退到宿主的文件图标。
2. **共享 Applications 启动器**：启动器现发布到 \`/Applications/ChatGPT - Inc.app\`。发布共享入口后，迁移 \`~/Applications\` 下精确归属 Incodex 的入口，并保留未知或其他用户的入口。
3. **更新双语手册**：手册说明启动器图标与迁移路径，并更新窗口指南示意图。

运行 \`inc update\` 升级，再运行 \`incodex launch\` 刷新 Finder 入口。无需重签、重装或重启宿主。`,
};

const v1511ReleaseCopy = {
  en: `### Changelog

1. **Restore Windows private-window controls after refresh**: Default private windows now restore the hat-glasses control and privacy banner after a page reload, while retaining normal close and session cleanup.

Upgrade with \`inc update\` to install the fixed CLI and publish its Runtime; previously opened private windows need to be closed and opened again to use the fix.

Store update recovery for the first restarted window remains experimental. Already-running windows without integration cannot be repaired in place. An older, mismatched Windows update history may still require a normal App exit followed by \`incodex uninstall\` and \`incodex install\`; that migration is separate from an automatic update.`,
  zh: `### 更新日志

1. **Windows 私密窗口刷新后恢复帽子**：默认私密窗口刷新页面后会重新显示眼镜帽子和隐私横幅，正常关闭及会话清理保持原有行为。

运行 \`inc update\` 安装修复后的 CLI 并发布配套 Runtime；已经打开的私密窗口需要关闭后重新打开，才能使用本次修复。

Store 更新后首个重启窗口的自动恢复仍属实验；已经运行但未集成的窗口不能就地修复。旧版遗留且身份不匹配的 Windows 更新记录仍可能需要正常退出 App，再依次运行 \`incodex uninstall\` 和 \`incodex install\`，这类迁移不算自动升级通过。`,
};

for (const locale of ['en', 'zh']) {
  test(`${locale} v1.5.11 release copy matches the published notes and stable baseline`, async () => {
    const page = await readManual(`src/content/docs/${locale}/releases/v1.5.11.mdx`);
    const index = await readManual(`src/content/docs/${locale}/releases/index.mdx`);
    const home = await readManual(`src/content/docs/${locale}/index.mdx`);
    const languageIndex = await readManual('src/content/docs/index.mdx');
    const releases = JSON.parse(await readManual('releases.json'));
    const latest = releases[0];
    const homeDescription = locale === 'en'
      ? /description: "A private window for Codex · v1\.5\.11"/
      : /description: "Codex 的私密窗口 · v1\.5\.11"/;

    assert.deepEqual(latest, {
      version: '1.5.11',
      tag: 'v1.5.11',
      commit: 'c95abd777a9d1a6ed9ac633ca82bc63a80f9ee9f',
      publishedAt: '2026-10-26T18:13:15Z',
    });
    assert.doesNotMatch(page, /^draft:\s*true$/m);
    assert.match(page, locale === 'en'
      ? /Published \*\*2026-10-26\*\* · \[Original GitHub release notes and downloads\]\(https:\/\/github\.com\/daftAI2026\/incodex\/releases\/tag\/v1\.5\.11\)/
      : /发布于 \*\*2026-10-26\*\* · \[GitHub 原始发布说明与下载\]\(https:\/\/github\.com\/daftAI2026\/incodex\/releases\/tag\/v1\.5\.11\)/);
    assert.equal(page.slice(page.indexOf(locale === 'en' ? '### Changelog' : '### 更新日志')).trim(), v1511ReleaseCopy[locale]);
    assert.match(index, locale === 'en'
      ? /^- \[v1\.5\.11 · 2026-10-26\]\(\/en\/releases\/v1\.5\.11\/\)/m
      : /^- \[v1\.5\.11 · 2026-10-26\]\(\/zh\/releases\/v1\.5\.11\/\)/m);
    assert.match(index, /stable baseline.*v1\.5\.11|稳定版是 \*\*v1\.5\.11\*\*/i);
    assert.match(home, /\*\*Incodex 1\.5\.11\*\*/);
    assert.match(home, homeDescription);
    assert.match(languageIndex, /Incodex 1\.5\.11/);
    assert.equal(releases[1].version, '1.5.10');

    const built = await readFile(new URL(`dist/${locale}/releases/v1.5.11/index.html`, manualRoot), 'utf8');
    assert.match(textContent(parse(built)), /v1\.5\.11/);
  });
}

const frozenReleaseHistory = [
  {
    record: { version: '1.5.10', tag: 'v1.5.10', commit: '407b78a49985152d70494e8efe33bf1c03ae9f2c', publishedAt: '2026-10-26T12:13:45Z' },
    copy: v1510ReleaseCopy, nextVersion: '1.5.9',
  },
  {
    record: { version: '1.5.9', tag: 'v1.5.9', commit: '9d7952e6607d9570efc26e1a35dd68a10823ead9', publishedAt: '2026-10-23T03:25:26Z' },
    copy: v159ReleaseCopy, nextVersion: '1.5.8',
  },
];
for (const { record, copy, nextVersion } of frozenReleaseHistory) {
  for (const locale of ['en', 'zh']) {
    test(`${locale} ${record.tag} historical release stays intact below the stable baseline`, async () => {
      const page = await readManual(`src/content/docs/${locale}/releases/${record.tag}.mdx`);
      const index = await readManual(`src/content/docs/${locale}/releases/index.mdx`);
      const releases = JSON.parse(await readManual('releases.json'));
      const position = releases.findIndex(release => release.tag === record.tag);
      assert.ok(position > 0, 'Historical release remains below the current stable baseline');
      assert.deepEqual(releases[position], record);
      assert.doesNotMatch(page, /^draft:\s*true$/m);
      assert.ok(page.includes(`**${record.publishedAt.slice(0, 10)}**`));
      assert.ok(page.includes(`https://github.com/daftAI2026/incodex/releases/tag/${record.tag}`));
      assert.equal(page.slice(page.indexOf(locale === 'en' ? '### Changelog' : '### 更新日志')).trim(), copy[locale]);
      const link = `/${locale}/releases/${record.tag}/`;
      const stableLink = `/${locale}/releases/${releases[0].tag}/`;
      assert.ok(index.includes(link));
      assert.ok(index.includes(stableLink));
      assert.ok(index.indexOf(stableLink) < index.indexOf(link));
      assert.equal(releases[position + 1].version, nextVersion);
      const built = await readFile(new URL(`dist/${locale}/releases/${record.tag}/index.html`, manualRoot), 'utf8');
      assert.ok(textContent(parse(built)).includes(record.tag));
    });
  }
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
    const [{ version: stableVersion }] = JSON.parse(await readManual('releases.json'));
    assert.match(releaseLinks[0] ?? '', new RegExp(`/releases/v${stableVersion.replaceAll('.', '\\.')}\\/$`),
      `${locale}: current stable release is first`);
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
