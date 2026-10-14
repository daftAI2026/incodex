# src/
> L2 | 父级: ../CLAUDE.md

成员清单
build-runtime.ts: 唯一 dist/manifest 构建器；内嵌退出诊断，不扩展资产清单
compatibility.test.ts: TypeScript，版本兼容性边界；回归边界
deploy-runtime.ts: TypeScript，外部 Runtime 构建与验证
forensics.test.ts: TypeScript，隐私取证和绝对无痕声明限制；回归边界
forensics.ts: TypeScript，隐私取证和绝对无痕声明限制
instance-port-lease.test.ts: TypeScript，内核端口租约的排他所有权；回归边界
instance.test.ts: TypeScript，实例归属与重复启动；回归边界
ipc-guard.test.ts: TypeScript，跨进程请求守卫；回归边界
locale.test.ts: TypeScript，语言别名、双权限安装说明与 Skip 语义回归
native-runtime-artifacts.ts: TypeScript，验证原生资产；分别校验视图与 host 支持源的有序摘要
permission-shared-copy.ts: TypeScript，保留AX原文并投影65语言双权限动作与perm指引；按系统版本替换权限名
permission-screen-copy.ts: TypeScript，65 语言小卡动作模板与录屏行；显式分离应用名/权限名/语义 runs，中繁日 helper 句无句末标点
runtime-cleanup-owner.test.ts: TypeScript，外部 Runtime 构建与验证；回归边界
runtime-late-recreation.test.ts: TypeScript，外部 Runtime 构建与验证；回归边界
runtime-load.test.ts: Runtime 完整清单、动作代际与固定原生资产的兼容回归
runtime-hot-bootstrap.test.ts: 冻结验证器与 23 项资产门；独立固定诊断 main 锚，迁移不得触发热 staging
runtime-macos-update.test.ts: TypeScript，默认 ASAR 模块路径、交接就绪与失败放行回归，不更新宿主
runtime-permission-handoff.test.ts: 原 requestId 续接、私有 marker 原子发布与原生单 owner 回归
runtime-main-injection.test.ts: 主 Runtime 组合回归；ACK 后原生续接与单 owner、Skip 终态，不执行 App 安装
runtime-main-session.test.ts: TypeScript，会话生命周期与清理；回归边界
runtime-manifest.ts: TypeScript，共用唯一资产 catalog 与 manifest 类型/读写，构建和验证不另造名单
runtime-session-contract.test.ts: TypeScript，会话生命周期与清理；回归边界
runtime-session-process.test.ts: TypeScript，会话生命周期与清理；回归边界
runtime-windows-main-session.test.ts: TypeScript，会话生命周期与清理；回归边界
safe-home.test.ts: TypeScript，隔离 home 安全边界；回归边界
window-kind.test.ts: TypeScript，窗口归属与显示；回归边界
runtime/: Electron 侧执行模块与回归边界，成员详见 runtime/CLAUDE.md。

法则: 成员完整·一行一文件·父级链接·技术词前置
[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
