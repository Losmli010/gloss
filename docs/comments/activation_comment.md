# activation.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），文件内不留指向本文件的指针。

## 整体方案

- 2026-10-12 背景：Chromium 系应用（Chrome/Electron/CEF）默认不建无障碍
  树，自建 AX 客户端不在其辅助技术检测名单里，AX 读选区在这类应用上恒
  失败，每个手势都落到剪贴板兜底（注入按键、写剪贴板、恢复原内容）。
  运行日志实证（~/.gloss/logs，2026-10-12）：Chrome/VSCode/zcode 里 AX
  读选区全空、全部手势走兜底，且兜底的恢复写回曾把在途文本探测顶掉。
  业界通行解法是写 `AXManualAccessibility`（Electron 与 Chrome 现行内核）
  / `AXEnhancedUserInterface`（老内核）开关属性，宿主收到即异步建树。
- 2026-10-12 本机证据（检测判据的出处）：`Google Chrome.app` 的
  Contents/Frameworks 下是 `Google Chrome Framework.framework`，
  `Visual Studio Code.app` 与本宿主应用是 `Electron Framework.framework`，
  `Safari.app` 的 Frameworks 为空——「品牌前缀 + 空格 + Framework」形态
  即 Chromium 家族的包内容签名。Brave 是 `Brave Browser Framework`
  （Chromium 对框架名做品牌替换的又一例），CEF 固定
  `Chromium Embedded Framework.framework`。

## 检测判据（`is_chromium_family`）

- **为什么用包内容证据而不是 bundle id 名单**：名单要持续维护（Electron
  应用数以千计），漏一个就静默失效；框架目录形态是内核自带的客观证据，
  零维护。误报（第三方框架恰好叫 `* Framework.framework`）的代价仅是一
  次无害的属性写入——非 Chromium 宿主对未知 AX 属性返回属性不支持错误并
  忽略；WebKit/Gecko 宿主没有这一形态，不会被误伤。
- **为什么后缀要求带前导空格**：裸 `Framework.framework` 不是任何已知的
  Chromium 内核框架名，加上空格把「名字恰好以 Framework 结尾」的第三方
  框架挡在门外。

## 一次会话一次（`Ledger`）

- **为什么确定性结论按 pid 只评估一次**：注入本身幂等，但重复注入只会
  反复打扰宿主而不会让建树更快；非 Chromium 宿主也按 pid 记账（评估过
  即不再碰文件系统），把激活路径的单手势开销收敛为每个新应用一次。
- **为什么注入瞬时失败不记账**（评审收敛 2026-10-12）：`kAXErrorCannotComplete`
  这类瞬时失败恰恰是「宿主当时忙/未响应」的常见码，一票锁死会把真实
  Chromium 宿主锁在激活门外整个会话（后果只是错失优化、落回剪贴板兜
  底，无正确性风险，但白白放弃收益）；不记账则下一手势自然
  重试，最坏代价是误报宿主（检测假阳性）时每个手势多两次被即时拒绝的
  属性写入——有界且无害，两害相权取重试。
- **为什么按 pid 而不是按 bundle id**：应用退出重开后 pid 变化，新进程
  的树是新的（未建），按 pid 记账天然在新进程上重新评估；按 bundle id
  记账会把「重启后的应用」误判成已激活。
- **宿主识别失败的记账**：无包、目录读不出等按「非 Chromium 家族」记账
  收口——运行中应用的包路径不变，重试不会改变识别结论；目录读的瞬态
  错误概率可忽略，不值得为此引入区分。
- **状态归属**：记账表在事件线程的组合读取器里（`CompositeReader` 随
  `acquire_command_handler` 闭包跨手势复用），单线程顺序消费，无锁不
  共享——与「通道②在同一线程顺序消费」的线程模型一致。

## 前台应用定位（`front_app`）

- **为什么走 AX 的 `AXFocusedApplication` 而不是 NSWorkspace**：焦点应用
  属性由系统 AX 运行时供给，宿主建不建树都能拿到（这正是唤醒路径可用的
  前提），且与应用元素同源——拿到的元素直接用于写入开关属性，不需要再按
  pid 反查元素；NSWorkspace 属于 AppKit 主线程语义，事件线程上不碰。
- **消息超时**：systemwide 的全局 1s 消息超时由 AX 读取路径
  （`AX_MESSAGE_TIMEOUT_SECS`）设置并对本进程持续生效，本模块的跨进程
  调用同受其上界约束，不会无限阻塞事件线程。

## 注入内容（`enable_manual_accessibility`）

- **为什么两个属性都写**：`AXManualAccessibility` 是 Electron 与现行
  Chrome 内核识别的开关，老版 Chromium 只认 `AXEnhancedUserInterface`；
  一次会话只注入一回，两发写换两代内核的覆盖面。任一成功即视为生效
  （不同内核对不认识的那个返回属性不支持）。
- **写入失败交给下一手势重试**（2026-10-12 语义更新，与「一次会话一次
  （Ledger）」节同源）：`kAXErrorCannotComplete` 这类瞬时失败恰是「宿主
  当时忙/未响应」，不记账、下一手势自然重试；误报宿主的重试代价是每手
  势两次被即时拒绝的属性写入（属性不支持错误即时返回、不耗超时），
  有界且无害。

## 边界

- **不管权限引导**：本模块只在天书已授权（AX 读路径先查 trusted）后才会
  被问到；未授权的引导属设置页路径，与 accessibility.rs 的分工一致。
- **不做真机单测**：`wake` 的端到端行为需要真实 Chromium 前台应用，属
  L4 人工测试（划词 Chrome 内文本，观察日志出现 manual accessibility
  enabled 且不再走剪贴板兜底）；单测只覆盖文件级纯逻辑
  （`is_chromium_family`、`first_assessment`）与 composite 的闸门时序。

## 唤醒常客记账（2026-10-16）

- rescued 集与 settled 分立：settle 管「不再评估」（确定性结论），rescued
  管「允许重注入 + 空读跳让渡」。非 Chromium 宿主与常客外的已评估宿主
  行为不变（重注入不放行，避免反复打扰）。
- **明示接受**：wake() 返回 true 即记账（乐观）——若该轮补读最终落兜底，
  常客标记仍生效；代价只是该宿主下次空读走激活通道而非让渡环，两条路
  都有确认写入/激活预算把关。

## 符号契约（自代码注释迁入）

- `Ledger`：评估记账（纯逻辑，单测覆盖）：确定性结论按 pid 登记，未登记的 pid 待评估——瞬时失败不登记即下一手势自然重试。`rescued` 另记「注入唤醒写成功」的 pid：这类宿主的树会缺席（宿主自行收回或尚未建好），空读等让渡等不来内容，下一手势直接唤醒才是解法。
  - `is_pending`：该 pid 是否待评估。
  - `settle`：登记确定性结论：此后该 pid 本会话不再评估。
  - `mark_rescued`：登记唤醒常客：注入写成功的 pid（乐观记账——宿主认了这个开关，空读时树缺席的概率远高于等让渡能等来内容），此后空读跳过让渡。
  - `is_rescued`：该 pid 是否唤醒常客。
- `is_chromium_family`：Chromium/Electron 家族检测（纯逻辑，单测覆盖）：框架目录条目里存在名字以「空格 + Framework.framework」结尾的框架包。Chromium 内核框架按宿主品牌重命名（Chrome 是 `Google Chrome Framework.framework`、Brave 是 `Brave Browser Framework.framework`），Electron 系固定 `Electron Framework.framework`，CEF 固定 `Chromium Embedded Framework.framework`，共同形态即「品牌前缀 + 空格 + Framework」。误报（第三方框架恰好同名）的代价仅是一次无害的属性写入——非 Chromium 宿主对未知 AX 属性返回属性不支持错误并忽略；WebKit/Gecko 宿主没有这一形态，不会被误伤。
- `imp::AX_ERROR_SUCCESS`：AXError.h 的 kAXErrorSuccess。
- `imp::AxActivator`：AX 通道激活器：会话内按 pid 记账的 Chromium 宿主唤醒。只在平台事件线程上使用（由组合读取器持有），无锁不共享。
- `imp::AxActivator::responds_to_wake`：前台应用是否为「唤醒常客」：上一手势注入唤醒写成功的 pid。常客的空读跳过让渡环直接进激活补读——树缺席时等再久也读不到。只做查询、无副作用。调用方保证：在平台事件线程上调用。
- `imp::AxActivator::wake`：唤醒前台应用的 AX 通道（确定性结论各只此一次）。返回 `true` 表示刚对 Chromium 宿主成功注入开关——宿主开始异步建树，调用方应以激活预算补读 AX；`false` 表示无需补读（该 pid 已有确定性结论且非常客、不是 Chromium 宿主、前台应用不可识别或注入瞬时失败——最后一种下一手势自然重试）。注入成功即登记唤醒常客；常客 pid 即便已有确定性结论也允许重复注入（注入幂等，宿主可能已自行收回树，重注入是空读后的唯一自救通道）。调用方保证：在平台事件线程上调用。
- `imp::front_app`：问系统要前台应用元素与其 pid：`AXFocusedApplication` 由系统 AX 运行时供给，宿主建不建树都能拿到（这正是唤醒路径可用的前提）。任一步失败返回 `None`（调用方跳过本轮激活）。消息超时沿用读取路径对 systemwide 设置的全局 0.3s 上界，不会无限阻塞事件线程。
- `imp::is_chromium_host`：按 pid 找运行中的应用并取其包路径，查内核框架形态（判定见 `is_chromium_family`）。应用无包（命令行进程等）或框架目录读不出，都按「不是 Chromium 宿主」收口。
- `imp::enable_manual_accessibility`：对宿主应用元素写入两代开关属性：现行内核认 `AXManualAccessibility`，老内核认 `AXEnhancedUserInterface`，一起写入覆盖两代；任一写入成功即视为注入生效。逐属性的写入失败只记 debug（唤醒失败另有 warn 收口）。
