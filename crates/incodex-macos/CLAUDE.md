# incodex-macos/
> L2 | 父级: ../../CLAUDE.md

## 成员清单

`Cargo.toml`: macOS 平台 crate 清单，声明 plist、签名检查与原生系统调用所需依赖。
`src/`: macOS 危险边界实现，统一负责应用退出、Mach-O 改写、宿主/Framework 同代签名、CUA sidecar 保留、窗口与进程识别。
`tests/`: macOS 平台回归套件，以合成 bundle 和系统 seam 验证签名、静止态与进程安全契约。

[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
