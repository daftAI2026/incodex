# incodex-cli/
> L2 | 父级: ../../CLAUDE.md

## 成员清单

`Cargo.toml`: 原生产品 CLI crate 清单，声明命令编排、平台适配与测试依赖。
`build.rs`: macOS 原生资产构建入口，把 Coordinator 与 Sparkle interposer 编译成可嵌入产物。
`native/`: macOS 更新桥接模块，隔离 Objective-C/AppKit/Sparkle 边界。
`src/`: Rust 产品 CLI，实现解析、安装、诊断、更新和平台生命周期。
`tests/`: 跨进程产品契约与平台回归套件，验证公开行为和危险边界。

[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
