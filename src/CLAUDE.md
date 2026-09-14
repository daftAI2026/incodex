# src/
> L2 | 父级: [../CLAUDE.md](../CLAUDE.md)

本层承载 Incodex 的 TypeScript/Bun 构建辅助、隐私取证契约与 Electron Runtime 入口；Rust 是产品 CLI 与原生变更边界，本层只生成/验证 Runtime 资产并维护其安全行为。

## 成员清单
- `build-runtime.ts`: 读取 Runtime 入口与 SVG 资产，构建可移植 `dist/` CJS、清理临时源码并写入版本/哈希清单。
- `compatibility.test.ts`: 验证 Runtime 对官方 Search 多语言 aria-label 的兼容识别，消费 `runtime/compatibility`。
- `deploy-runtime.ts`: 将已构建且受清单约束的 Runtime 资产复制到 `~/.incodex/targets` 开发热加载目录，不参与产品安装。
- `forensics.test.ts`: 覆盖会话各退出路径的证据扫描与清理契约，确保正常与异常生命周期边界可验证。
- `forensics.ts`: 创建取证会话、植入唯一提示并扫描受管目录，复用 Runtime 安全目录实现验证残留证据。
- `instance-port-lease.test.ts`: 验证基于进程身份与固定端口的实例租约、旧记录拒绝及并发所有权语义。
- `instance.test.ts`: 验证实例所有者元数据、PID 复用防护、锁恢复与单飞连接行为。
- `ipc-guard.test.ts`: 以 Electron 发送者快照验证来源、窗口、Frame 与动作响应授权边界。
- `locale.test.ts`: 验证 Runtime 文案的 locale 归一化、区域回退与中英文案约束。
- `runtime-cleanup-owner.test.ts`: 通过 Electron 生命周期测试夹具验证退出后所有者清理、诊断与安全烧毁流程。
- `runtime-late-recreation.test.ts`: 验证会话目录被晚到替换时仅凭已证明删除状态进行身份绑定清理。
- `runtime-load.test.ts`: 验证生产 Runtime 固定使用内置资产，开发热加载才可按目标身份覆盖，并检查 loader 失败开放契约。
- `runtime-main-injection.test.ts`: 验证 Electron 主进程窗口挂钩、UI 注入就绪上报与 Dock 装饰动作的跨平台行为。
- `runtime-main-session.test.ts`: 验证 Electron 主进程复制设置失败时销毁身份绑定会话，防止半初始化状态存留。
- `runtime-manifest.ts`: 从 `runtime-artifacts.json` 读取当前代资产目录，导出稳定文件名并写出 Runtime manifest。
- `runtime-session-contract.test.ts`: 以源码契约检查主进程创建/烧毁路径携带会话目录设备与 inode 身份。
- `runtime-session-process.test.ts`: 验证退出会话的 helper 进程精确匹配、静默化与首次烧毁顺序。
- `runtime-windows-main-session.test.ts`: 验证 Windows 安装 Runtime 的 locale、窗口挂钩与 guardian 生命周期桥接。
- `safe-home.test.ts`: 覆盖私有会话目录、symlink 拒绝、仅复制 `auth.json`/`config.toml` 与烧毁证明等安全契约。
- `window-kind.test.ts`: 验证主窗口、登录/OAuth 对话框与辅助窗口的分类边界。
- `runtime/`: Electron-side Runtime、浏览器注入、隔离会话、原生桥接与其测试；局部地图见 [runtime/CLAUDE.md](runtime/CLAUDE.md)。

法则: 成员完整·一行一文件·依赖方向清晰·测试先于实现·安全边界优先。

[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
