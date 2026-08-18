# 智能快照（Appshot）为什么救不回来

记录日期：2026-08-19  
对过的正式包：Codex `26.814.41407` / build `6720` / `com.openai.codex`  
官方 Developer ID：`OpenAI OpCo, LLC`，Team ID `2DC432GLL2`

这不是待办。改 asar 之后，Appshot 在现有产品约束下做不回来。以后再有人想「把 Team ID 签回去」或「中间加一道桥」，先读这篇。

---

## 1. 它是什么

界面上叫 **智能快照**。Composer 里那颗拍照 / 截屏附件，IPC 名是 `composer.appshotCapture`。

它不是 Electron 自己开摄像头。拍完要交给 Sky / Computer Use 那条链路：`SkyComputerUseClient` / `SkyComputerUseService` 连本机 unix socket，再由那边真正抓屏、回图。

用户看到的失败文案：

- 中文：`无法附加智能快照`
- 英文：`Can't be used`

Computer Use（让模型操作电脑）和 Appshot 走同一条 Sky 服务，但检查点不一样。2026-08-19 实机上，Computer Use 已经能用，Appshot 仍然失败。

---

## 2. 先排除的东西

这些都查过，都不是根因：

| 怀疑 | 实际 |
| --- | --- |
| 系统没给相机 / 录屏权限 | TCC 已经开了。失败文案也不是权限弹窗 |
| 重签丢了 `com.apple.security.device.camera` | 后来外层补回了相机、麦克风、JIT、`disable-library-validation`。权限在，Appshot 还是不行 |
| `Codex Computer Use.app` 被 `--deep` 签成 adhoc | 已经 stash / 还原。sidecar 仍是官方 Team `2DC432GLL2`。Computer Use 因此恢复了 |
| 无痕窗第二次点帽子触发 EPIPE | 是另一件事。socket 在 `destroy` 后再 `end`。修了之后不再弹 |
| 改了 bundle id，Sky 不认 | 没改。还是 `com.openai.codex` |

所以：相机 entitlement 要留，CUA sidecar 不要动，这些是对的，但只够救 Computer Use，不够救 Appshot。

---

## 3. 它实际在查什么

Sky 不是读一段配置、也不是读 entitlements 里的 Team 字符串。它用 Security.framework 查**连上来的那个进程**的真实代码签名：

- `SecCodeCopyGuestWithAttributes`
- `SecCodeCopySigningInformation`
- `kSecCodeInfoTeamIdentifier`
- `kSecCodeInfoIdentifier`

父进程判定大概是 `parentProcessIsCodex`。失败码能看到 `UNTRUSTED_PARENT` / `MISSING_PARENT`。

允许的 identifier：

```text
com.openai.codex
com.openai.codex.alpha
com.openai.codex.beta
com.openai.codex.dev
com.openai.codex.nightly
```

还不够。`Parent.coderequirement` 要求签名里带：

```text
team-identifier 2DC432GLL2
```

IPC 套接字本身也绑在这个 Team 的 Group Container 上：

```text
~/Library/Group Containers/2DC432GLL2.com.openai.sky.CUAService/IPC/computeruse.sock
```

`kSecCodeInfoTeamIdentifier` 来自**苹果签发的 Developer ID 证书**。不是 entitlements 里的 `com.apple.developer.team-identifier`，也不是自签证书 Subject 里手写的 OU。

adhoc（`codesign --sign -`）之后，`codesign -dv` 看到的是 `TeamIdentifier=not set`。identifier 可以仍是 `com.openai.codex`，Team 没了。Sky 就拒。

---

## 4. 为什么签不回去

Incodex 必须改 asar（帽子按钮、无痕窗、横幅）。asar 或 `Info.plist` 一改，包上的 OpenAI 密封签名就坏了。没有 OpenAI 的 Developer ID 私钥，没法再签出带 `2DC432GLL2` 的合法身份。

试过、确定没用的：

1. **把官方 entitlements 原样贴回去**  
   `com.apple.developer.team-identifier`、`application-identifier`、`application-groups`、`keychain-access-groups` 是绑 Team 的。adhoc 签上去，launchd / 系统不认，还可能让启动更糟。现在 `hostEntitlementsForAdhoc()` 会剥掉这些键。

2. **自签一张 OU=`2DC432GLL2` 的证书，再 `codesign --sign`**  
   `codesign` 能成功。`TeamIdentifier` 仍是 `not set`。macOS 只从苹果签发的证书抄 Team ID。自己写的 OU 不算。实验证书已从登录钥匙串删掉。

3. **只签外层，里面的 helper 继续挂官方 Team**  
   进程内的 Codex Framework / 若干 helper 和宿主 Team 不一致时，`dlopen` 直接失败，日志是 `different Team IDs`。应用起不来。所以进程内的必须跟宿主同一套 adhoc；独立 sidecar（`Codex Computer Use.app`、`SkyComputerUseClient.app`、`CUALockScreenGuardian.app`）可以保留官方签名。

4. **中间再过一道桥，让 Sky 去读另一个还带着官方 Team 的进程**  
   Sky 查的是**当前连 socket 的那个进程**的 `SecCode`，不是你递给它的字符串，也不是「附近还有一个官方 ChatGPT」。要骗过这条检查，桥的这一端必须自己就是未改包、仍由 OpenAI 签过的 `com.openai.codex`。那等于再跑一份没打补丁的正式应用，专门给 Appshot 当马甲——产品范围外，也解决不了「无痕窗自己就是调用方」这件事。

---

## 5. 这是不是不可能三角

在现有约束下是的。三件事不能同时成立：

1. 改 asar（帽子、无痕、文案，产品本身）
2. 宿主进程仍持有苹果签发的 `TeamIdentifier=2DC432GLL2`
3. 不换 bundle id、不逼用户重新登录

1 + 3 是现在的产品。2 + 3 是没打补丁的官方应用。没有 OpenAI 证书，做不到 1 + 2。

签名原则上只签必须签的：

- 进程内 helper：跟宿主一起 `--force --deep --sign -`，否则 `dlopen` 因 Team 不一致退出
- 独立 sidecar：`--deep` 前 stash，签完再 ditto 回去，让 Computer Use 继续用官方身份
- 外层再单独 `signOne`（不要 `--deep`）：补回相机 / JIT 等可保留的 entitlement，避免 `--deep` 把宿主 entitlement 摊到每个 dylib 上

这套能保住启动、钥匙串、Computer Use。保不住 Appshot。

---

## 6. 产品上怎么对待

- 不要再为 Appshot 改签名策略碰运气。根因不在 entitlements，也不在 TCC。
- 不要实现「藏一份官方 ChatGPT 当 socket 代理」。检查的是调用进程自己。
- 不要伪造 Team ID。系统不吃，也越过产品该碰的线。
- Computer Use 和 Appshot 不要捆在一起回归。前者 sidecar 在就能用，后者看宿主 Team ID。
- 用户问为什么无痕里不能智能快照：改过包的宿主不再带 OpenAI Team ID，Sky 拒绝父进程。卸载 Incodex、回到未改的官方应用，快照会恢复。

以后若官方自己改了 Sky 的父进程检查（不再要 Team ID，或接受 adhoc + 同 bundle id），再回头看。在那之前，这篇的结论不变。
