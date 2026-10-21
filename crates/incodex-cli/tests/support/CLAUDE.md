# support/
> L2 | 父级: ../../CLAUDE.md

成员清单
mod.rs: Rust，暴露 Runtime 夹具与共享 PTY 驱动。
committed_install.rs: Rust，构造合成包的已提交安装事务。
legacy_typescript_matrix.rs: Rust，冻结 v1 读取器的磁盘状态矩阵。
native_tty.rs: Rust，native_contract 套件验证菜单顺序与编号提示；Launch 仅在 macOS 出现，PTY 不触发 Launch。
readonly.rs: Rust，只读 CLI 的隔离 HOME 与 JSON 断言工具。
runtime.rs: Rust，构造旧 Runtime 指针的兼容夹具。
tty.rs: Rust，跨套件串行 PTY 驱动与有界退出。
update_menu.rs: Rust，update 套件的后台通知与菜单回归；稳定Quit文案不绑定平台编号。
CLAUDE.md: 共享集成夹具地图。

[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
