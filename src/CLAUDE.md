# src/
> L2 | 父级: ../CLAUDE.md

成员清单
build-runtime.ts: 唯一 dist/manifest 构建器；从原始动作源和adapter双pin生成同代token，不扩展资产清单
compatibility.test.ts: TypeScript，版本兼容性边界；回归边界
deploy-runtime.ts: 仅复制开发载荷到已有 target；不代替 Rust 产品发布
forensics.test.ts: TypeScript，隐私取证和绝对无痕声明限制；回归边界
forensics.ts: TypeScript，隐私取证和绝对无痕声明限制
instance-port-lease.test.ts: TypeScript，内核端口租约的排他所有权；回归边界
instance.test.ts: TypeScript，实例归属与重复启动；回归边界
ipc-guard.test.ts: TypeScript，跨进程请求守卫；回归边界
locale.test.ts: TypeScript，语言别名、双权限安装说明与 Skip 语义回归
native-runtime-artifacts.ts: TypeScript，校验权限与旧remote-key addon；固定helper和hot adapter不入Runtime清单
permission-shared-copy.ts: TypeScript，保留AX原文并投影65语言双权限动作与perm指引；按系统版本替换权限名
permission-screen-copy.ts: TypeScript，65 语言小卡动作模板与录屏行；显式分离应用名/权限名/语义 runs，中繁日 helper 句无句末标点
runtime-cleanup-owner.test.ts: VM 注入真实权限模块；验证普通 Runtime 仍保留唯一清理 owner。
runtime-late-recreation.test.ts: TypeScript，外部 Runtime 构建与验证；回归边界
runtime-load.test.ts: Runtime 完整清单、动作代际与固定原生资产的兼容回归
runtime-install-permission.test.ts: VM执行真实loader/probe，显式目录通知验证安装退出契约；Node probe套件另验OS事件，不启动宿主。
runtime-hot-bootstrap.test.ts: 冻结验证器、激活token、旧动作拒绝与Windows原字节checkout门；不放宽热换白名单。
runtime-macos-update.test.ts: TypeScript，默认 ASAR 模块路径、交接就绪与失败放行回归，不更新宿主
runtime-permission-handoff.test.ts: Node 子进程执行真实 fs.watch；验证 marker 权限、requestId 续接、原子发布、owner 退休与 watcher dispose
runtime-main-injection.test.ts: 主 Runtime 组合回归；ACK 后原生续接与单 owner、Skip 终态，不执行 App 安装
runtime-main-session.test.ts: TypeScript，会话生命周期与清理；回归边界
runtime-manifest.ts: TypeScript，共用唯一资产 catalog 与 manifest 类型/读写，构建和验证不另造名单
runtime-session-contract.test.ts: TypeScript，会话生命周期与清理；回归边界
runtime-session-process.test.ts: TypeScript，会话生命周期与清理；回归边界
runtime-windows-main-session.test.ts: TypeScript，会话生命周期与清理；回归边界
safe-home.test.ts: 会话身份与销毁回归；Node 子进程验证只读目录、外部链接和中断重试
window-kind.test.ts: TypeScript，窗口归属与显示；回归边界
runtime/: Electron 侧执行模块与回归边界，成员详见 runtime/CLAUDE.md。

法则: 成员完整·一行一文件·父级链接·技术词前置
[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
