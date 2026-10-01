# INCODEX-V1.1-MAC-AX-RC1

本地、未发行的 macOS 授权连续性实验。先隔离证明“不同内容仍是同一获权身份”，不在真实 Codex 上试签名；此实验不是两套产品安装，也不是 AX 权限代理。

## 从 Storage 复用什么

私人文档 `incodex/rust-cli/install-keychain-reminder-investigation.md` 与 `macOS-update-auto-restore-task.md` 已记录：Storage 由首次获权的固定 Helper 承担真实查询，普通 CLI/Runtime/宿主更新不轮换它，provider 窄接调用且失败回退官方路径；后台 readiness 试读曾引发系统框，已删除。

本实验复用稳定量/易变量分离、私有注册、首次身份冻结和实操作验收。身份存放在候选目录外的 `stable-identity/`，RC 换号不自动换证书；缺失或篡改拒绝自动重建。差异是 AX/TCC 判断实际责任主体：Helper 获权不等于 ChatGPT 获权，不能代理一个 trust 布尔值便宣布宿主修复。

## 编号与边界

- 候选：`INCODEX-V1.1-MAC-AX-RC1`，基础 Runtime `1.1.0`；不修改产品 semver。
- `v1/v2` 是本候选预先定义的两份内容不同载荷，不是 CLI 版本。
- 冻结清单记录 source commit/source digest、Mach-O hash、证书指纹与 DR；冻结后源码修改必须换 RC，不覆盖旧候选。
- 未冻结源码禁止构建编号候选。普通实验身份与长期获权身份是两层，不以 RC 号重新授权。
- 不改 `/Applications/ChatGPT.app`，不发布 Runtime，不写 TCC，不自动授权，不使用 identifier-only DR、不伪造 OpenAI Team。
- 身份后端使用独立钥匙串的进程动态 preference domain、限制 codesign ACL；不导入登录钥匙串、不修改 trust settings。系统若拒绝签名则失败，不能以修改系统 trust 兜底。
- 实验会生成新的 synthetic 证书/私钥，仅保留在当前用户 `0700` 目录/`0600` 文件；不得提交它们。

## 构建与验证

```bash
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s labs/macos-ax-continuity -v
python3 labs/macos-ax-continuity/continuity.py --dry-run
# 源码提交后，在唯一候选私有目录构建；已存在候选禁止覆盖。
python3 labs/macos-ax-continuity/continuity.py
```

第一层验证：v1/v2 的 binary hash 不同、证书与 bundle ID 相同、两份 strict 验证通过且满足同一证书绑定 DR；钥匙串 search list 构建前后必须不变。签名通过不代表 TCC 通过。

第二层验证必须由 LaunchServices 启动实验 GUI（而非仅从已授权终端执行 `--probe`），对首次 v1 由用户批准。正常退出自己的 v1，再换到同一实验安装路径的 v2；比较 `selfTrusted`、Finder 公共菜单节点读取和窄窗口 tccd 责任主体。Helper 不读取菜单文字、窗口标题、文件、聊天或登录信息；`--report` 仅写私有目录脱敏状态。无权限时不自动开启 Settings，只有用户点击才开启。

第二层尚未执行；未经系统批准或仅有终端探针结果不得标记连续性通过。当前没有安全的自动换代安装器，v1/v2 路径变化的实验不得冒充同路径原地升级。

后续才讨论接入既有 Rust signing/install/update recovery 边界，并验证 Framework/helper/Sparkle 同域、CUA 官方签名保留、卸载还原和用户撤销权限；不另建 Session Agent 或永久 broker。真实 Codex 跨版本权限保留尚未实现。

[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
