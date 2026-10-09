# labs/macos-ax-continuity/
> L2 | 父级: ../CLAUDE.md

成员清单
test_native_identity.py: 原生工具链回归，仅创建临时 synthetic 身份并核对 search list 不变，不验证 TCC。
continuity.py: Python 非发行后端，固定私有证书身份、明确 RC/载荷编号并签名不同内容的实验应用，不触碰产品 Runtime。
identity-store.m: Security.framework 一次性私有钥匙串初始化/解锁并只读核验 search list，ACL 仅信任系统 codesign，不进入用户搜索列表。
helper.swift: AppKit 真实实验应用，以当前进程 trust 和 Finder 公共菜单 AX 读取共同验收，不借用操控终端授权。
README.md: RC1 冻结载荷与实测合同，并导航 RC2 接入边界；明确 synthetic 成功不等于正式宿主验收。
test_continuity.py: Python 失败先行合同；约束私有长期身份复用、完整性拒绝和候选编号，本身不证明 TCC 连续性。

[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
