# manual/src/
> L2 | 父级: ../CLAUDE.md

成员清单
components/: Astro 站点组件
components.ts: Astro 组件覆写表；站点与产品 UI 分离
content/: 中英指南与内容片段
content.config.ts: Astro 内容集合配置；载入中英指南并排除架构地图
layouts/: Astro 页面外壳
lib/: 站点构建与路由工具
pages/404.astro: 输出静态未找到页，复用 BaseLayout 与站点导航
pages/[...slug].astro: 输出指南页，将所选语言的导航条目作为侧栏根节点
pages/llms-full.txt.ts: 输出 Nimbus 全量 Markdown 索引
pages/llms.txt.ts: 输出 Nimbus 精简 Markdown 索引
pages/robots.txt.ts: 输出 robots 规则与站点地图地址
styles/: 站点样式

pages/ 将 .md 文件视为公开路由。导航地图折叠在本文件，不在该目录建图。
法则: 实验显式标记·双语同步·不承诺未验行为
[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
