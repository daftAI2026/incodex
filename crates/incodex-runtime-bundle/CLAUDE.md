# crates/incodex-runtime-bundle/
> L2 | 父级: ../../CLAUDE.md

成员清单
Cargo.toml: Cargo，macOS 原生 Runtime 发布边界；资产由跨平台 runtime-assets crate 提供
src/lib.rs: Rust，内容寻址 release 与 current 指针发布，保留不可变代际并验证清单
src/native.rs: Rust，macOS 权限 host/dylib 的固定 ABI 与原生清单发布，不在安装时重建
src/tests.rs: Rust，隔离发布与真实 CommonJS loader 时序回归；官方初始化不等窗口 owner

窗口可见性与拒绝 owner 的安全合同由 `src/runtime/experiment-integration.test.ts`、`src/runtime-cleanup-owner.test.ts` 覆盖；不能把官方入口启动误写成窗口已显示。嵌入资产与磁盘 dist 对比要求测试期间冻结产物。

法则: 成员完整·一行一文件·父级链接·技术词前置
[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
