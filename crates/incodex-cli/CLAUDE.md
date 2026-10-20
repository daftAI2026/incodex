# crates/incodex-cli/
> L2 | 父级: ../../CLAUDE.md

成员清单
src/macos_launch.rs: Rust，启动宿主前发布Finder入口；已安装包复用原Runtime，原厂走CDP。
src/macos_launch_cli.rs: Rust，保留安装渠道的稳定CLI入口，不固化Cellar代际路径。
src/macos_launch_bridge_tests.rs: Rust，自有loopback/CDP传输与精确ACK回归，不代真实窗口验收。
src/macos_launch_bridge.rs: Rust，共享动作factory的CDP转接；发现暂缺时保留连接与请求，聚合双ACK。
src/macos_launch_process.rs: Rust，绑定PID/start/executable与逐FD kernel socket身份；同socket继承FD可共享。
src/macos_launch_runtime.rs: Rust，macOS目录事件和已验证代际适配，复用UiUpdate提交与回滚。
src/macos_launch_icon.rs: Rust，原子发布短命Finder入口；外部同名bundle与symlink保持原样。
src/macos_launch_owner.rs: Rust，专用目标锁与普通宿主记录，重复入口不新建controller。
src/open_launch_control.rs: Rust，已验证launch子进程的闭集stdio控制；页面及OS确认后回复置前。
src/windows_installed_native_open_tests.rs: Rust，原就绪与私密置前控制回归；失败保留既有owner。
Cargo.toml: Cargo，定义模块依赖与平台编译边界
assets/incodex-windows-bootstrap.cjs: cjs，窗口归属与显示
assets/incodex-windows-platform.cjs: cjs，窗口归属与显示
build.rs: Cargo build script，跟踪共享诊断与pending头，合流 macOS 原生资产与 Windows manifest；不跨平台启动工具链。
native/CLAUDE.md: L2，逐文件导航原生资产与更新、Keychain 职责边界。
native/fishhook.c: C，Mach-O 符号重绑定
native/fishhook.h: C header，Mach-O 符号重绑定
native/macos_keychain_helper.m: Objective-C，Keychain 连续性
native/macos_keychain_provider.c: C，Keychain 连续性
native/macos_signing_store.m: Objective-C，私有稳定签名身份 Keychain 创建/只读验证/解锁，不改搜索列表或信任
native/macos_sparkle_interpose.m: Objective-C，精确Sparkle目标替换与排队信号；不调用安装block
native/macos_update_coordinator.m: Objective-C，退出事件与交接所有权协调；活宿主等待后重验登记 CLI，正常关闭不重开
native/macos_update_pending.h: Objective-C header，共用pending磁盘协议、异常锁释放、身份比较与排队标记。
native/macos_update_diagnostics.h: Objective-C header，默认关闭的闭集更新回执；产品写系统日志，测试固定 stderr。
src/permission_guide_rows_tests.rs: 无窗状态机回归；锁内迟到 grant 不 reset，意图先锁，Unknown 与 AX 隔离
src/accessibility_guide_host.rs: Rust，有界 stdio、分项操作与原版人工验证策略；UI 不执行 reset
src/accessibility_guide_host_tests.rs: Rust，既有 AX、协议与语言选择的隔离回归
src/permission_official_flow_tests.rs: Rust，原版拖放收据的无窗回归；屏幕未知不得转成授权。
src/permission_guide_protocol.rs: Rust，nonce 绑定的封闭事件解码；不执行权限操作。
src/permission_guide_flow.rs: Rust，三入口共用双行状态机；原版收据仅结束引导，安装仍须探针授权。
src/permission_guide_install.rs: Rust，安装前置适配；LaunchServices 短命宿主复用 nonce，变化提示才复查；隔离共用收尾内核，不放宽 App 身份。
src/permission_update_flow_tests.rs: Rust，无窗更新预检与双行共享截止时间回归；区分授权、用户终态、EOF 与完成
src/permission_install_chain_tests.rs: Rust，默认安装收尾串联；真实 nonce/Runtime 子进程/Swift stdio，不代 LaunchServices 或实机 UI。
src/permission_install_chain_fixture.rs: Rust，生产安装变更与 ad-hoc 签名合成包；不启动系统 App。
src/permission_install_flow_tests.rs: Rust，无窗安装前置回归；启动缺主体不提前结束，真实未知不盲重试；缺项不等工作窗。
src/permission_machine_terminal_tests.rs: Rust，无窗机器终态回归；错误页只有真实 Later/Close 可延期，Screen reset 由成功 Mock 提供
src/permission_update.rs: Rust，已登记 helper 的按代检查；保留原请求并记录任务来源，不启动宿主
src/permission_guide_update.rs: Rust，只读固定原生卡资产；不倒退 Runtime 或重建进行中的引导
src/permission_guide_system.rs: Rust，同进程快照；初始主体未就绪不发录屏请求，保留分项 reset 与有界日志。
src/permission_guide_copy.rs: Rust，安装与运行中共用语言；续接错误指向 incodex perm。
src/permission_screen_probe.rs: Rust，nonce 与进程绑定录屏请求；安装复用同协议，退出前完成身份核验。
src/accessibility_restore.rs: Rust，恢复原版的权限衔接；人工收据不转授权，perm 可继续引导
src/accessibility_setup.rs: Rust，单owner与请求CAS；更新重读旧granted，同代Skip去重，未完意图保留
src/accessibility_setup_tests.rs: Rust，原请求续接、短锁争用与终态 CAS 回归；不读真实 TCC
src/permission_update_request_tests.rs: 更新重读旧granted、同代Skip与pending复用回归；不读TCC
src/accessibility_target.rs: Rust，APFS 支持判断与单次引导的目标连续性守卫
src/app_bundle.rs: Rust，App bundle 路径与执行文件解析
src/app_quiescence.rs: Rust，精确PID静止证明；事务前busy映射闭集延期，探针失败不重试
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
src/help.rs: Rust，公开命令帮助；macOS 提供 perm 双权限用法，Windows 不扩展权限命令
src/install_proof.rs: ASAR marker/COMMITTED/live/backup 的只读绑定证明，共用明确的 root 签名政策
src/install_mutation.rs: 危险事务与签名复验；共享入口只保留恢复初始探针的busy码，事务错误仍为普通失败
src/macos_signing.rs: 注册身份在安装/恢复/验收间的唯一适配，后台无注册硬失败，复用 coordinator.log
src/install_permission_setup.rs: Rust，默认安装收尾；真实 owner/CAS/签名复验后授权；终态落盘失败不启动或宣称成功。
src/install.rs: 原生安装编排及启动器只读归属识别；新Loader先授权，卸载成功后撤销更新登记。
src/install_keychain_advice.rs: Rust，Keychain 连续性
src/install_tests.rs: Rust，正常退出、失败卸载保留更新登记与冻结旧 Loader 兼容回归。
src/legacy_proof.rs: Rust，历史磁盘状态兼容
src/legacy_typescript.rs: Rust，历史磁盘状态兼容
src/lib.rs: Rust，唯一CLI分派与退出码边界；保留内部恢复结果
src/lifecycle.rs: Rust，Runtime/CLI 生命周期编排，发布外置资产后以同 epoch 刷新实验恢复登记而不重签宿主
src/locale.rs: Rust，语言策略
src/macos_keychain_assets.rs: Rust，Storage 固定获权 Helper 注册与显式授权，普通更新不轮换获权身份
src/macos_keychain_protocol.rs: Rust，Keychain 连续性
src/macos_signing_assets.rs: Rust，稳定本机签名身份注册、只读实际 Keychain 验证与显式解锁
src/macos_signing_files.rs: Rust，本机身份私有文件、权限与原子发布合同
src/macos_signing_process.rs: Rust，本机身份外部工具的有界进程执行
src/macos_update_assets.rs: Rust，解析源 CLI 链接后发布 helper；保留私有目标拒绝链接、epoch/signing CAS 与 local outer proof。
src/macos_update_log.rs: Rust，更新代际与恢复
src/macos_update_restore.rs: Rust，后台恢复与权限检查交接；保留signer/epoch、闭集busy退出码与原日志
src/main.rs: Rust，产品二进制入口与退出码
src/menu.rs: Rust，原生菜单交互
src/menu_controller.rs: Rust，原生菜单交互
src/menu_view.rs: Rust，原生菜单交互
src/open.rs: Rust，私有会话计划、owner交接和清理；open_process监督child，不改包
src/open_process.rs: Rust，child监督与回收；保留PID、wait状态和worker关闭语义。
src/open_process_spawn.rs: Rust，macOS posix_spawn适配；关闭额外FD，缺disclaim接口时拒绝启动。
src/open_exit_tests.rs: Rust，隔离退出、信号、回收与owner handoff回归；只启动合成子进程。
src/open_cleanup_tests.rs: Rust，隔离启动
src/open_command.rs: Rust，隔离open适配；launch请求复验CLI与Runtime一致，不倒退发布。
src/open_native_close.rs: Rust，隔离启动
src/open_presentation.rs: Rust，隔离启动
src/open_process_tests.rs: Rust，spawn契约和生产owner handoff回归；只启动测试进程。
src/open_tests.rs: Rust，CDP注入、native-close前置与session清理回归。
src/parse.rs: Rust，命令解析；仅接受公开 perm 并映射稳定内部权限命令
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
src/windows_update_native_tests.rs: Runtime 发布前与旧 helper 消费同一受管 CLI 的权限、版本和保留代际回归；仅使用私有临时目录。
tests/accessibility_diagnosis.rs: Rust，辅助功能权限证据与引导；回归边界
tests/diagnose_process_identity.rs: Rust，诊断证据；回归边界
tests/doctor/transaction_evidence.rs: Rust，事务证据；回归边界
tests/doctor_depth.rs: Rust，诊断输出；回归边界
tests/doctor_symlink_truth.rs: Rust，诊断输出；回归边界
tests/doctor_truth.rs: Rust，诊断输出；回归边界
tests/fixtures/: 历史 Loader 与 responder 字节，只用于兼容回归。
tests/fixtures/install-permission-runtime.cjs: cjs，原样dist短命进程夹具；保留真实PID/start/nonce，移除回执只认实际lstat缺席。
tests/fixtures/incodex-loader-v0.3.1.cjs: cjs，ASAR 到外部 Runtime 的加载；回归边界
tests/fixtures/incodex-loader-pre-install-permission.cjs: cjs，安装前置入口之前的原样 Loader，锁定历史安装兼容。
tests/fixtures/permission-responder-rc4.cjs: cjs，原始 UUID responder 与真实 Rust 生产器交接；不加载真实 App。
tests/fixtures/runtime-load-rc11.cjs: cjs，冻结旧验证器的真实准备/提交回归，不进入产品。
tests/fixtures/runtime-manifest-rc11.json: JSON，已部署固定资产摘要，防止业务更新扩大启动边界。
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
tests/macos_signing_context.rs: Rust，显式本地证书实签验收；两例多组件DR/载荷连续性与拒绝回归，默认忽略
tests/macos_signing_registration.rs: Rust，默认schema/CAS与legacy不迁移回归；证书代际提升和错误签名拒绝两例归显式本地验收
tests/macos_sparkle_diagnostics.rs: macOS，执行隔离 interposer smoke；验证原回调与诊断，不启动真实更新。
tests/macos_update_diagnostics.rs: Rust，编译并执行隔离 Coordinator；验证 opt-in 和普通退出，不启动官方 App。
tests/macos_update_continuation.rs: macOS，排队普通退出后续接；真实子进程、owner与helper回执，不代开二阶段。
tests/CLAUDE.md: L2，CLI 测试成员与临时进程边界。
tests/native/CLAUDE.md: L2，续接哨兵、重开探针与测试支撑边界。
tests/macos_update_registration.rs: Rust，更新代际与恢复；回归边界
tests/macos_update_restore.rs: Rust，更新恢复busy码经共享install入口传播；pending锁回归
tests/native_contract.rs: Rust，公开命令及 Runtime 条件权限预览的无副作用合同；平台原生适配；回归边界
tests/open.rs: Rust，隔离启动；回归边界
tests/probe.rs: Rust，CLI 10/5 MiB、外置 JS/JSON 1 MiB 与本机 50ms 门；预算边界独立回归
tests/readonly.rs: Rust，只读命令不得创建产品状态；回归边界
tests/recovery_cleanup.rs: Rust，恢复后的事务清理；回归边界
tests/release_asset_smoke.rs: Rust，发布边界；回归边界
tests/permission_update.rs: Rust，内部更新任务拒绝伪造与只读预览；不运行宿主
tests/runtime_drift.rs: Rust，外部 Runtime 构建与验证；回归边界
tests/session_privacy.rs: Rust，源码渠道拒绝自卸载与自建文件哨兵；依赖可识别的源码构建布局
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

src/windows_installed_native_open.rs: Rust，共享 native open 单owner及真实OPENED回执；平台入口只适配进程参数。

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

assets/incodex-windows-actions.cjs: 共享CDP稳定动作控制器，保留Windows导出兼容与请求捕获代际。
src/runtime_ui_update.rs: 共享CDP UI/动作代际状态机，真实 ACK 后提交，未知状态保留并回滚
src/windows_runtime_ui.rs: Windows installed CDP 更新适配，验证发布与当前窗口，不改变普通 App 生命周期

tests/version_macos.rs: macOS 版本事实回归，假 PATH 记录子命令，空 PATH 检查降级边界

法则: 成员完整·一行一文件·父级链接·技术词前置
[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
