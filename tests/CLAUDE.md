# tests/
> L2 | 父级: ../CLAUDE.md

成员清单
native/: Swift/Objective-C 原生回归；可见探针要求独立授权
ci-path-scope.test.ts: TypeScript，CI 手册独立选路与未知/混合改动保留产品门
README.md: Markdown，说明本模块的验证和使用边界
agent-skills.test.ts: TypeScript，技能树和 shared guide 链接合同；回归边界
architecture.test.ts: TypeScript，架构依赖与原生 CLI 边界回归；指导文档不承载候选流水
dependabot.test.ts: TypeScript，依赖更新配置；回归边界
github-actions-pin.test.ts: TypeScript，GitHub Actions 固定提交 SHA；回归边界
homebrew-tap.test.ts: TypeScript，自有 Homebrew tap 发布边界；回归边界
install-sh.test.ts: TypeScript，安装变更与回滚；回归边界
legacy-retirement.test.ts: TypeScript，历史磁盘状态兼容；回归边界
native-cli-contract.test.ts: TypeScript，平台原生适配；回归边界
native-runtime-deploy.test.ts: TypeScript，外部 Runtime 构建与验证；回归边界
package-retirement.test.ts: TypeScript，退役 TypeScript CLI 不得复活；回归边界
permission-allow-state-c14.test.ts: TypeScript，原生权限呈现；回归边界
permission-ax-keyboard-diagnostic.test.ts: TypeScript，原生权限呈现；回归边界
permission-card-text-layout.test.ts: TypeScript，原生权限呈现；回归边界
permission-dual-rows.test.ts: SwiftUI 双行回归；验证 26/27 呈现、独立动作和自然测高
permission-row-component.test.ts: SwiftUI，无窗验证权限行状态和回调独立；不新增生产权限项
permission-cli-host-error.test.ts: TypeScript，原生权限呈现；回归边界
permission-host-arrow-timers.test.ts: TypeScript，原生权限呈现；回归边界
permission-host-flight.test.ts: TypeScript，原生权限呈现；回归边界
permission-host-g12-lifecycle.test.ts: TypeScript，原生权限呈现；回归边界
permission-host-handoff-g10.test.ts: TypeScript，原生权限呈现；回归边界
permission-host-migration.test.ts: TypeScript，原生权限呈现；回归边界
permission-host-osa.test.ts: TypeScript，原生权限呈现；回归边界
permission-host-presenter-g13.test.ts: TypeScript，原生权限呈现；回归边界
permission-host-presenter.test.ts: TypeScript，原生权限呈现；回归边界
permission-icon-quality.test.ts: TypeScript，原生权限呈现；回归边界
permission-instruction-copy.test.ts: TypeScript，冻结 AX 拖拽句子与系统名称；总说明覆盖双权限
permission-native-activation-policy-ax.test.ts: TypeScript，原生权限呈现；回归边界
permission-native-artifact.test.ts: TypeScript，原生权限呈现；回归边界
permission-native-cutover.test.ts: TypeScript，原生权限呈现；回归边界
permission-native-host.test.ts: TypeScript，原生权限呈现；回归边界
permission-osa-reuse.test.ts: TypeScript，原生权限呈现；回归边界
permission-runtime-error.test.ts: TypeScript，外部 Runtime 构建与验证；回归边界
permission-shared-copy.test.ts: TypeScript，65 语言双权限文案、显式语言与系统页名模板回归
permission-swift-candidate-alignment.test.ts: TypeScript，验证系统适配先于视图字段投影；保留共用组件与动画合同
permission-swift-source.ts: 读取 Presenter 支持源并截取声明；源码合同不绑定文件拆分或访问修饰符
permission-swiftui-native.test.ts: TypeScript，原生权限呈现；回归边界
quick-launchers.test.ts: TypeScript，公共 CLI 快捷启动器；回归边界
release-cli.test.ts: 实验恢复资产与 v1.1.0 原生权限资产的发布边界回归
release-version.test.ts: TypeScript，发布边界；回归边界
runtime-manifest.test.ts: TypeScript，摘要、启动代际与外置 JS/JSON 的 1 MiB 原始字节门；不接入宿主
rust-workspace.test.ts: TypeScript，Rust workspace 结构与产品入口；回归边界
supported-builds.test.ts: TypeScript，测试夹具；回归边界
test-suite-contract.test.ts: TypeScript，共享与平台测试入口完整性；回归边界
tooltip-cold-acceptance.test.ts: TypeScript，官方 Tooltip 识别与生命周期；回归边界
windows-bootstrap.test.ts: TypeScript，窗口归属与显示；回归边界
windows-ci-integrity.test.ts: TypeScript，窗口归属与显示；回归边界
windows-release-asset.test.ts: TypeScript，窗口归属与显示；回归边界

windows-actions.test.ts: 合成 native-open 回调验证动作代际、暂存提交及旧请求结果

法则: 成员完整·一行一文件·父级链接·技术词前置
[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
