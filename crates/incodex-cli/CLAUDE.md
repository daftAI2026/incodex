# crates/incodex-cli/
> L2 | 父级: ../../CLAUDE.md

成员清单
Cargo.toml: Cargo，定义模块依赖与平台编译边界
assets/incodex-windows-bootstrap.cjs: cjs，窗口归属与显示
assets/incodex-windows-platform.cjs: cjs，窗口归属与显示
build.rs: Cargo build script，生成本 crate 消费的原生资产
native/fishhook.c: C，Mach-O 符号重绑定
native/fishhook.h: C header，Mach-O 符号重绑定
native/macos_keychain_helper.m: Objective-C，Keychain 连续性
native/macos_keychain_provider.c: C，Keychain 连续性
native/macos_sparkle_interpose.m: Objective-C，Sparkle 退出交接的进程内拦截器
native/macos_update_coordinator.m: Objective-C，更新代际与恢复
src/accessibility_guide_host.rs: Rust，辅助功能权限证据与引导
src/accessibility_restore.rs: Rust，辅助功能权限证据与引导
src/accessibility_setup.rs: Rust，辅助功能权限证据与引导
src/accessibility_setup_tests.rs: Rust，辅助功能权限证据与引导
src/accessibility_target.rs: Rust，辅助功能权限证据与引导
src/app_bundle.rs: Rust，App bundle 路径与执行文件解析
src/app_quiescence.rs: Rust，进程静默证明
src/cdp.rs: Rust，loopback CDP 观测与注入
src/cdp_lifecycle_tests.rs: Rust，loopback CDP 观测与注入
src/cdp_masked_lifecycle.rs: Rust，loopback CDP 观测与注入
src/cdp_mode.rs: Rust，loopback CDP 观测与注入
src/cdp_mode_tests.rs: Rust，loopback CDP 观测与注入
src/cdp_partial_flush_tests.rs: Rust，loopback CDP 观测与注入
src/cdp_ui_probe_tests.rs: Rust，loopback CDP 观测与注入
src/cdp_unit_tests.rs: Rust，loopback CDP 观测与注入
src/confirm.rs: Rust，TTY 单次确认与非交互 --yes 合同
src/diagnose.rs: Rust，诊断证据
src/diagnose_checks.rs: Rust，诊断证据
src/diagnose_checks_tests.rs: Rust，诊断证据
src/diagnose_format.rs: Rust，诊断证据
src/diagnose_fs.rs: Rust，诊断证据
src/diagnose_runtime.rs: Rust，外部 Runtime 构建与验证
src/diagnose_sessions.rs: Rust，会话生命周期与清理
src/diagnose_signing.rs: Rust，签名身份与 entitlement
src/diagnosis_presentation.rs: Rust，诊断展示文案与层级
src/friendly_name.rs: Rust，用户可读的 App/路径名称
src/help.rs: Rust，公开命令帮助与参数说明
src/install.rs: 原生 CLI 的危险变更编排器；仅在 quiescence 与代际证明成立时交给底层事务
src/install_keychain_advice.rs: Rust，Keychain 连续性
src/install_tests.rs: Rust，安装变更与回滚
src/legacy_proof.rs: Rust，历史磁盘状态兼容
src/legacy_typescript.rs: Rust，历史磁盘状态兼容
src/lib.rs: Rust，native CLI 的模块接线、命令分派与错误边界
src/lifecycle.rs: Rust，Runtime 发布及 CLI 生命周期命令
src/locale.rs: Rust，语言策略
src/macos_keychain_assets.rs: Rust，Storage 固定获权 Helper 注册与显式授权，普通更新不轮换获权身份
src/macos_keychain_protocol.rs: Rust，Keychain 连续性
src/macos_update_assets.rs: Rust，更新代际与恢复
src/macos_update_log.rs: Rust，更新代际与恢复
src/macos_update_restore.rs: Rust，更新代际与恢复
src/main.rs: Rust，产品二进制入口与退出码
src/menu.rs: Rust，原生菜单交互
src/menu_controller.rs: Rust，原生菜单交互
src/menu_view.rs: Rust，原生菜单交互
src/open.rs: Rust，隔离启动
src/open_cleanup_tests.rs: Rust，隔离启动
src/open_command.rs: Rust，隔离启动
src/open_native_close.rs: Rust，隔离启动
src/open_presentation.rs: Rust，隔离启动
src/open_tests.rs: Rust，隔离启动
src/parse.rs: Rust，命令语言解析
src/profile_mask.rs: Rust，原生 profile-health 观测
src/spinner.rs: Rust，有界进度呈现与清理
src/stable_release.rs: Rust，发布边界
src/terminal.rs: Rust，TTY 终端状态控制
src/terminal_presentation.rs: Rust，终端输出呈现
src/update_flow.rs: Rust，更新代际与恢复
src/version.rs: Rust，CLI 版本报告
src/windows_activation.rs: Rust，窗口归属与显示
src/windows_activation_capability.rs: Rust，窗口归属与显示
src/windows_app.rs: Rust，窗口归属与显示
src/windows_cleanup.rs: Rust，窗口归属与显示
src/windows_cleanup_tests.rs: Rust，窗口归属与显示
src/windows_console.rs: Rust，窗口归属与显示
src/windows_doctor.rs: Rust，窗口归属与显示
src/windows_file.rs: Rust，窗口归属与显示
src/windows_helper.rs: Rust，窗口归属与显示
src/windows_install.rs: Rust，窗口归属与显示
src/windows_install_state.rs: Rust，窗口归属与显示
src/windows_installed_cdp.rs: Rust，窗口归属与显示
src/windows_launch.rs: Rust，窗口归属与显示
src/windows_locale.rs: Rust，窗口归属与显示
src/windows_locale_tests.rs: Rust，窗口归属与显示
src/windows_menu.rs: Rust，窗口归属与显示
src/windows_open.rs: Rust，窗口归属与显示
src/windows_open_tests.rs: Rust，窗口归属与显示
src/windows_process.rs: Rust，窗口归属与显示
src/windows_process_parameters.rs: Rust，窗口归属与显示
src/windows_profile.rs: Rust，窗口归属与显示
src/windows_registration.rs: Rust，窗口归属与显示
src/windows_runtime.rs: Rust，窗口归属与显示
src/windows_runtime_lifecycle.rs: Rust，窗口归属与显示
src/windows_runtime_open.rs: Rust，窗口归属与显示
src/windows_runtime_raise.rs: Rust，窗口归属与显示
src/windows_self_uninstall.rs: Rust，窗口归属与显示
src/windows_status.rs: Rust，窗口归属与显示
src/windows_system.rs: Rust，窗口归属与显示
src/windows_update.rs: Rust，更新代际与恢复
tests/accessibility_diagnosis.rs: Rust，辅助功能权限证据与引导；回归边界
tests/diagnose_process_identity.rs: Rust，诊断证据；回归边界
tests/doctor/transaction_evidence.rs: Rust，事务证据；回归边界
tests/doctor_depth.rs: Rust，诊断输出；回归边界
tests/doctor_symlink_truth.rs: Rust，诊断输出；回归边界
tests/doctor_truth.rs: Rust，诊断输出；回归边界
tests/fixtures/incodex-loader-v0.3.1.cjs: cjs，ASAR 到外部 Runtime 的加载；回归边界
tests/install.rs: Rust，安装变更与回滚；回归边界
tests/install/recovery.rs: Rust，安装事务恢复；回归边界
tests/install/transaction_evidence.rs: Rust，事务证据；回归边界
tests/install_guards.rs: Rust，安装变更与回滚；回归边界
tests/legacy_proof.rs: Rust，历史磁盘状态兼容；回归边界
tests/legacy_typescript.rs: Rust，历史磁盘状态兼容；回归边界
tests/legacy_uninstall.rs: Rust，历史磁盘状态兼容；回归边界
tests/macos_keychain_authorize.rs: Rust，Keychain 连续性；回归边界
tests/macos_keychain_native.rs: Rust，Keychain 连续性；回归边界
tests/macos_keychain_protocol.rs: Rust，Keychain 连续性；回归边界
tests/macos_keychain_provider.rs: Rust，Keychain 连续性；回归边界
tests/macos_keychain_registration.rs: Rust，合成注册回归，证明获权 Helper 冻结与未获权显式换代，不触碰真实 Keychain
tests/macos_keychain_shadow.rs: Rust，Keychain 连续性；回归边界
tests/macos_signing_policy.rs: 官方安装/后台恢复的上下文选择红测；后台缺失身份不创建不降级
tests/macos_signing_assets.rs: Rust，合成签名身份回归，证明稳定注册、只读读取与损坏拒绝，不触碰宿主/TCC
tests/macos_signing_registration.rs: 更新 epoch 绑定 local 指纹，旧 None 不因 root 证书出现而悄悄迁移
tests/macos_signing_context.rs: Rust，生产私有身份驱动的 synthetic 多组件 DR/载荷连续性与拒绝回归
tests/macos_update_registration.rs: Rust，更新代际与恢复；回归边界
tests/macos_update_restore.rs: Rust，更新代际与恢复；回归边界
tests/native_contract.rs: Rust，平台原生适配；回归边界
tests/open.rs: Rust，隔离启动；回归边界
tests/probe.rs: Rust，原生诊断探针；回归边界
tests/readonly.rs: Rust，只读命令不得创建产品状态；回归边界
tests/recovery_cleanup.rs: Rust，恢复后的事务清理；回归边界
tests/release_asset_smoke.rs: Rust，发布边界；回归边界
tests/runtime_drift.rs: Rust，外部 Runtime 构建与验证；回归边界
tests/session_privacy.rs: Rust，会话生命周期与清理；回归边界
tests/signing_doctor.rs: Rust，签名身份与 entitlement；回归边界
tests/signing_doctor_custom.rs: Rust，签名身份与 entitlement；回归边界
tests/signing_doctor_identity.rs: Rust，签名身份与 entitlement；回归边界
tests/support/committed_install.rs: Rust，安装变更与回滚；回归边界
tests/support/legacy_typescript_matrix.rs: Rust，历史磁盘状态兼容；回归边界
tests/support/mod.rs: Rust，集成测试公共夹具入口；回归边界
tests/support/native_tty.rs: Rust，平台原生适配；回归边界
tests/support/readonly.rs: Rust，只读命令不得创建产品状态；回归边界
tests/support/runtime.rs: Rust，外部 Runtime 构建与验证；回归边界
tests/support/tty.rs: Rust，真实 PTY 测试夹具；回归边界
tests/support/update_menu.rs: Rust，更新代际与恢复；回归边界
tests/tty_harness.rs: Rust，原生菜单 TTY 行为；回归边界
tests/uninstall_safety.rs: Rust，安装变更与回滚；回归边界
tests/update.rs: Rust，更新代际与恢复；回归边界
tests/windows_activation.rs: Rust，窗口归属与显示；回归边界
tests/windows_activation_capability.rs: Rust，窗口归属与显示；回归边界
tests/windows_app.rs: Rust，窗口归属与显示；回归边界
tests/windows_cdp.rs: Rust，窗口归属与显示；回归边界
tests/windows_helper.rs: Rust，窗口归属与显示；回归边界
tests/windows_install_confirmation.rs: Rust，窗口归属与显示；回归边界
tests/windows_install_state.rs: Rust，窗口归属与显示；回归边界
tests/windows_installer.rs: Rust，窗口归属与显示；回归边界
tests/windows_launch.rs: Rust，窗口归属与显示；回归边界
tests/windows_open.rs: Rust，窗口归属与显示；回归边界
tests/windows_platform.rs: Rust，窗口归属与显示；回归边界
tests/windows_process.rs: Rust，窗口归属与显示；回归边界
tests/windows_process_parameters.rs: Rust，窗口归属与显示；回归边界
tests/windows_quiescence.rs: Rust，进程静默证明；回归边界
tests/windows_runtime.rs: Rust，窗口归属与显示；回归边界
tests/windows_runtime_open.rs: Rust，窗口归属与显示；回归边界
tests/windows_runtime_raise.rs: Rust，窗口归属与显示；回归边界
tests/windows_self_uninstall.rs: Rust，窗口归属与显示；回归边界
tests/windows_update.rs: Rust，更新代际与恢复；回归边界

法则: 成员完整·一行一文件·父级链接·技术词前置
[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
