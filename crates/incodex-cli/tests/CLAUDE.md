# crates/incodex-cli/tests/
> L2 | 父级: ../CLAUDE.md

成员清单
launch.rs: Rust，真实CLI帮助、预览与共享Applications目标合同；路径预览不读写入口，不启动真实宿主。
fixtures/install-permission-runtime.cjs: cjs，真实Loader与探针进程整链；文件守卫只由probe的lstat ENOENT记录移除。
accessibility_diagnosis.rs: Rust，辅助功能权限证据与引导；回归边界
diagnose_process_identity.rs: Rust，诊断证据；回归边界
doctor/: 测试支撑模块；依赖边界见本目录成员地图。
doctor_depth.rs: Rust，诊断输出；回归边界
doctor_symlink_truth.rs: Rust，诊断输出；回归边界
doctor_truth.rs: Rust，诊断输出；回归边界
fixtures/: 测试支撑模块；依赖边界见本目录成员地图。
install/: 测试支撑模块；依赖边界见本目录成员地图。
install.rs: Rust，真实 CLI 与隔离 HOME 验收安装变更、回滚和不创建状态的签名模式预览
install_guards.rs: Rust，安装变更与回滚；回归边界
legacy_proof.rs: Rust，历史磁盘状态兼容；回归边界
legacy_typescript.rs: Rust，历史磁盘状态兼容；回归边界
legacy_uninstall.rs: Rust，合成签名包的旧 journal 卸载迁移与损坏备份拒绝
macos_keychain_authorize.rs: Rust，Keychain 连续性；回归边界
macos_keychain_native.rs: Rust，生产 Storage helper/provider 与 install_mutation 的事务/回滚回归
macos_keychain_protocol.rs: Rust，Keychain 连续性；回归边界
macos_keychain_provider.rs: Rust，Keychain 连续性；回归边界
macos_keychain_registration.rs: Rust，合成注册回归，证明获权 Helper 冻结与未获权显式换代，不触碰真实 Keychain
macos_keychain_shadow.rs: Rust，Storage readiness 与 install_mutation 前台授权门合同
macos_signing_assets.rs: Rust，合成签名身份回归，证明稳定注册、content-addressed verifier 引用、只读读取与严格 Keychain lock 白名单，不触碰宿主/TCC
macos_signing_context.rs: Rust，显式本地证书实签验收；两例多组件DR/载荷连续性与拒绝回归，默认忽略
macos_signing_policy.rs: 官方安装/后台恢复的上下文选择红测；后台缺失身份不创建不降级
macos_signing_registration.rs: Rust，默认schema/CAS与legacy不迁移回归；证书代际提升和错误签名拒绝两例归显式本地验收
macos_sparkle_diagnostics.rs: macOS，执行隔离 interposer smoke；验证原回调与诊断，不启动真实更新。
macos_update_continuation.rs: macOS，执行真实Coordinator与登记哨兵；排队普通退出后自动续接，不代开二阶段。
macos_update_relaunch_race.rs: macOS，编译真实Coordinator并以真实AppQuiescence复现活宿主与回调前早退两种竞态、重试CAS与不重开意图。
macos_update_diagnostics.rs: Rust，编译并执行隔离 Coordinator；验证 opt-in 和普通退出，不启动官方 App。
macos_update_registration.rs: Rust，inc 链接源的登记、真实 runtime 与恢复 CAS；保留不可变 helper 和控制文件拒绝链接。
macos_update_restore.rs: Rust，退出转发、pending锁与共享install入口的恢复busy码传播合同。
native_contract.rs: Rust，公开命令及 Runtime 条件权限预览的无副作用合同；平台原生适配；回归边界
open.rs: Rust，隔离启动；回归边界
open_permissions.rs: Rust，只读CLI的Mach-O导入表；拒绝重新引入CLI无障碍提示。
permission_update.rs: Rust，内部更新任务拒绝伪造与只读预览；不运行宿主
probe.rs: Rust，CLI 10/5 MiB、外置 JS/JSON 1 MiB 与本机 50ms 门；预算边界独立回归
readonly.rs: Rust，只读命令不得创建产品状态；回归边界
recovery_cleanup.rs: Rust，恢复后的事务清理；回归边界
release_asset_smoke.rs: Rust，发布边界；回归边界
runtime_drift.rs: Rust，外部 Runtime 构建与验证；回归边界
session_privacy.rs: Rust，源码渠道拒绝自卸载与自建文件哨兵；依赖可识别的源码构建布局
signing_doctor.rs: Rust，签名身份与 entitlement；回归边界
signing_doctor_custom.rs: Rust，签名身份与 entitlement；回归边界
signing_doctor_identity.rs: Rust，签名身份与 entitlement；回归边界
support/: 测试支撑模块；依赖边界见本目录成员地图。
tty_harness.rs: Rust，原生菜单 TTY 行为；回归边界
uninstall_safety.rs: Rust，安装变更与回滚；回归边界
update.rs: Rust，脚本更新、TTY 通知和并发代际锁回归
update_homebrew.rs: Rust，替身 brew 路由与代际回归；严格保留已确认发出的诊断，超时前无输出单独验收
update_runtime.rs: Rust，更新后的 Runtime 交接、待修复标记和部分成功回归
update_support/: 测试支撑模块；依赖边界见本目录成员地图。
version_macos.rs: macOS 版本事实回归，假 PATH 记录子命令，空 PATH 检查降级边界
windows_activation.rs: Rust，窗口归属与显示；回归边界
windows_activation_capability.rs: Rust，窗口归属与显示；回归边界
windows_app.rs: Rust，窗口归属与显示；回归边界
windows_cdp.rs: Rust，窗口归属与显示；回归边界
windows_execution_manifest.rs: Rust，CLI/helper 的执行权限 manifest 回归，拒绝请求提权
windows_helper.rs: Rust，窗口归属与显示；回归边界
windows_install_confirmation.rs: Rust，窗口归属与显示；回归边界
windows_install_state.rs: Rust，窗口归属与显示；回归边界
windows_installer.rs: Rust，窗口归属与显示；回归边界
windows_launch.rs: Rust，窗口归属与显示；回归边界
windows_open.rs: Rust，窗口归属与显示；回归边界
windows_platform.rs: Rust，窗口归属与显示；回归边界
windows_process.rs: Rust，窗口归属与显示；回归边界
windows_process_parameters.rs: Rust，窗口归属与显示；回归边界
windows_quiescence.rs: Rust，进程静默证明；回归边界
windows_runtime.rs: Rust，窗口归属与显示；回归边界
windows_runtime_open.rs: Rust，窗口归属与显示；回归边界
windows_runtime_raise.rs: Rust，窗口归属与显示；回归边界
windows_self_uninstall.rs: Rust，自卸载 owner/延迟进程与状态保留；等待 finally 释放稳定锁后清理夹具。
windows_update.rs: Rust，更新代际与恢复；回归边界
windows_update_repair.rs: Rust，Store 更新 epoch、目标预备与恢复失败的安全合同，不宣称实机已通过

native/: 原生续接夹具与Rust测试支撑；成员和生命周期边界见本目录地图。
测试只使用自有临时输入与进程。
真实宿主验收不由合成进程替代。
[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
