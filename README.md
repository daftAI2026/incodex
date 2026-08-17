# Incodex

Incognito + Codex。给本机 Codex 桌面端加一个无痕开关。终端里一条命令，装完重启官方应用，搜索按钮左边出现帽子墨镜图标。再点一下，侧栏对话藏起来。

没有安装界面。默认不会改你正在用的 `/Applications/ChatGPT.app`，除非你显式加上 `--live`。

## 命令

```bash
cd /Users/luo/Desktop/ClaudeCode/web/incodex
bun install
bun src/cli.ts install --clone    # 只补丁一份副本，安全
bun src/cli.ts status
bun src/cli.ts install --live     # 备份后改官方 app；用完可卸载
bun src/cli.ts uninstall --live   # 从 ~/.incodex/backup 还原
```

侧栏按钮图形是 [`assets/hat-glasses.svg`](assets/hat-glasses.svg)，按官方搜索按钮的 Lucide 规格绘制。这不是项目图标。
