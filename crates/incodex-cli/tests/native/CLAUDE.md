# crates/incodex-cli/tests/native/
> L2 | 父级: ../CLAUDE.md

成员清单
install-permission-presenter.swift: Swift，无窗 Presenter 回调替身；事件由真实 host 入口发出，不探测权限。
macos-update-continuation-helper.c: C，登记哨兵；先报告PID/PPID，再等FIFO放行并写成功结束回执。
macos-update-continuation-host.c: C，假宿主重开探针；标记路径在编译期绑定，不依赖LaunchServices继承环境。
macos_update_continuation_support.rs: Rust，自有目录与进程工具；执行有界等待、精确PID核验和测试收尾。

主测试编译当前产品Coordinator，不代开二阶段。
哨兵不执行install，也不读取官方账户或权限。
[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
