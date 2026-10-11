# scripts/
> L2 | 父级: ../CLAUDE.md

构建、CI 选路与显式验收工具。
实机脚本不是产品默认路径，不从测试授权推出操作授权。

成员清单
build-remote-key-native.ts: N-API 双架构构建；登记只读身份依赖与产物摘要，不触碰宿主。
build-permission-native.ts: 原生权限资产构建，不在产品安装时编译；按 views、initial 固定顺序绑定视图源码；host 绑定 geometry/views/presenter 有序源码；构建 host/dylib 与摘要 manifest。
build-windows-release.ts: Windows 构建门，不改变 macOS 资产；构建并验证 x64 Windows CLI 发布资产。
check-dist.ts: 产物一致性门；执行期间不能并发重建；按来源 commit 重建并验证 dist 与提交一致。
ci-change-scope.ts: CI 选路入口；仅 manual 单域可跳产品检查；输出是否需要产品检查；未知证据保留产品门。
flight-video-measure.swift: 离线测量工具，不操作应用窗口；输出锚点轨迹、置信度和帧间隔报告。
flight-video-measure.test.ts: 离线分析合同，不触发真实权限页；验证锚点几何、VFR 与输出边界。
incodex-e11-e12-installed-acceptance.ts: 显式实机验收编排，不属于产品启动链；验证已安装开窗、尺寸投影和关闭后置条件。
incodex-e11-e12-installed-ax.swift: 精确 PID 的原生验收适配，不是权限代理；为验收脚本提供目标窗口观察与动作。
incodex-tooltip-cold-acceptance.ts: 显式冷启动验收，不为日常宿主增加 debug 端口；测量隔离窗口首次 Tooltip 就绪与命中状态。
permission-swift-candidate-visual-helper.swift: 可见验收工具，由配套 runner 限定操作范围；提供显式目标的权限页观察、动作与截图。
permission-swift-candidate-visual.test.ts: 安全回归；默认拒绝真实 UI 操作；验证显式可见 UI 授权、路径和诊断开关门。
permission-swift-candidate-visual.ts: 独立验收入口，不替代默认 install 授权路径；编排显式许可的可见候选验收并保存证据。
prepare-release.ts: 发布准备工具，不创建 tag 或 Release；同步稳定版本声明和公开输出样例。
setup-quick-launchers.sh: 显式集成工具；拒绝重定向路径；配置或移除自有快速启动入口。
test-platform.ts: 本地检查选路；不代替另一平台实机验收；串行运行 shared 和当前宿主测试门。
test-windows-rust.ts: Windows 检查选路；不扩展 unsupported workspace；提供并运行支持边界内的 Rust 命令。
update-homebrew-tap-formula.sh: 发布工作流工具；不创建公开版本；更新并验证自有 Homebrew 公式。

[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
