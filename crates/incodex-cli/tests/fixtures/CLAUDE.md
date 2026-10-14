# fixtures/
> L2 | 父级: ../../CLAUDE.md

成员清单
runtime-load-rc11.cjs: 冻结已部署验证器字节；SHA256 c2827f，用于旧宿主升级回归。
runtime-manifest-rc11.json: 冻结 RC11 完整清单；固定资产不能随权限业务变更。
incodex-loader-v0.3.1.cjs: 历史 ASAR 引导字节，只供旧安装兼容回归。
incodex-loader-pre-install-permission.cjs: 原样冻结旧 Loader；新安装前置入口不能破坏旧安装识别。
permission-responder-rc4.cjs: 冻结主入口原始 IIFE；验证真实 Rust requestId，不进入产品。
CLAUDE.md: 本历史夹具地图。

[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
