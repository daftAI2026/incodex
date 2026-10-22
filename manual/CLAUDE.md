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
releases.json: JSON，公共稳定版本基线，须与站点双语发布页对齐
tsconfig.json: TypeScript，站点编译检查边界
public/: 站点静态图片与图标；images/ 含权限截图、窗口图和 v1.5.8 图标预览，不含账号或聊天资料
scripts/: 站点覆盖/版本/链接检查，发布前验证公共文档一致性
src/: Astro 页面、组件与双语 MDX 内容，独立于 Electron Runtime

法则: 成员完整·一行一文件·父级链接·技术词前置
[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
