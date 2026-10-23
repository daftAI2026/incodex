# manual/
> L2 | 父级: ../CLAUDE.md

Astro 7.3.5 + Nimbus Docs 0.15.2；独立构建的中英双语公共指南，不参与 CLI 安装或宿主 Runtime。

成员清单
.gitignore: Git，排除站点依赖和生成物，公共源码仍可独立重建
NOTICE: 版权说明，保留 Nimbus 来源与第三方授权
README.md: 站点开发与发布入口，产品稳定行为不得混入实验承诺
astro.config.ts: Astro，站点路由/内容/构建集成
nimbus.json: Nimbus，站点导航与主题配置
package-lock.json: npm，锁定独立站点依赖；Vite 8.3.3 修复编辑器启动漏洞
package.json: npm，站点 v1.2.1 及 Astro/Nimbus 构建检查命令
releases.json: JSON，当前稳定版与UTC发布时间；驱动首页、语言页和版本索引
tsconfig.json: TypeScript，站点编译检查边界
public/: 静态资源目录，构建时原样公开；内部成员清单折叠在本图，避免发布 CLAUDE.md
public/favicon.svg: 站点浏览器图标
public/social.svg: 社交分享预览图
public/images/accessibility-guide.png: 辅助功能单行向导示例
public/images/launcher-finder-entry.png: v1.5.8 用户目录启动器历史 Finder 示例，仅供历史记录
public/images/launcher-applications-entry.png: 共享 Applications 启动器 Finder 实图，仅含 ChatGPT 与 ChatGPT - Inc
public/images/host-hat-control.png: Codex 工具栏帽子入口实图裁剪，不含账户或聊天内容
public/images/launcher-icon-design-v3.png: 启动器铭牌设计对照图
public/images/launcher-icon-native-preview.png: 原生生成的启动器图标预览
public/images/permissions-dual-guide-en.png: 英文双权限卡渲染图，使用 macOS 27+ 名称
public/images/permissions-dual-guide-zh.png: 中文双权限卡渲染图，使用 macOS 27+ 名称
public/images/window-guide-en.svg: 英文实测布局示意：左侧栏入口与输入框上方通知
public/images/window-guide-zh.svg: 中文实测布局示意：左侧栏入口与输入框上方通知
scripts/: 站点覆盖/版本/链接检查，发布前验证公共文档一致性，地图见 scripts/CLAUDE.md
src/: Astro 页面、组件、样式与双语 MDX 内容，独立于 Electron Runtime，地图见 src/CLAUDE.md

法则: 成员完整·一行一文件·父级链接·技术词前置
[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
