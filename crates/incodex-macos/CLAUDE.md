# crates/incodex-macos/
> L2 | 父级: ../../CLAUDE.md

成员清单
tests/local_signing.rs: 稳定证书/组件 DR 的失败先行合同；缺失与非法输入失败，不允许降级为 ad-hoc。
Cargo.toml: Cargo，定义模块依赖与平台编译边界
src/accessibility.rs: Rust，精确宿主只读 AX 查询；多个同路径进程保持未知
src/app_termination.rs: Rust，按确切执行文件身份退出 App
src/asar_integrity_digest.rs: Rust，ASAR 读取、校验与变更
src/entitlements.rs: Rust，ad-hoc 权限规划
src/framework.rs: Rust，macOS 动态框架句柄、符号与全局数据加载
src/lib.rs: CLI 与事务层共用的 macOS 能力门面，平台细节不进入产品命令层
src/live_window.rs: Rust，窗口归属与显示边界
src/live_window_macos.rs: Rust，CoreGraphics 窗口候选坐标与严格身份快照
src/macho.rs: Rust，Mach-O 普通加载命令
src/open_window.rs: Rust，持有型窗口观察器；新增固定窗口成功空查询证据值
src/session_process.rs: Rust，会话生命周期与清理
src/signature_inspection.rs: Rust，codesign 输出解析与身份证据
src/signing.rs: macOS 签名验收与 vendor 保留；ad-hoc/local 共用受限 entitlement 剔除策略，变更委派 signing_mutation
src/signing_context.rs: 注册证书的逐组件 DR 与显式 local 验收，不创建身份或放宽 generic 信任
src/signing_mutation.rs: 唯一 codesign 变更流程；排他创建私有临时目录，保留 vendor/CUA 与组件 DR
src/window_server.rs: Rust，按 CDP 主窗选唯一候选；测试约束空查询与失败分离
src/window_server_macos.rs: Rust，SkyLight 私有 ABI；只读 PID、窗口 ID、标签与 order 状态
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
