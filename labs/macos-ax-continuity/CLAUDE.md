# labs/macos-ax-continuity/
> L2 | 父级: ../CLAUDE.md

成员清单
continuity.py: Python 非发行后端，固定私有证书身份、明确 RC/载荷编号并签名不同内容的实验应用，不触碰产品 Runtime。
identity-store.m: Security.framework 一次性动态域钥匙串初始化/解锁，ACL 仅信任系统 codesign，不进入用户搜索列表。
helper.swift: AppKit 真实实验应用，以当前进程 trust 和 Finder 公共菜单 AX 读取共同验收，不借用操控终端授权。
README.md: RC1 实验合同，定义一次授权后的不同内容换代与真实 AX 读操作，不把 Helper 权限冒充 Codex 权限。
test_continuity.py: Python 失败先行合同；约束私有长期身份复用、完整性拒绝和候选编号，本身不证明 TCC 连续性。

[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
