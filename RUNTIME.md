# Runtime 热更新架构

> 本文约束 Runtime 设计，不记录候选历史。
> 实现与验收须分别陈述。

## 原则

换功能实现，不重建资源所有权。

新增 Runtime 功能，先设计热更新路径。
能复用已部署协议时，不改固定启动模块。
业务更新先用冻结旧验证器执行候选。
涉及 helper 换代时，必须同时证明 Runtime ACK 与新 helper 实际执行。
不能默认把业务逻辑放进启动入口。
无法热换的代码必须说明原因与迁移边界。
禁止用清空 require.cache 冒充热更新。
禁止为 Runtime 更新退出或重开宿主。

### CLI、Runtime 与原生 helper 的更新交接

本约束同时适用于 macOS 和 Windows 的 `inc update`、`runtime` 与候选部署。
同步指可验证的兼容与正确路由，不要求所有 helper 的版本号相等。
每次影响 CLI、Runtime 或 helper 的变更都要核对两端的生产者和实际消费者：

- CLI 的安装渠道、目标版本和实际文件摘要；Runtime 的选中代际与资产校验。
- 实际登记的 helper 路径、内容身份和协议；下一次原生请求实际选中的可执行文件。
- 安装器产物必须满足消费者的同一权限与身份验证。Windows 发布前与旧 helper 启动时共用目标代际的私有 ACL、普通文件和版本验证，不能只检查最新 CLI 指针或发布进程自身。
- 兼容的固定 helper 可以保留；新业务交给已验证的目标代际。必须换代时沿已有登记/所有权事务，不覆盖不可变文件，不为更新重签宿主或重置系统授权。macOS 已获权的 Keychain helper 不因 CLI 版本变化而轮换。
- CLI 已替换、Runtime 已发布、helper 已登记、业务已执行分开报告。缺少后续证据不能称为完整生效；失败保留可诊断的部分成功或迁移边界，不用卸载重装冒充原位升级。
- Windows Runtime 内容变化不推进安装授权 epoch，避免撤销活 App 协调器捕获的授权；安装/卸载仍沿原 epoch CAS。显式同步必须同时交接同一安装登记下的预挂钩与恢复记录，包括同版本修复。持安装锁验证独立登记、helper 摘要与旧/新 Runtime，保留目标包和预留 operation ID，先预备记录再提交安装状态。中断只能续接精确已预备的内容代际；异主、未知或已撤销授权不迁移。后台 helper 的 epoch/Runtime 精确校验不放宽，不能用删除 pending、替换兼容 helper 或重开宿主完成交接。

### macOS device-key 消费者

官方模块继续创建与拥有 device key。
Incodex 保留旧 `SecKeyCreateRandomKey` 回退合同。
旧 remote-key addon 保持 SHA256 `4418cab0…2327fd`。
旧 remote-key manifest 保持 SHA256 `56fddd68…2ea081`。
旧 startup source 不接入 device-key wrapper。
adapter 使用独立内容路径和manifest。
device-key helper 与 Storage helper 保持独立登记。
adapter 不进入 Runtime 外置资产清单。
publisher 独立验证并发布 adapter 二进制和 manifest。
Runtime builder 校验 manifest 与 adapter 原始字节。
builder 将 main-actions、hot-feature 源和双 pin 定界后求 SHA-256。
injector 与 main-actions 内嵌同一个 64 字符十六进制 token。
固定 loader、main、preload、verifier 和 catalog 不变。
device-key 热候选只更换 main-actions 和 injector。
冻结 verifier 对这两个已允许资产执行原子代际提交。
动作工厂准备阶段不调用 `require` 或文件 API。
renderer 启动后发送 token 化动作名和 requestId。
请求不携带 key ID、签名数据或文件路径。
旧 actions 不识别新 token 时返回 `UNKNOWN_ACTION`。
renderer 只对 `UNKNOWN_ACTION` 重试，最多发送五次请求。
generation dispose 或 pagehide 会撤销后续重试。
UI `active` 只证明动作与界面代际提交。
它不证明 adapter 已激活或 Keychain 授权成功。
adapter 激活后只包装已缓存的官方 exports 对象。
包装只路由 `dk_osn_` 公钥查询和签名。
其他 key ID 与 API 保持原官方函数。
兼容 ABI 的 adapter 可按新内容摘要原位换绑。
相同 adapter 的新业务 token 也触发原位换绑。
相同 token 和双 pin 才返回 `ALREADY_ACTIVE`。
稳定 wrapper 只捕获 binding 并调用该 binding。
每个请求在首个 await 前捕获当前 adapter binding。
旧请求继续由旧 binding 完成，不卸载原生库。
拒绝与取消原样返回，不重试官方签名。
固定 helper 只查询精确标签的现存 P-256 私钥。
helper 不创建、删除、导出或轮换密钥。
helper 身份与系统授权保持不变。
公钥请求全程保留 300 秒机器期限。
两类请求在 helper 校验与完整 stdin 发送前使用该期限。
签名 stdin EOF 后只等回复、helper 退出或环境取消。
N-API 用专用 pthread 与 kqueue，不占 libuv 池。
TSFN 不引用事件循环；空闲 Node 可自然退出。
环境清理先取消并 join，再只 abort 未结束的 TSFN。
不得修改 Keychain ACL 或转接 generic-password Storage。
activation 与实际 public/sign 回执写入既有 `incognito.log`。
activation 回执记录闭合阶段和失败码，不记录原始异常或路径。
日志只记录 PID、activation token 摘要、adapter/helper 摘要、操作和闭合码。
日志不得记录 key ID、payload、公钥或签名。
仅真实官方操作回执能证明对应操作完成。
源码、合成测试和 publisher 输出都不证明实机授权。
RC1 的 `restartRequired` 只描述旧候选，不约束此路径。

受影响平台必须覆盖旧 CLI/helper → 新 Runtime 的升级与同版本修复，
并证明失败时旧请求/代际仍安全。平台专有的 ACL、签名与系统授权各走原有适配，
不能把一端的通过推定为另一端通过。原生兼容边界变化须补对应平台实机验收；
源码、单测和发布摘要不能替代该回执。

Windows Runtime 状态提交报错后，仅在锁内读回仍为原状态时恢复原 pending，避免旧协调器失去预留 operation ID；读回未知、已发布或回退失败时保留证据并报错，不宣称跨文件崩溃原子性。若旧协调器已经完成目标重绑，显式同步须验证当前独立登记、同 helper 目标及安装 registration ID 与预留 operation ID 相等，提交选中 Runtime 后才退休已完成的预挂钩记录。异操作或仍有 repair 的不确定记录保留并拒绝，不关闭或撤销当前目标。

Windows Store 可跳过先前暂存的目标包。同一完整来源授权的新事件只能把预挂钩移向系统包身份 API 解析出的更高版本；同目标保留 operation ID，迟到旧事件不能倒退。调用方持安装锁验证来源、helper 与 Runtime，在新旧目标均无运行进程且旧目标登记可确认时，复用既有取消和 operation CAS 后预备新目标。正常恢复也必须在提交新来源之前收敛所属过期目标；已消费的预挂钩仅由匹配目标和预留 registration ID 的安装退休。未知、异主或取消失败保留证据。此原生 helper 行为须显式安装迁移；既有异操作残留不能靠相同 helper/Runtime 冒认归属，迁移烟测不等于下一次真实 Store 首窗验收。

### 受控退出诊断

退出诊断默认关闭，不属于热更新能力。
仅显式旗标观察官方 sky.node 原判定。
不主动载入原生模块，不重建 sender 判定。
原返回、异常和导出对象身份保持不变。
getter-attached 只证明原位包装，不调用判定。
enabled 只证明诊断入口已启用。
只记录闭合码、PID、布尔值与日志时间。
事件派发后值只描述同步监听器的结果。
它不证明后续异步确认已结束。
观察器内嵌外置 main，不进入纯动作工厂。
main 字节变化保留 restartRequired 边界。
仅在明确批准的受控复现中正常重开迁移。
不修改 Loader、原生资产或官方 App。
不把该迁移签作同进程热激活。
它不阻止退出，也不自动复活用户关闭的 App。

### 原生更新诊断

INCODEX_DIAGNOSTIC_UPDATE_TRACE=1 才记录回执。
未设置旗标时，不写诊断日志。
排队标记按独立业务合同装配委托。
只观察已匹配 updater 的官方原回调。
保持原 self、selector、更新对象、block 和返回。
原异常对象继续向调用方抛出。
官方委托独占 immediateInstallationBlock。
不复制、调用或持有该 block。
Coordinator 只记录原分支、helper PID 和退出码。
pending-absent 表示未接管，不证明文件不存在。
registration-missing 表示字段不可用，不证明缺文件。
helper 启动成功与退出成功分别判定。
relaunch-host 只记录策略，不证明实际重开。
共享头将产品回执写入系统日志。
stderr 接收端仅由测试编译宏启用。
日志不含路径、URL、nonce、账户或聊天。
不增加拒绝、等待、重开或 pending schema。
原生字节变化另立迁移，不直接热发布。
诊断通过不等于更新恢复或权限连续性通过。

发布不等于生效。
磁盘指针、主服务代际和界面确认是三种证据。
不能用文件哈希替代运行中确认。
不能用一个功能成功替代完整代际验收。

### 官方更新的活宿主续接

新宿主已运行时，不修改或退出它。
复用原 Coordinator 与 pending owner。
启动前检查精确路径，helper用闭集busy结果续接。
锁内保存 relaunchHost=false，再等内核退出事件。
退出后重验 owner、登记 helper 与恢复事务。
仍有同路径宿主时继续等，不轮询。
取消或换 handoff 后不恢复。
事务前PID探针明确命中才返回内部75。
探针、签名、事务与回滚错误不返回75。
回调前宿主已退出时，立即重验最多一次。
普通失败不因错误文字或宿主存在而重试。
恢复后不重开用户正常关闭的窗口。
这覆盖事务前busy续接，不保证首个窗口已注入。
原生变更要求迁移，隔离不代真实升级。

### 用户验收定义

运行中的 App 执行 Runtime 更新后，新增功能立即可用。
验收前后保留同一宿主进程和页面。
不以退出重开、刷新页面或重新安装完成换代。
每项新增功能都须验证自己的生产调用链。
已有动作的 A→B→A 只证明该动作的换代机制。
它不证明新增权限探针、权限卡或续接流程已生效。
冷启动新版再热换测试动作，不能替代旧版升级验收。
发布了新 CLI，也须证明当前宿主能使用新权限流程。

## 唯一架构

```text
Rust publisher -> verified immutable release -> current.json
                                                 |
                              stable Runtime host / verifier
                                                 |
                                      active generation
                                       /             \
                               main services      renderer hooks
                                       \             /
                                        exact acknowledgements
```

稳定宿主持有进程资源。
功能代际提供可替换的实现。
平台适配器提供授权后的系统能力。
三者不互相夺取所有权。

### 稳定宿主

宿主唯一持有 IPC、窗口身份和会话所有权。
宿主唯一持有 watcher、timer 和 native handle。
授权、进程身份和私有文件校验不得绕过。
现有会话与进行中的任务不得因换代消失。
权限 watcher 不随策略换代重新创建。
nonce 和安装代际也不随换代重置。
权限请求遵循已安装 responder 的传输协议。
临时请求编码不应迫使用户更换主入口。
通知缺失仍保持 unknown，不能伪造授权。
一次请求的重试不得延长其原有截止时间。

稳定不等于把所有业务永远固定。
稳定宿主只保留资源、授权和分派原语。
遗留业务须逐项迁移到功能代际。

### 只读会话目录销毁

会话销毁先验证 root、owner 与私有父目录。
Electron 父进程先确认 child 退出和 helper 静止。
Electron 仅在此退出清理链适配目录权限。
其活 owner 直接销毁与同步孤儿扫描不适配权限。
Rust 同步孤儿扫描也保留原有删除能力。
owner 已退出不证明其遗留 helper 已停止。
只适配同一会话内的只读目录，不修改文件权限。
链接由原删除器移除，不跟随其目标。
目录权限调整失败时，保留会话并报告失败。
父进程沿用原退出快照和 inode/device 重试。
不将 owner 缺失推广为普通销毁的授权证据。
JS 路径校验依赖既有 helper 静止合同。
不声明同 UID 恶意并发写入的隔离保证。
该原语在启动时由 safe-home 模块固定加载。
其字节变化必须保留 restartRequired 判定。
新进程加载修复不等于旧进程热激活成功。
不清 require.cache，不扩大 renderer 热换白名单。
正常退出重开迁移另行确认，不为发版强退宿主。

### 功能代际

工厂只构造函数与不可变数据，不执行 IO。
动作、文案和界面应属于功能代际。
权限呈现规则进入已验证的动作代际。
真实探针与授权结论仍由稳定宿主拥有。
功能通过明确的宿主能力调用 IO。
工厂不能注册第二个 IPC handler 或 watcher。
工厂不能创建第二套会话与清理器。
权限策略只接收状态值，不接收 Electron 权限对象。
宿主保留真实读数，拒绝策略伪造 granted。

验证摘要与接口形状不构成代码沙箱。
候选是受审查的 Incodex 代码，不是第三方插件。
工厂纯度须由源码审查与副作用回归共同约束。

每个请求在第一次调用或 await 前捕获代际。
旧请求用旧代际完成，新请求使用新代际。
纯代际通过请求引用自然保留，无需第二套 owner。
新增代际资源须提供幂等 dispose 与排空测试。
dispose 只能释放代际资源，不能烧毁用户会话。

### 平台适配器

共享层定义功能协议与换代事务。
macOS 和 Windows 当前消费兼容的动作协议。
macOS 新宿主显式声明权限呈现能力。
Windows 不因共享工厂获得已验收的权限流程。
各平台保留自己的进程、签名与启动适配器。
不因共享协议宣称另一平台已通过真实验收。
不新增服务、LaunchAgent 或常驻修复器。

### macOS可选普通宿主入口

`inc launch`不改包、不重签官方App。
薄启动App转调同一CLI，不拥有会话实现。
先原子发布Finder入口，再启动宿主。
外部同名包冲突时，不打开官方App。
界面ACK之后不再执行可能失败的图标写入。
稳定CLI入口保留安装渠道的链接路径。
已安装包读取marker与提交记录后用LaunchServices。
正常打开不运行安装级验签或快照扫描。
该识别不能授权安装、恢复或Runtime发布。
原Loader、IPC、Runtime继续独占功能。
不增加CDP或controller，不发布Runtime。
无需先卸载，不改变既有权限和签名。
Windows沿用install与官方图标，不加薄启动App。

以下CDP合同只适用于未安装的原版包。
CDP绑定复用共享动作工厂与Rust `open`。
原生`open`继续独占私密进程和销毁流程。
正常宿主退出不强杀独立私密窗口。
Finder入口短退，CLI仅随普通App存活。
重开只通过已捕获的CDP连接激活普通页。
发现请求暂时失败或空快照不撤销活连接和请求。
沿原有节奏重试发现，命令仍逐连接证明owner。
不以LaunchServices兜底重开未注入App。
重复帽子请求复用原生open的stdio与owner。
CONTROL_READY只证明控制协议可用。
OPENED与置前RAISED是不同回执。
RAISED要求页面与系统激活均已确认。
待回执不阻塞普通宿主的监督循环。
失败保留私密窗口，不新增会话代理。
此入口不提供Dock菜单或安装权限responder。
原生open以只读WindowServer观察关闭。
产品CLI不请求AX、Automation或录屏权限。
官方宿主的业务授权仍属于官方宿主。

新文档脚本注册属于DevTools session。
最终就绪后，原worker持有该session。
断线后用新session重新注册共享payload。
恢复时钟重启，不重复模式fallback或OPENED。
Blocked重试只修当前文档并读取轻量标记。
短连接不拥有跨刷新注册。
退出取消原监督资源并等待其结束。
Windows installed入口保留既有截止策略。Windows私密open把成功注入的原socket
交接给现有窗口监督worker，使Chromium继续拥有new-document注册；不增加线程、
定时重放payload或重复OPENED。原Job归属、target关闭和mask检查不放宽；注册连接
丢失沿既有连续失败限额处理。此原生CLI变化需要新CLI消费，不能冒称JS热更新完成。

适配器先订阅目录事件，再读取完整验证代际。
renderer与动作均以精确摘要确认后提交。
新文档使旧ACK失效，必须重新确认。
未确认换代最多退避重试三次。
原生迁移不在空闲循环反复尝试。
launch子请求在启动准备前后复验CLI与所选Runtime。
旧CLI不得用内嵌资产倒退已选Runtime。
命令路径和socket夹具不代替真实App验收。
双击启动、窗口重开和同进程热换须实测。

## 唯一换代事务

`prepare → acknowledge → commit → retire`

1. prepare：验证完整清单、摘要、路径和功能协议。
   加载无副作用工厂。旧代际继续服务。
2. acknowledge：验证候选服务能力与精确界面确认。
   包括换代期间新建的合格窗口。
3. commit：重验当前选择，统一切换功能代际指针。
   新增同代能力时，不得单独提前提交。
4. retire：排空旧请求，再释放旧代际资源。

校验失败时，不执行候选。
任一确认失败时，保留旧服务并逆序回滚界面。
回滚失败须保留不确定状态，不报告成功。
连续发布只保留一个换代事务，随后读取最新选择。
日志失败不能改变换代结果。

### 界面过渡期

现有注入器在确认前已替换单窗界面。
因此它不是跨窗口、跨进程的全局原子提交。
第一窗运行 B、第二窗待确认时，主服务仍是 A。
普通请求继续执行，不暂停用户任务。

候选必须兼容现有动作协议的输入与输出。
暂存界面不得依赖尚未提交的新操作。
新增操作或破坏协议时，先实现界面提交门控。
不能只增加协议数字或绕过版本检查。
失败时回滚每个尝试过的窗口，包括丢失确认的窗口。
主服务指针只在全部确认后切换。

## 更新分类

| 对象 | 路径与边界 |
| --- | --- |
| 动作与界面 | 走既有统一功能代际事务 |
| 权限解释 | 先复用已安装协议；迁移须明确首次接口边界 |
| 文案与权限卡呈现 | 走功能代际；保留正在进行的授权流程 |
| 新增业务能力 | 扩展功能协议，不添加启动期特例 |
| 尚未迁移的主进程业务 | 明确待迁移，不冒充完整热更新 |
| 已打开的无痕窗口 | 保留隐私契约；扩展前须通过独立真实验收 |
| 短生命周期原生工具 | 下一次调用选择已验证的新资产 |
| 已加载的原生库 | 使用稳定 ABI；库换代须单独设计并验收 |
| ASAR loader 与启动桥 | 属于安装边界，不能由外置文件发布替换 |
| 签名、Keychain 与 TCC 身份 | 保留身份；更新不是轮换身份或重置权限 |

业务热更新的目标是全部覆盖，不是不断扩大文件白名单。
不能把不可安全卸载的二进制标成“支持热更新”。

## 当前覆盖范围

既有事务覆盖普通 macOS 窗口的界面与纯动作。
新主控制器允许注入器、纯动作和权限文案变化。
权限文案必须与动作同一清单验证并冻结。
当前验证器保持已部署字节和校验边界。
短命 Rust 权限业务由已登记的新 CLI/helper 执行。
它复用现有 responder，不替换 Electron 启动模块。
原生卡资产保持同一已验证字节与 ABI。
helper 仍须校验登记、摘要、ABI 与源码绑定。
固定资产变化仍拒绝热换，不以新白名单追溯升级旧验证器。
未来原生资产变化须单独实现并验收热路径。
旧控制器仍只允许原有资产变化。
权限 responder 与其他固定资产随进程启动。
工厂权限呈现通过隔离代际回归。
这不证明原生产品卡消费了该 JS 功能接口。
产品权限卡由原生 CLI 独占。
进行中的卡片继续使用固定的验证资产。
旧 AX 请求只读交接到已绑定的原生任务。
新 Runtime 不接入旧 JS 弹卡或 reset 路径。
交接只使用已 active 且仍 selected 的清单。
完整提交新代际后重试旧 pending 请求。
未知 owner 不按旧协议接管。
Rust 再验 epoch、helper、Runtime 与签名。
CLI marker 只在三类合法 resumeReason 与 pending/awaiting-user 状态下唤醒 helper。
三类 reason 是 post-update、screen-authorization 和 permission-setup。
Runtime 把原 requestId 绑定到 Task；该字段不是授权证明。
granted、deferred、error、未知 owner 与损坏 marker 均拒绝续接。
一个 controller watcher 观察 marker 原子发布；flight 结束后重读最新值。
宿主生命周期事件可重查同一 pending；同签名 rename 不会自动重放。
controller dispose 关闭 watcher；Runtime 不扫描目录，也不增加服务。
JS 兼容 controller 仅保留冻结回归边界。
CLI 引导是短生命周期进程，下一次调用使用新 CLI。
修正 CLI 请求编码无需替换现有 responder。
同内容的新清单确认只证明发布与协调器路径。
功能热替换须另外证明行为确实变化。
没有同进程验收，不报告“全部热更新完成”。

### 权限卡触发边界

权限检查与展示权限卡是两步。
新 Loader 的首次安装先检查，缺项才引导。
旧 Loader 保留运行中显式引导。
更新后检查现有权限；确认缺项才展示引导。
全部已授权时不展示；只有 unknown 时保持 pending。
不能因 App 持续运行、窗口聚焦或探针超时新建引导。
生命周期事件只续接已有的未完成请求。
同一请求的显式跳过是终态，不能被自动重试覆盖。
短命原生程序指按需运行，不指频繁运行。

### 恢复原版后的人工引导

官方原版不含 Incodex 录屏探针。
原版录屏读数保持 unknown，不推断 denied。
仅此入口启用 manual 验证策略。
AppKit copy 回执只证明设置窗口接收拖放。
它不证明权限行或开关，也不授予权限。
CLI 只在本轮 Screen 设置已打开后接受回执。
AX 已授权时结束引导，返回 AwaitingEvidence。
AX 未授权时收回辅助卡，保留 AX 操作。
后续 AX 获准仍不把 Screen 改成 granted。
安装和更新入口拒绝人工回执。
它们仍须当前宿主的真实录屏探针。
新短命卡使用新发布的验证资产。
进行中的旧卡不原位替换原生字节。

### 安装前置授权

安装 CLI 仍是唯一引导 owner。
先安装、签名，再检查最终 App。
通过 LaunchServices 隐藏启动同一 App。
不得直接继承终端的权限责任域。
显式内部探针不载入官方 main。
普通启动继续同步载入官方 main。
探针复用既有 nonce、PID 与启动代际协议。
CLI 完成同主体快照后，退休自己的请求。
探针收到退休信号后正常退出。
旧探针退出后，才呈现原生卡或打开设置。
安装上下文只取消工作窗口呈现门。
默认呈现门、双行视图与动画不变。
明确缺项才出卡；未知不冒充拒绝。
闲置卡复用快照，不循环冷启 Electron。
当前 owner 用 notify_check 检查变化提示。
变化提示不是授权事实，只触发新探针。
Allow 与 Retry 保留新鲜复查路径。
机器探针有时限；人工授权没有倒计时。
先记录终态，再正常打开工作 App。
Skip、关闭与传输丢失保留不同结果。
旧 Loader 不进入新探针分支。
旧安装识别继续接受冻结历史字节。
这次 Loader 与原生 host 变化需显式安装。
它不是运行中功能热激活。
默认安装共用一个权限收尾编排器。
隔离整链复用真实安装变更与终态 CAS。
探针退休、退出与最后启动共用生产内核。
OS 命名空间和权限替身必须显式标注。
无窗 Presenter 只证明原生协议，不证明 UI。
真实责任域、双授权与免重开行为仍须实测。
inc update 的不自动启动 App 合同不变。

```text
install CLI -> LaunchServices -> 最终 App 探针 -> 既有 nonce 回执
install CLI -> 原生双行卡 -> 系统设置
权限变化提示 -> 同一 CLI -> 新探针
CLI 终态落盘 -> 正常工作 App
```

发布后按安装身份与 Runtime 清单摘要准入。
同代已跳过的请求不再创建。
每次成功更新重读上次 granted 的当前权限。
新鲜双 granted 不出卡；确认缺项才出卡。
未完流程复用原 requestId 与 resumeReason。
新代际可重新检查；显式授权入口仍可重新进入。
显式双权限入口仅为 `incodex perm`。
两项权限分别保留显式 Allow 的一次 reset。
录屏只在持锁复验明确未获准时 reset ScreenCapture。
已获准或未知不 reset；Retry 只重开设置。
先保存录屏续接意图，再获取 reset 锁。
意图发布自身使用同一 App 短锁，不在锁内重入。
reset 不等于授权，仍等待当前主体 fresh 双 granted。

已完成 marker 不是当前授权证明。
修改原生权限业务时，同时更新 Runtime 来源清单。
再构建配套 CLI，不能沿用旧清单掩盖业务换代。
宿主实际使用权限时可复用引导，但先证明触发入口。
当前没有接管 CUA 的权限调用或替换其系统弹窗。

### 权限诊断边界

复用有界 coordinator.log，不新增监控程序。
任务日志绑定 worker PID、来源与已验证资产摘要。
探针日志绑定实际主体 PID、启动身份与状态枚举。
仅主体或状态变化产生探针日志，不记录每次轮询。
闭集转场区分无卡预检、卡片 ready、用户跳过与机器中断。
card-ready 是协议确认，不代替用户目视验收。
不记录原始安装标识、请求标识、nonce 或错误正文。
日志失败不能改变权限结论或引导生命周期。

旧进程不会追溯获得新控制器接口。
首次引入接口须等待正常的进程生命周期。
新宿主的协议不能追溯装入旧控制器。
首次迁移与后续同 PID 热替换须分别验收。
不能诱导退出、重注册主入口或绕过校验来迁移。

### 资源与功能迁移表

本表区分当前所有者与目标，不宣布全部覆盖。

| 局部结构 | 当前所有者 | 功能目标与验收边界 |
| --- | --- | --- |
| `loader`、完整清单验证 | ASAR 与启动加载器 | 保留最小启动和验证边界；修改不得冒充业务热更新 |
| `main` 的 IPC、窗口身份、发布 watcher | 稳定主进程 | 只保留授权、所有权和分派；每个入口读取已提交代际 |
| `main-actions` | 已验证功能代际 | 同次提交动作、权限呈现与界面；旧请求保留捕获的对象 |
| 录屏 responder 与授权事实 | 稳定 `main` | 复用现有 UUID 请求；不得重复 watcher 或伪造 granted |
| 注入器、头像遮罩、Banner | renderer 代际 | 普通窗口逐窗确认；已打开的私密窗口仍须独立验收 |
| preload 与桥接 | 当前 document 的桥接对象 | 固定传输协议；功能在代际内；不能假设新 preload 追溯执行 |
| Dock 与状态菜单 | 已创建的菜单 controller | 保留原生对象；菜单方案进入代际；回调不固定旧动作 |
| 权限文案、方向与缺失解释 | 主功能代际或本次 CLI 的固定资产 | 新请求使用新代际；保留进行中引导的请求与窗口 |
| 权限卡动画与原生窗口 | 原生 presenter 与 host | 保留窗口及原生 ABI；代际只提供呈现与状态转换规则 |
| 隔离启动方案 | `main` 的启动闭包 | 迁移参数方案；单飞、child owner 与会话清理仍属稳定宿主 |
| 会话、租约、退出回收 | safe-home、instance、owner 与生命周期模块 | 保留稳定安全原语；隐私协议变更单独验收，不复制第三套清理器 |
| 官方更新交接与密钥兼容 | 启动时加载的更新模块与原生适配器 | 保留安装 epoch、签名与授权；更新能力不能轮换身份 |
| Rust CLI 与原生引导进程 | 本次 CLI 调用的已验证资产 | 以后调用可选新资产；进行中的引导继续使用原资产 |

遗留 `main` 同时包含编排与业务，须按此边界拆分。
不能为追求文件行数重新注册入口或打断会话。

### 迁移顺序

1. 优先复用现有主控制器与传输协议。
   用生产请求到冻结 responder 的组合回归锁住互操作。
   不能为临时编码问题增加启动期接口。
2. 迁移权限呈现数据与菜单方案。
   验证已完成行保留、设置往返和进行中引导不重建。
3. 新增主服务入口时，先明确首次生命周期边界。
   再迁移权限策略、启动参数与可替换业务规则。
   验证旧请求完成、新请求换代和 owner 数量不增长。
4. 扩展已打开私密窗口与 Windows 的对应适配器。
   分别验证遮罩、关闭回收、桥接所有权与真实生命周期。

每次迁移独立可用，不等待其余迁移才能安全运行。
未迁移对象继续明确报告边界，不能扩大支持声明。

## macOS 排队更新的单次续接

Sparkle的NO不表示拒绝安装。
正常返回NO或YES后，适配器标记安装已排队。
标记复用pending锁与已捕获的交接身份。
它不调用、复制或持有官方安装block。
业务标记不依赖诊断开关。
自有读写异常先释放锁，再放行原官方结果。

普通关闭宿主时，保留sourceBuild与relaunchHost=false。
旧AppKit Coordinator启动同二进制console后退出。
Sparkle继续拥有下载、换包和安装事务。
console不创建NSApplication，不进入退出目标枚举。
它先等旧owner退出，再锁内接管同一handoff。
它先观察宿主父目录vnode，再读当前build。
不同build出现后，只沿原登记helper恢复一次。
180秒只限制等换包，不限制人的授权或helper。
没有新包时，不运行helper，不重开宿主。
成功后清除自有pending并结束console。
写状态或启动失败仍放行官方换包，不报恢复成功。

显式Sparkle重开沿既有AppKit路径。
普通关闭的false重开策略跨owner接力保留。
两条路径共用owner-CAS与登记helper，不增加常驻服务。
原生映像仍需显式迁移，文件发布不证明热激活。

## 更新后的权限检查

Runtime 发布与官方恢复共用 CLI 后置步骤。
更新成功后提交一次有界权限预检。
预检最多 30 秒，不是用户完成授权的期限。
这不是主服务代际提交的确认。
旧 responder 的 UUID 协议继续作为状态通道。

任务只接受已登记的内容寻址 CLI。
任务绑定安装 epoch、helper 和 Runtime 摘要。
运行前验证 COMMITTED、签名和当前主进程。
任务不打开、退出或重签官方 App。
需要重开时，官方恢复任务等Coordinator启动宿主。
普通关闭策略不启动宿主，不制造权限读数。
没有运行中宿主时，不伪造权限或自动开 App。

真实双项 granted 时，不展示卡片。
确认至少一项缺失时，复用现有双行卡。
仅有 unknown 时，有界等待后保持 pending。
窗口就绪后再读取，避免已获权用户闪卡。
更新检查不执行 reset 或打开系统设置。
只有卡片内明确的用户选择能启动既有操作。
正式卡片不设五分钟选择或两分钟授权倒计时。
卡片等待完成、跳过、关闭或传输结束。
系统命令、探针往返和原生就绪仍各自有界。
有限人机期限仅供隔离测试，不进入产品配置。

同一 App 的卡片所有权锁阻止重复引导。
等待用户时不占用 App 危险事务锁。
请求写入、终态写入和分项 reset 各用短锁。
锁内复验目标与权限，再执行对应服务 reset。
重复更新不创建并行卡或第二个 watcher。
进行中的卡保留请求、文案与原生资产。
新 CLI 或 Runtime 发布不重建该卡。
固定原生资产只读验证，不倒退 current.json。
显式引导也不得借资产查找发布旧 Runtime。
资产缺失时，要求当前 CLI 完成显式发布。
安装 epoch 或签名变化立即拒绝旧任务。
主进程重启不撤销同身份用户的授权流程。
每份快照绑定读取前后的 PID 与启动身份。
换代或暂缺进程时，丢弃整份读数为 unknown。
新进程就绪后重新读取，不沿用旧 grant。

签名连续性保留 TCC 识别身份，不伪造授权。
后续更新必须重读权限，不沿用 marker 结论。
首次授权与真实更新连续性分别验收。
隔离状态机通过不等于真实 TCC 已授予。


## 开发与验收门

- 先写失败回归，再修改实现。
- 记录功能输入、代际协议、资源所有者和退休方法。
- 验证 A 到 B、B 到 C 与 A 到 B 失败回滚。
- 验证确认前仍用 A，提交后新请求使用 B。
- 验证 A 的进行中请求不被 B 中断。
- 验证 IPC、watcher、窗口和 owner 数量不增长。
- 验证真实生产器与仍在支持的冻结读端互操作。
- 验证坏摘要、旧进程、过期请求与替换候选被拒绝。
- 验证权限策略不能绕过宿主身份校验。
- 重建 dist，并运行共享与平台回归。
- 真实验收绑定同一 PID、启动身份和候选摘要。
- 无宿主退出、页面重载、重签或权限重置才算热更新。
- 验证安装、Runtime 刷新与官方更新恢复共用该权限协议。
- 签名连续性、探针互操作与实际授权是独立证据。

`INCODEX_DEV_HOT=1` 是开发覆盖，不是生产验收证据。
候选历史、运行回执与工作日志只进入私人文档。

## 设计依据

复用 `incodex-runtime-load.cts` 的完整清单验证与回滚。
复用 `incodex-main-actions.cts` 的无副作用工厂。
复用 `renderer-generation.ts` 的精确界面确认。
不引入通用插件框架或额外运行时。

Node 的模块缓存不撤销旧闭包或资源。
原生 addon 也不能靠删除缓存重新加载。
见 [Node CommonJS 文档](https://nodejs.org/api/modules.html#requirecache)。

Vite 明确区分接受更新、释放副作用和跨代数据。
这里采用同一职责划分，不引入开发 HMR 服务。
见 [Vite HMR API](https://vite.dev/guide/api-hmr)。

Vite 源码分开保存代际回调、释放函数和跨代数据。
本项目取其所有者分离，不复制开发重载路径。
见 [Vite HMR 实现](https://github.com/vitejs/vite/blob/main/packages/vite/src/shared/hmr.ts)。

Webpack 源码将检查、准备、释放与应用分成显式状态。
本项目取其阶段门，不引入模块图或通用插件框架。
见 [Webpack HMR 实现](https://github.com/webpack/webpack/blob/main/lib/hmr/HotModuleReplacement.runtime.js)。

[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
