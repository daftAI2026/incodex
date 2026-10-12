# crates/incodex-runtime-assets/
> L2 | 父级: ../../CLAUDE.md

成员清单
Cargo.toml: Cargo，只引入编译期 JSON 解析，不依赖平台工具链。
build.rs: 编译期资产装配，校验唯一目录并嵌入已提交 dist。
src/lib.rs: 只读静态表门面，供跨平台 CLI 与 macOS 发布器复用。

[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
