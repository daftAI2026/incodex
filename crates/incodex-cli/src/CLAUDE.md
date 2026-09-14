# incodex-cli/src/
> L2 | 父级: ../../../CLAUDE.md

Rust 产品 CLI 的命令分发与平台编排层：解析和终端交互共享，ASAR/签名/事务与会话清理委托下层 crate；Windows Store 激活由原生 adapter 承担，不另建 CLI。Shot 仅通过显式实验开关复用 CDP，不改变安装入口。

## 成员清单
app_bundle.rs: Info.plist 可执行文件解析，拒绝非单段文件名，供官方 App 启动前验证。
app_quiescence.rs: macOS App 静止状态到事务 QuiescenceGuard 的桥接，集中执行退出与静止检查。
cdp.rs: Localhost CDP 传输、官方顶层页面筛选和共享 Runtime 注入，按窗口类型验收并承载实验 Shot 请求。
cdp_lifecycle_tests.rs: CDP 生命周期回归，用本地服务器验证页面消失、连接失败和资料遮罩监视的退出信号。
cdp_masked_lifecycle.rs: macOS 资料遮罩持续监督，将页面关闭、连接宽限与连续遮罩失败区分后交回 open。
cdp_mode.rs: Codex 模式 DOM 探测及就绪状态机，官方阻塞 UI 暂停计时，限定回退与失败预算。
cdp_mode_tests.rs: Codex 模式回归，以受控时序和 CDP 响应验证等待、阻塞、回退及终止条件。
cdp_partial_flush_tests.rs: WebSocket 部分写入回归，模拟 WouldBlock，验证有界发送不会重复写帧或丢失守卫检查。
cdp_system_wallpapers.rs: Shot 壁纸 CDP adapter，只路由目录、恢复与不透明资源 ID 请求；双资源任务独立执行，持有并回收 worker，不接收 renderer 文件路径或 URL。
cdp_system_wallpapers_tests.rs: 壁纸协议回归，验证动作白名单、不透明 ID 和额外字段拒绝。
cdp_ui_probe_tests.rs: Runtime 就绪验收回归，分别约束按钮、横幅和资料遮罩结果，拒绝畸形探测值。
cdp_unit_tests.rs: CDP 基础契约回归，覆盖页面筛选、loopback 限制、窗口类型及显式 Shot 开关。
confirm.rs: 破坏性命令统一确认门，TTY 读取平台按键，非交互调用要求 --yes。
diagnose.rs: macOS 安装诊断聚合，组合 App、ASAR、签名、Runtime 和事务检查形成结构化报告。
diagnose_checks.rs: 诊断检查模型及 journal/owner 扫描，保留 checked 与 unknown 的证据边界。
diagnose_checks_tests.rs: 目录读取失败回归，禁止把部分扫描结果误报为完整检查。
diagnose_format.rs: Diagnosis 的文本与序列化呈现，统一 status/doctor 的安装和 Runtime 状态说明。
diagnose_fs.rs: 诊断专用只读文件与进程探测，目录读取错误向上传递，不隐去不确定性。
diagnose_runtime.rs: 外部 Runtime 身份检查，将部署状态与当前内嵌代际比较后生成诊断项。
diagnose_sessions.rs: 会话目录与 owner 证据扫描，区分遗留 Chromium 数据和不确定状态，不执行清理。
diagnose_signing.rs: macOS 签名诊断适配，区分外层身份、深度检查及未请求结果，不把 spctl 当成功门。
diagnosis_presentation.rs: status/doctor 共享进度文案，供两平台诊断入口复用。
friendly_name.rs: 系统随机源驱动临时友好名称生成，供资料遮罩使用，不依赖用户真实身份。
help.rs: 命令帮助与平台能力说明，围绕共享解析器命令集合呈现公开用法。
install.rs: macOS install/uninstall/recover 编排，经过静止检查、备份证明与事务引擎执行 ASAR 和签名变更。
install_keychain_advice.rs: 首次默认官方 App 安装后的 Keychain 提示门，只说明系统授权，不索取密码。
install_tests.rs: macOS 安装编排回归，使用进程与时钟替身验证退出请求、静止检查和错误传播。
legacy_proof.rs: 冻结 TypeScript v1 安装的只读证明门，持 target lock 核对磁盘身份后供原生迁移消费。
legacy_typescript.rs: 冻结 v1 磁盘结构读取器，仅解析历史记录，不恢复已退役 TypeScript 产品执行路径。
lib.rs: CLI 库入口与平台模块注册，统一解析、命令分发及失败退出码，Shot 主机资源留在私有模块。
lifecycle.rs: macOS CLI 更新、自卸载及更新通知编排，按安装渠道更新并通过新 CLI 发布 Runtime。
locale.rs: localeOverride 共享解析，允许调用方明确规定引号策略，不接管平台文件读取。
macos_desktop_wallpaper.rs: AppKit 主线程查询当前桌面路径快照，后续验证和解码交给后台壁纸资源库。
macos_system_wallpapers.rs: 已取证的 macOS 26/27 双壁纸来源，优先本地，按需下载固定 Apple HTTPS 视频并转换；临时源文件随任务清理。
macos_wallpaper_video.rs: AVFoundation 首帧解码，禁止所有外部媒体引用，限制最长边并复用 ImageIO JPEG 编码。
macos_image_io.rs: ImageIO 字节解码与共享 JPEG 编码边界，接收受限静态图像或视频 adapter 的 CGImage，不接受路径或网络来源。
main.rs: 进程入口，将参数交给 CLI 库并按统一错误格式和退出码结束。
menu.rs: 非 Windows 菜单 adapter，提供命令项、终端按键和后台刷新后的更新提示。
menu_controller.rs: 共享菜单状态机，将平台按键映射为选中项或命令，不执行产品变更。
menu_view.rs: 共享菜单文本渲染和光标生命周期管理，消费菜单项与提示，不承担命令决策。
open.rs: 非 Windows 隔离会话与官方进程编排，协调 CDP 就绪、owner 移交和安全销毁；实验 Shot 在此接入。
open_cleanup_tests.rs: open 清理回归，验证先停写入者再销毁、身份检查及不确定结果保留。
open_command.rs: open 命令门面，将解析参数转为资料遮罩和启动计划，dry-run 不发布 Runtime 或创建会话。
open_presentation.rs: 两平台 open 进度和完成结果分类，区分进程失败与 UI 注入未验收。
open_tests.rs: 原生 open 生命周期回归，使用假 App 与 CDP 服务验证隔离计划、就绪和退出行为。
parse.rs: 产品命令语言唯一解析边界，把参数和兼容确认语法转为受约束的 ParsedCli。
profile_mask.rs: 资料遮罩参数及受限头像读取，输出受控名称/图片数据；无副作用 Base64 编码供壁纸复用。
shot_wallpaper_preference.rs: macOS 私有根目录中的 Shot 启用标志，使用目录 fd 和 no-follow，不持久化壁纸路径或图片。
spinner.rs: 平台共用进度动画，按终端能力和宽度呈现并在结束时回收后台线程。
stable_release.rs: 稳定发布元数据和规范三段版本解析，供更新逻辑比较版本，拒绝非规范标签。
system_wallpaper_catalog.rs: 当前桌面既有缩略图候选策略，只计算邻接资源路径，不扫描历史系统版本。
system_wallpaper_catalog_tests.rs: 缩略图候选与双资源来源回归，固定邻接预览、版本身份和 Apple URL 白名单。
system_wallpaper_files.rs: 跨平台 no-follow 有界读取和私有临时目录，静态图片与下载视频共用文件安全实现。
system_wallpapers.rs: 非 macOS 当前桌面资源库及 macOS 旧合同测试入口，以验证后的主机路径建立不透明 ID，限定读取、缩略图与原图传输。
system_wallpapers_tests.rs: 壁纸资源回归，以临时文件验证路径边界、会话索引和资源上限，macOS 解码仅只读系统素材。
terminal.rs: termios 终端输入 adapter，有界读取转义序列并恢复原模式，为菜单和确认提供按键。
terminal_presentation.rs: 终端报告与结果的共享输出间距规范，不承担诊断或命令语义。
version.rs: 平台版本事实采集与格式化，报告 CLI、系统和安装渠道供用户排查。
windows_activation.rs: Store 包激活与 Package Debugger 编排，协调 capability、环境管道、挂起进程和 Job 归属。
windows_activation_capability.rs: Windows 激活 capability 与 debugger 路由校验，把隔离目录、Job 和环境管道绑定为受限能力。
windows_app.rs: 当前用户 Store Codex 包发现与证据验证，从包元数据解析官方程序，不硬编码安装位置。
windows_cleanup.rs: Windows Job 关闭结果到会话清理的门，无法证明关闭时返回 unknown 并保留目录。
windows_cleanup_tests.rs: Windows 清理回归，验证未证实停写时保留会话及删除后重建的观察行为。
windows_console.rs: Win32 控制台 adapter，转换输入事件为共享菜单按键，管理模式、终端宽度与 ANSI 支持。
windows_doctor.rs: Windows doctor 聚合 Store 集成状态和会话检查，输出结构化诊断而不扩大修复能力。
windows_file.rs: Windows 普通文件验证与流式 SHA-256 工具，供 helper、注册和安装证据复用。
windows_helper.rs: 私有目录内发布内容寻址的无控制台 helper，区分安装态与临时激活布局并验证文件身份。
windows_install.rs: Windows Runtime 安装/卸载编排，确认后重验包身份，经过安装状态锁和运行进程门再变更注册。
windows_install_state.rs: Windows 安装状态持久化和 epoch 转换门，结合私有 ACL、包/helper 身份及互斥锁约束代际变更。
windows_installed_cdp.rs: 安装态 Store CDP adapter，证明连接归属后注入正常窗口，受限 binding 仅交给原生 open。
windows_launch.rs: Job 身份认证的本地命名管道，向受控激活进程传递启动模式与环境，拒绝远端客户端。
windows_locale.rs: Windows 配置有界读取，委托共享 locale 解析器并接受该平台的单双引号策略。
windows_locale_tests.rs: Windows locale 读取回归，超限配置不产生语言覆盖值。
windows_menu.rs: Windows 菜单 adapter，仅声明已批准命令并复用共享控制器与 Win32 按键输入。
windows_open.rs: Windows open 信任管线，连接 Store 发现、私有会话、Job/CDP 归属、共享注入和关闭后清理。
windows_open_tests.rs: Windows open 回归，验证官方阻塞提示、窗口消失宽限及隔离进程生命周期。
windows_process.rs: Win32 进程树和 kill-on-close Job 边界，验证包/线程/连接归属并提供窗口存活与关闭证据。
windows_process_parameters.rs: x64 挂起 Store 进程参数 adapter，只在既有命令行缓冲区内追加 localhost CDP 开关。
windows_profile.rs: 当前进程 token 的 Windows 用户目录查询，不依赖可伪造的环境变量路径。
windows_registration.rs: Package Debugger 注册证据的私有持久化，绑定包、helper 路径/哈希及安装状态，支持临时注册恢复。
windows_runtime.rs: Windows 外部 Runtime 与平台资产发布及校验，结合共享资产目录、清单和私有原子文件替换。
windows_runtime_lifecycle.rs: Runtime 启动与关闭纯状态规则，以认证关闭、Job 空闲和可见性决定握手及退出许可。
windows_runtime_open.rs: 安装态 Runtime 隔离启动编排，用命名管道握手绑定包激活、Job 生命周期与安全会话清理。
windows_self_uninstall.rs: Windows 受管 CLI 自卸载，复用安装身份和稳定锁，通过隐藏 PowerShell 等待退出后删除批准范围。
windows_status.rs: Windows status 将 Store 包、安装状态、注册证据及 Runtime 校验组合为可读和 JSON 报告。
windows_system.rs: Windows 系统程序绝对路径解析及展示路径规范化，拒绝不安全相对组件，不依赖 PATH 找系统工具。
windows_update.rs: Windows 受管更新编排，验证安装代际和稳定发布，持稳定锁运行固定安装器并同步 Runtime。
update_flow.rs: 两平台共享更新呈现，串联安装、Runtime 发布和兼容回退回调，不自行操作平台事务。

[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md

windows_runtime_raise.rs: Windows 字节管道有界通信，连接/写入/分段读取共享截止时间，与 Electron node:net 服务协作。
