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
- 身份后端使用独立私有钥匙串、限制 codesign ACL 和 search-list 前后只读不变门；不导入登录钥匙串、不修改 trust settings。系统若拒绝签名则失败，不能以修改系统 trust 兜底。
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

2026-10-01 第二层已由主代理脚本实测：v1 PID 11190 首次由用户批准后 `selfTrusted=true`、Finder AX 读取 PASS；正常退出且内核确认旧 PID 不在后，同一 `active/Incodex AX Continuity Lab.app` 路径替换为 v2 PID 11622。v2 首次观测 trust 与实际 AX 读取均 PASS，未请求第二次批准；tccd 两轮 subject 均为实验 bundle，不借用终端权限。源码候选冻结于 `7c19cb02`，两份 binary hash 不同、证书/DR 相同。

运行态证据在本机私有候选目录的 `continuity-result.json`、`v1/v2-granted-proof.json`、`switch-proof.json` 和筛选 tccd 日志；私人文档独立归档。`candidate.json` 保留构建时 `tccContinuity=NOT_RUN`，它是不可变构建清单，不代表后续运行态。

此次 PASS 仅证明 synthetic 换代连续性；RC1 本身没有产品自动换代安装器，也未验证不同证书、撤销权限、卸载回官方与正式宿主跨版。两份不同路径的签名校验本身不得冒充此次同路径运行结果。

RC2 接入沿用既有 Rust signing/install/update recovery，增加每设备私有稳定身份、逐组件 DR、签名代际 CAS 和原 Coordinator 阶段日志；synthetic host/Sparkle/updater 的换代签名与 Doctor 合同已经验证。RC1 的构建载荷及身份保持不变，生产身份不得复制 RC1 私钥。

真实宿主仍是旧 ad-hoc 安装，尚未部署 RC2 或完成正式更新后的 AX 连续性验收；需要明确卸载/重新安装迁移及首次用户授权，再用原私人监控脚本观察下一次更新。签名正确不等于 AX 实操作成功，跨设备也不继承 TCC 授权。不另建 Session Agent、永久 broker 或监控服务。

[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
