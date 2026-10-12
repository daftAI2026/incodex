# tests/native/
> L2 | 父级: ../CLAUDE.md

成员清单
CLAUDE.md: 本目录成员与调用边界。
permission-activation-policy-ax-smoke.swift: 临时权限宿主激活策略与 AX 输入回归。
permission-allow-state-c14.m: Allow 按钮自然尺寸、可用状态与输入回归。
permission-ax-keyboard-diagnostic.swift: 显式诊断权限宿主键盘与 AX；不作为产品入口。
permission-card-snapshot-smoke.swift: 单卡前景的无窗快照与布局回归。
permission-dual-rows-smoke.swift: 双权限状态、系统录屏图标与 flight/drag 回归；夹具窗口不显示。
permission-card-text-layout-smoke.swift: 本地化单卡文字换行与按钮边界回归。
permission-flight-pixels-smoke.swift: 自有视图图像的合成与圆角回归。
permission-helper-snapshot-smoke.swift: 设置 helper 的无窗快照与自然测高。
permission-host-g12-lifecycle-smoke.swift: 临时宿主生命周期；不操作官方应用。
permission-instruction-smoke.swift: 本地化语义片段与文本完整性回归。
permission-placeholder-hover-smoke.swift: 设置占位控件的 hover 与还原回归。
permission-row-component-smoke.swift: 两个独立权限行的无窗状态、动作与尺寸回归。
permission-runtime-error-ax-smoke.m: 权限 Runtime 错误呈现与 AX 回归。
permission-views-abi-smoke.m: 已发布原生库的 ObjC ABI 回归。
permission-views-layout-smoke.m: 显式临时窗口的原生布局探针。
permission-views-smoke.swift: 现有单卡、helper 与图像的无窗回归。

显式可见测试的授权不包含日常宿主操作。
无窗组件测试不申请权限，不启动捕获。

[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
