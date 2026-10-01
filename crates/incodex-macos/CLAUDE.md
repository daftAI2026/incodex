# crates/incodex-macos/
> L2 | 父级: ../../CLAUDE.md

成员清单
tests/local_signing.rs: 稳定证书/组件 DR 的失败先行合同；缺失与非法输入失败，不允许降级为 ad-hoc。
Cargo.toml: Cargo，定义模块依赖与平台编译边界
src/accessibility.rs: Rust，辅助功能权限证据与引导
src/app_termination.rs: Rust，按确切执行文件身份退出 App
src/asar_integrity_digest.rs: Rust，ASAR 读取、校验与变更
src/entitlements.rs: Rust，ad-hoc 权限规划
src/lib.rs: CLI 与事务层共用的 macOS 能力门面，平台细节不进入产品命令层
src/live_window.rs: Rust，窗口归属与显示
src/live_window_macos.rs: Rust，窗口归属与显示
src/macho.rs: Rust，Mach-O 普通加载命令
src/open_window.rs: Rust，窗口归属与显示
src/session_process.rs: Rust，会话生命周期与清理
src/signature_inspection.rs: Rust，codesign 输出解析与身份证据
src/signing.rs: macOS 签名唯一边界；合并完整性更新与实验 provider 代际策略，保留外部 vendor/CUA
tests/add_load_dylib.rs: Rust，普通 LC_LOAD_DYLIB 的原子校验与写入；回归边界
tests/asar_integrity_digest.rs: Rust，ASAR 读取、校验与变更；回归边界
tests/asar_integrity_plist.rs: Rust，ASAR 读取、校验与变更；回归边界
tests/asar_integrity_signing.rs: Rust，ASAR 读取、校验与变更；回归边界
tests/keychain_integrity_signing.rs: macOS 实验与指定基准的真实工具链合流证据，不访问用户 App 或 Keychain
tests/quiescence.rs: Rust，进程静默证明；回归边界
tests/session_processes.rs: Rust，会话生命周期与清理；回归边界
tests/signing_components.rs: Rust，签名身份与 entitlement；回归边界
tests/signing_policy.rs: Rust，签名身份与 entitlement；回归边界
tests/signing_regressions.rs: Rust，签名身份与 entitlement；回归边界
tests/support/asar_integrity_fixture.rs: incodex-macos 测试的共用证据生成器，不读取真实 App 或 Keychain

法则: 成员完整·一行一文件·父级链接·技术词前置
[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
