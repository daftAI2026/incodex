# crates/incodex-runtime-bundle/
> L2 | 父级: ../../CLAUDE.md

成员清单
Cargo.toml: Cargo，macOS 原生 Runtime 发布边界；资产由跨平台 runtime-assets crate 提供
src/lib.rs: Rust，锁内发布 Runtime、helper 和独立 adapter，先落原生资源再提交 current 指针
src/native.rs: Rust，验证 Runtime native 与分离 helper/adapter 的 ABI、源顺序和摘要；adapter 不进 catalog
src/device_key.rs: Rust，发布固定 helper 和原登记；共享私有目录/文件验证原语，不触碰 Storage key
src/device_key_adapter.rs: Rust，锁内发布独立 adapter 内容地址；保留有效 helper 登记，不替换损坏代际
src/tests.rs: Rust，隔离发布与真实 CommonJS loader 时序回归；官方初始化不等窗口 owner

窗口可见性与拒绝 owner 的安全合同由 `src/runtime/experiment-integration.test.ts`、`src/runtime-cleanup-owner.test.ts` 覆盖；不能把官方入口启动误写成窗口已显示。嵌入资产与磁盘 dist 对比要求测试期间冻结产物。

法则: 成员完整·一行一文件·父级链接·技术词前置
[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
