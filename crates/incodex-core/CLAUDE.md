# crates/incodex-core/
> L2 | 父级: ../../CLAUDE.md

成员清单
Cargo.toml: Rust crate 配置，限定 macOS/Unix 与 Windows 依赖边界
src/lib.rs: crate 导出门面，按平台暴露路径、目标与 session 能力
src/canonical.rs: Unix，规范化 App 路径并绑定目标 inode 与 device
src/paths.rs: Unix，定义官方 App 与用户私有目录路径
src/print.rs: 跨平台，统一 CLI 标签、步骤与状态格式
src/session.rs: Unix，创建隔离 session，复制允许设置，并校验 owner 后销毁
src/session/readonly_orphan_tests.rs: Unix，验证同步 orphan sweep 保留 0500 子树与内容
src/session/window_state.rs: Unix，投影窗口几何与官方空 Home 初始化哨兵
src/session_layout.rs: 跨平台，集中定义 session 目录与设置文件名
src/session_tests.rs: Unix，真实临时目录回归 session 身份、权限与销毁行为
src/target.rs: Unix，生成稳定的官方或自定义 App 目标 ID
src/windows_path.rs: Windows，拒绝 reparse ancestry 并验证 session 路径边界
src/windows_session.rs: Windows，创建、检查、清理带私有 ACL 的 session
src/windows_session_owner.rs: Windows，读写 owner 身份并探测进程存活状态
src/windows_settings.rs: Windows，仅投影允许的设置与窗口状态
tests/windows_paths.rs: Windows，验证 Unicode 路径、junction 与可信根边界
tests/windows_session.rs: Windows，验证 session 生命周期、ACL 与设置投影

法则: 成员完整·一行一文件·父级链接·技术词前置
[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
