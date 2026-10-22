# manual/scripts/
> L2 | 父级: ../CLAUDE.md

成员清单
check-audit.mjs: 验证 npm 审计完整且无漏洞
check-audit.test.mjs: 覆盖审计报告校验与漏洞拒绝
check-coverage.mjs: 对照 Rust 命令和双语指南覆盖率
check-links.mjs: 检查构建 HTML 的链接、资源与片段
check-links.test.mjs: 测试链接解析、侧栏导航和地图不外露
check-release.mjs: 校验稳定版本清单、顺序和发布时间
html-links.mjs: 用 parse5 提取真实 HTML 链接与 ID
latest-release.test.mjs: 覆盖稳定版徽标筛选和更新

法则: 构建检查读取生成产物；测试不得把源码文本当页面输出。
[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
