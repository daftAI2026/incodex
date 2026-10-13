# crates/incodex-cli/
> L2 | 父级: ../../CLAUDE.md

成员清单
Cargo.toml: Cargo，定义模块依赖与平台编译边界
assets/incodex-windows-bootstrap.cjs: cjs，窗口归属与显示
assets/incodex-windows-platform.cjs: cjs，窗口归属与显示
build.rs: Cargo build script，合流 macOS Keychain/更新/私有签名 store 资产与 Windows 执行 manifest；平台分支互不启动对方工具链
native/fishhook.c: C，Mach-O 符号重绑定
native/fishhook.h: C header，Mach-O 符号重绑定
native/macos_keychain_helper.m: Objective-C，Keychain 连续性
native/macos_keychain_provider.c: C，Keychain 连续性
native/macos_signing_store.m: Objective-C，私有稳定签名身份 Keychain 创建/只读验证/解锁，不改搜索列表或信任
native/macos_sparkle_interpose.m: Objective-C，精确宿主的 Sparkle 重开目标替换，不执行恢复
native/macos_update_coordinator.m: Objective-C，退出事件与交接所有权协调，恢复委托登记 CLI
src/permission_guide_rows_tests.rs: 无窗状态机回归；更新后缺失才引导，双项完成与动作隔离
src/accessibility_guide_host.rs: Rust，有界 stdio 与 CLI 权限操作；引导不发布 Runtime，UI 不执行 reset
src/accessibility_guide_host_tests.rs: Rust，既有 AX、协议与语言选择的隔离回归
src/permission_guide_flow.rs: Rust，共用双行状态机；EOF 保留续接，显式跳过终结；新鲜双项快照才完成
src/permission_update_flow_tests.rs: Rust，无窗更新流程、人机等待与短锁回归；旧进程读数或身份退休不能完成授权
src/permission_update.rs: Rust，已登记 helper 的一次性检查；复验原请求、epoch 与 Runtime，不启动已关闭的宿主
src/permission_guide_update.rs: Rust，只读固定原生卡资产；不倒退 Runtime 或重建进行中的引导
src/permission_guide_system.rs: Rust，AX/录屏同进程快照与系统设置能力；重启读数隔离
src/permission_guide_copy.rs: Rust，安装/官方身份的文案与语言方向选择
src/permission_screen_probe.rs: Rust，绑定 nonce 与进程代际的录屏请求；打开设置前记录原请求续接意图
src/accessibility_restore.rs: Rust，官方恢复后复用双行引导；无主 Runtime 录屏证据则未知
src/accessibility_setup.rs: Rust，单所有者与原请求 CAS；机器交接释放 owner 后发布 pending，用户终态禁止回退
src/accessibility_setup_tests.rs: Rust，原请求续接、短锁争用与终态 CAS 回归；不读真实 TCC
src/accessibility_target.rs: Rust，APFS 支持判断与单次引导的目标连续性守卫
src/app_bundle.rs: Rust，App bundle 路径与执行文件解析
src/app_quiescence.rs: Rust，进程静默证明
src/cdp.rs: Rust，loopback 所有权与有界注入；使用编译期 inject，不随外置发布换代
src/cdp_lifecycle_tests.rs: Rust，loopback CDP 观测与注入
src/cdp_masked_lifecycle.rs: Rust，loopback CDP 观测与注入
src/cdp_mode.rs: Rust，loopback CDP 观测与注入
src/cdp_mode_tests.rs: Rust，loopback CDP 观测与注入
src/cdp_partial_flush_tests.rs: Rust，loopback CDP 观测与注入
src/cdp_ui_probe_tests.rs: Rust，loopback CDP 观测与注入
src/cdp_unit_tests.rs: Rust，CDP 目标与平台预算回归；夹具读取覆盖暂不可读、分片和有界失败
src/confirm.rs: Rust，TTY 单次确认与非交互 --yes 合同
src/diagnose.rs: Rust，浅层/深层诊断聚合，local 签名须经安装事务绑定，AX trust 独立报告
src/diagnose_checks.rs: Rust，诊断证据
src/diagnose_checks_tests.rs: Rust，诊断证据
src/diagnose_format.rs: Rust，诊断证据
src/diagnose_fs.rs: Rust，诊断证据
src/diagnose_runtime.rs: Rust，外部 Runtime 构建与验证
src/diagnose_sessions.rs: Rust，会话生命周期与清理
src/diagnose_signing.rs: Rust，保留旧签名政策并窄接注册 local 证明，不推导 AX 权限
src/diagnosis_presentation.rs: Rust，诊断展示文案与层级
src/friendly_name.rs: Rust，用户可读的 App/路径名称
src/help.rs: Rust，公开命令帮助与参数说明
src/install_proof.rs: ASAR marker/COMMITTED/live/backup 的只读绑定证明，共用明确的 root 签名政策
src/install_mutation.rs: 安装/卸载/官方恢复的危险事务，签名上下文显式传递并复验，旧安装不悄悄迁移
src/macos_signing.rs: 注册身份在安装/恢复/验收间的唯一适配，后台无注册硬失败，复用 coordinator.log
src/install.rs: 原生 CLI 的危险变更编排器；只读预览区分官方稳定签名/既有模式与自定义 ad-hoc，quiescence 和代际证明后才执行事务
src/install_keychain_advice.rs: Rust，Keychain 连续性
src/install_tests.rs: Rust，安装变更与回滚
src/legacy_proof.rs: Rust，历史磁盘状态兼容
src/legacy_typescript.rs: Rust，历史磁盘状态兼容
src/lib.rs: Rust，native CLI 的模块接线、命令分派与错误边界
src/lifecycle.rs: Rust，Runtime/CLI 生命周期编排，发布外置资产后以同 epoch 刷新实验恢复登记而不重签宿主
src/locale.rs: Rust，语言策略
src/macos_keychain_assets.rs: Rust，Storage 固定获权 Helper 注册与显式授权，普通更新不轮换获权身份
src/macos_keychain_protocol.rs: Rust，Keychain 连续性
src/macos_signing_assets.rs: Rust，稳定本机签名身份注册、只读实际 Keychain 验证与显式解锁
src/macos_signing_files.rs: Rust，本机身份私有文件、权限与原子发布合同
src/macos_signing_process.rs: Rust，本机身份外部工具的有界进程执行
src/macos_update_assets.rs: Rust，更新注册、epoch/signing CAS 与 helper 执行证明；local 匹配 root 身份和 app outer proof
src/macos_update_log.rs: Rust，更新代际与恢复
src/macos_update_restore.rs: Rust，后台恢复与权限检查交接；保留 signer/epoch 和原日志
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
src/version.rs: Rust，macOS 以 sysctl/uname/statfs 读取机器事实；仅 SIP 保留子命令，标签与 Windows 不变
src/windows_activation.rs: Rust，窗口归属与显示
src/windows_activation_capability.rs: Rust，窗口归属与显示
src/windows_app.rs: Rust，窗口归属与显示
src/windows_cleanup.rs: Rust，窗口归属与显示
src/windows_cleanup_tests.rs: Rust，窗口归属与显示
src/windows_console.rs: Rust，窗口归属与显示
src/windows_doctor.rs: Rust，窗口归属与显示
src/windows_file.rs: Rust，窗口归属与显示
src/windows_helper.rs: Rust，窗口归属与显示
src/windows_install.rs: Rust，显式安装/卸载事务；精确包静默证明先于预挂钩撤销
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
tests/fixtures/: 历史 Loader 与 responder 字节，只用于兼容回归。
tests/fixtures/incodex-loader-v0.3.1.cjs: cjs，ASAR 到外部 Runtime 的加载；回归边界
tests/fixtures/permission-responder-rc4.cjs: cjs，原始 UUID responder 与真实 Rust 生产器交接；不加载真实 App。
tests/install.rs: Rust，真实 CLI 与隔离 HOME 验收安装变更、回滚和不创建状态的签名模式预览
tests/install/recovery.rs: Rust，安装事务恢复；回归边界
tests/install/transaction_evidence.rs: Rust，事务证据；回归边界
tests/install_guards.rs: Rust，安装变更与回滚；回归边界
tests/legacy_proof.rs: Rust，历史磁盘状态兼容；回归边界
tests/legacy_typescript.rs: Rust，历史磁盘状态兼容；回归边界
tests/legacy_uninstall.rs: Rust，合成签名包的旧 journal 卸载迁移与损坏备份拒绝
tests/macos_keychain_authorize.rs: Rust，Keychain 连续性；回归边界
tests/macos_keychain_native.rs: Rust，生产 Storage helper/provider 与 install_mutation 的事务/回滚回归
tests/macos_keychain_protocol.rs: Rust，Keychain 连续性；回归边界
tests/macos_keychain_provider.rs: Rust，Keychain 连续性；回归边界
tests/macos_keychain_registration.rs: Rust，合成注册回归，证明获权 Helper 冻结与未获权显式换代，不触碰真实 Keychain
tests/macos_keychain_shadow.rs: Rust，Storage readiness 与 install_mutation 前台授权门合同
tests/macos_signing_policy.rs: 官方安装/后台恢复的上下文选择红测；后台缺失身份不创建不降级
tests/macos_signing_assets.rs: Rust，合成签名身份回归，证明稳定注册、content-addressed verifier 引用、只读读取与严格 Keychain lock 白名单，不触碰宿主/TCC
tests/macos_signing_context.rs: Rust，生产私有身份驱动的 synthetic 多组件 DR/载荷连续性与拒绝回归
tests/macos_signing_registration.rs: Rust，更新注册签名代际回归，覆盖 schema2 兼容、legacy epoch迁移边界、双向CAS漂移与local wrong-proof/损坏硬失败
tests/macos_update_registration.rs: Rust，更新代际与恢复；回归边界
tests/macos_update_restore.rs: Rust，更新代际与恢复；回归边界
tests/native_contract.rs: Rust，公开命令及 Runtime 条件权限预览的无副作用合同；平台原生适配；回归边界
tests/open.rs: Rust，隔离启动；回归边界
tests/probe.rs: Rust，CLI 10/5 MiB、外置 JS/JSON 1 MiB 与本机 50ms 门；预算边界独立回归
tests/readonly.rs: Rust，只读命令不得创建产品状态；回归边界
tests/recovery_cleanup.rs: Rust，恢复后的事务清理；回归边界
tests/release_asset_smoke.rs: Rust，发布边界；回归边界
tests/permission_update.rs: Rust，内部更新任务拒绝伪造与只读预览；不运行宿主
tests/runtime_drift.rs: Rust，外部 Runtime 构建与验证；回归边界
tests/session_privacy.rs: Rust，会话生命周期与清理；回归边界
tests/signing_doctor.rs: Rust，签名身份与 entitlement；回归边界
tests/signing_doctor_custom.rs: Rust，签名身份与 entitlement；回归边界
tests/signing_doctor_identity.rs: Rust，签名身份与 entitlement；回归边界
tests/support/committed_install.rs: Rust，安装变更与回滚；回归边界
tests/support/CLAUDE.md: 通用集成夹具地图；测试入口按职责引用
tests/support/legacy_typescript_matrix.rs: Rust，历史磁盘状态兼容；回归边界
tests/support/mod.rs: Rust，集成测试公共夹具入口；回归边界
tests/support/native_tty.rs: Rust，平台原生适配；回归边界
tests/support/readonly.rs: Rust，只读命令不得创建产品状态；回归边界
tests/support/runtime.rs: Rust，外部 Runtime 构建与验证；回归边界
tests/support/tty.rs: Rust，真实 PTY 测试夹具；回归边界
tests/support/update_menu.rs: Rust，更新代际与恢复；回归边界
tests/tty_harness.rs: Rust，原生菜单 TTY 行为；回归边界
tests/uninstall_safety.rs: Rust，安装变更与回滚；回归边界
tests/update.rs: Rust，脚本更新、TTY 通知和并发代际锁回归
tests/update_homebrew.rs: Rust，替身 brew 路由与代际回归；严格保留已确认发出的诊断，超时前无输出单独验收
tests/update_runtime.rs: Rust，更新后的 Runtime 交接、待修复标记和部分成功回归
tests/update_support/fixture.rs: Rust，共用隔离安装代际与替身工具
tests/update_support/CLAUDE.md: 更新共享夹具地图；测试文件复用同一安装构造
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

assets/incodex-windows.manifest: Windows manifest，明确 asInvoker 与 uiAccess=false，嵌入原生 CLI/helper 而不请求提权

src/windows_installed_native_open.rs: Rust，installed bridge 到受管 native open 的转换，保持会话/Job 所有权管线

src/windows_official_cache.rs: Rust，官方 bundled CUA 的当前用户内容寻址缓存，校验后只发布新代且不改 Store 包

src/windows_package_native.rs: Rust，当前用户 Package/WinRT 与原生路径发现，保留 staged/installed 状态差异

src/windows_prearm_process.rs: Rust，精确 suspended primary 的进程证明，预备失败仍放行官方启动

src/windows_update_observer.rs: Rust，安装路径拥有的 Store 更新观测与待处理目标绑定，不能据 helper 存活推导恢复成功

src/windows_update_observer_log.rs: Rust，更新 observer 的有界事件日志，不收集账号或会话内容

src/windows_update_observer_tests.rs: Rust，订阅/事件/退出的隔离回归合同，不替代真实 Store 更新验收

src/windows_update_prearm.rs: Rust，来源 epoch 绑定的预挂钩与晋升；取消保留未知目标状态

src/windows_update_repair.rs: Rust，Package 更新后的授权替换事务，目标登记与 Runtime 必须匹配 pending intent

src/windows_update_repair_lifecycle.rs: Rust，observer 与 installed bridge 的并行生命周期；失败取消，正常退出等待修复

src/windows_update_startup.rs: Rust，精确普通主进程对预留更新 intent 的消费，复用既有安装事务

tests/windows_execution_manifest.rs: Rust，CLI/helper 的执行权限 manifest 回归，拒绝请求提权

tests/windows_update_repair.rs: Rust，Store 更新 epoch、目标预备与恢复失败的安全合同，不宣称实机已通过

assets/incodex-windows-actions.cjs: Windows 稳定动作控制器，保留请求捕获的 release 与原始失败响应
src/runtime_ui_update.rs: Windows UI/动作代际状态机，真实 ACK 后提交，未知状态保留并回滚
src/windows_runtime_ui.rs: Windows installed CDP 更新适配，验证发布与当前窗口，不改变普通 App 生命周期

tests/version_macos.rs: macOS 版本事实回归，假 PATH 记录子命令，空 PATH 检查降级边界

法则: 成员完整·一行一文件·父级链接·技术词前置
[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
