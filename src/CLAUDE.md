# src/
> L2 | 父级: ../CLAUDE.md

成员清单
build-runtime.ts: TypeScript，唯一 dist/manifest 构建器；局部标识符压缩保留入口与函数名，ASAR loader 不变
compatibility.test.ts: TypeScript，版本兼容性边界；回归边界
deploy-runtime.ts: TypeScript，外部 Runtime 构建与验证
forensics.test.ts: TypeScript，隐私取证和绝对无痕声明限制；回归边界
forensics.ts: TypeScript，隐私取证和绝对无痕声明限制
instance-port-lease.test.ts: TypeScript，内核端口租约的排他所有权；回归边界
instance.test.ts: TypeScript，实例归属与重复启动；回归边界
ipc-guard.test.ts: TypeScript，跨进程请求守卫；回归边界
locale.test.ts: TypeScript，语言策略；回归边界
native-runtime-artifacts.ts: TypeScript，验证原生资产；分别校验视图与 host 支持源的有序摘要
permission-shared-copy.ts: TypeScript，合并 AX/Screen Recording 文案；校验 65 个 locale 对齐并保留输入
permission-screen-copy.ts: TypeScript，65 语言录屏与系统录音引导；路径语义 runs 与平台名称分离
runtime-cleanup-owner.test.ts: TypeScript，外部 Runtime 构建与验证；回归边界
runtime-late-recreation.test.ts: TypeScript，外部 Runtime 构建与验证；回归边界
runtime-load.test.ts: Runtime 启动与已安装 Loader 兼容性的回归边界
runtime-macos-update.test.ts: TypeScript，默认 ASAR 模块路径、交接就绪与失败放行回归，不更新宿主
runtime-main-injection.test.ts: 主 Runtime 组合契约与旧 loader/native-open 单 owner 回归，不执行真实 App 安装
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
