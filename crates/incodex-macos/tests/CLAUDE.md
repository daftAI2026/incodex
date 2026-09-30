# crates/incodex-macos/tests/
> L2 | 父级: ../CLAUDE.md

成员清单
add_load_dylib.rs: Rust，普通 LC_LOAD_DYLIB 的原子校验与写入；回归边界
asar_integrity_digest.rs: Rust，ASAR 读取、校验与变更；回归边界
asar_integrity_plist.rs: Rust，ASAR 读取、校验与变更；回归边界
asar_integrity_signing.rs: Rust，ASAR 读取、校验与变更；回归边界
keychain_integrity_signing.rs: macOS 实验与指定基准的真实工具链合流证据，不访问用户 App 或 Keychain
quiescence.rs: Rust，进程静默证明；回归边界
session_processes.rs: Rust，会话生命周期与清理；回归边界
signing_components.rs: Rust，签名身份与 entitlement；回归边界
signing_policy.rs: Rust，签名身份与 entitlement；回归边界
signing_regressions.rs: Rust，签名身份与 entitlement；回归边界
support/: 共享合成签名 fixture，详见 support/CLAUDE.md。

法则: 成员完整·一行一文件·父级链接·技术词前置
[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
