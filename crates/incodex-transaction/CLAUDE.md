# crates/incodex-transaction/
> L2 | 父级: ../../CLAUDE.md

成员清单
Cargo.toml: Rust crate，定义事务引擎依赖和平台边界。
src/durable.rs: Rust，原子发布 journal 并同步目录；文件和目录权限固定。
src/engine.rs: Rust，驱动备份、staging、swap、commit 与 recovery 状态机。
src/engine_tests.rs: Rust，验证引擎读回、失败注入与状态恢复。
src/journal.rs: Rust，定义 V2 journal、相对路径合同、阶段迁移和校验和。
src/lib.rs: Rust，导出事务 API 并拒绝重定向 storage root。
src/lock.rs: Rust，按 target 身份获取独占事务锁并验证 owner。
src/proof.rs: Rust，生成并核验目录身份、内容与权限摘要。
src/uninstall.rs: Rust，恢复 live tree 并清理事务残留。
tests/durability.rs: Rust，验证 rename checkpoint 只在目录持久化后发布。
tests/extreme_sigkill.rs: Rust，验证各 swap 间隙遭进程终止后的 recovery。
tests/legacy_compat.rs: Rust，验证旧 journal 的安全读取与迁移。
tests/phase_guards.rs: Rust，拒绝不匹配事务阶段的操作。
tests/pre_swap_cleanup.rs: Rust，验证 swap 前清理不会跟随 staging symlink。
tests/proof_guards.rs: Rust，验证快照内容或权限变化会阻止 seal 与 swap。
tests/quiescence.rs: Rust，验证恢复前的 target 静止守卫。
tests/reconstructed_guards.rs: Rust，拒绝重建 journal 路径的 symlink 越界。
tests/recover_cleanup_retry.rs: Rust，验证恢复意图持久化后可重试清理。
tests/recover_identity.rs: Rust，拒绝恢复到已替换的 live target。
tests/restore_safety.rs: Rust，验证 restore 不覆盖身份不匹配的路径。
tests/sigkill.rs: Rust，验证事务各 durable phase 的中断恢复。
tests/support/sigkill.rs: Rust，提供 crash 测试共用的合成 App 和摘要工具。
tests/v2.rs: Rust，验证 V2 journal、锁、路径身份与持久化合同。

法则: 成员完整·一行一文件·父级链接·技术词前置
[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
