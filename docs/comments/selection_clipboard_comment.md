# clipboard.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），代码内不留指向本文件的指针。
> 只记录生产代码；测试代码的记录见 `selection_clipboard_tests_comment.md`。

## 模块总览（Pasteboard 级快照/恢复；流程语义随方案 A2 上移共享层，见 fallback 记录）

- 2026-09-18 兜底读取只在“能完整快照原剪贴板”时才注入 Cmd+C：注入把快捷键投给用户的前台应用，可能误拷非文本对象，且注入后剪贴板就不再是用户的内容——没有完整快照就没有可靠恢复，宁可放弃兜底。快照做在 Pasteboard 层（逐条目逐 flavor 保存原始字节，恢复经同一 API 原样写回），浏览器/编辑器复制的 text+HTML/RTF 富文本与多条目结构都能保全；任何一个 flavor 的数据拿不到（如 Finder 的 promised 延迟写入）即整体放弃。
- 2026-09-18 Pasteboard 级快照是 #37 的整改：初版只认纯文本，保全不了富文本就直接拒绝兜底，划词在富剪贴板场景下整条失效；#37 之后富板也能兜底，并由 live 测试 `reads_live_selection_from_rich_clipboard_via_simulated_copy` 守住这一行为差异。

## ChangeMonitor（modified 闩锁）

- 2026-09-18 代数用“闩锁后的 0/1”表达而不是计数：kPasteboardModified 标志在 Synchronize 调用时被消费，错过的中间改动次数无从得知，只能回答“变没变”；baseline 恒为 0，探针只需区分“与基线相同/不同”。

## imp::create_cf_string（稳定字符串值）

- 2026-09-18 粘贴板名等属性/名称常量不用符号常量、改用稳定字符串值构造 CFString：这些常量在现行系统已不导出数据符号（accessibility.rs 同款处理），字符串值是稳定的公共契约。

## imp::OpenPasteboard（名称字符串保活）

- 2026-09-18 打开粘贴板时把名称字符串与引用一起用守卫保活到销毁：PasteboardCreate 对名称参数的所有权约定未文档化（不确定是否拷贝），保守保活，不赌实现细节。

## imp::inject_copy_key（配对释放）

- 2026-09-18 修饰键已按下而后续步骤失败时补发配对释放：否则前台应用停留在孤立的按下态（菜单栏高亮、快捷键半生效），影响用户后续操作。

## imp::restore_pasteboard（两阶段恢复）

- 2026-09-18 两阶段：先为全部 flavor 预建 CFData（任一建不出来即整体放弃，此时粘贴板尚未被触碰），全部就绪后才 Clear + 逐条 Put——把“已清空、半恢复”的失败窗口压到只剩 Put 一步。单阶段边建边写的话，CFDataCreate 中途失败会留下“原内容已清空、新内容没写全”的板，用户原剪贴板内容永久丢失。Put 半途失败仍可能半恢复：数据已全部备妥、失败会留 debug 痕，恢复本就是 best-effort（不决定主结果），接受这个残余窗口。

## snapshot_pasteboard（按 flavor 容错，2026-10-12 修订 2026-09-18 的整体放弃规则）

- **为什么读不出的 flavor 从“整轮放弃”改为“单独跳过”**：原规则（任何
  flavor 数据拿不到即整体放弃、不注入）在含 promised flavor 的板上让兜底
  通道整批失效——Finder 复制文件、部分浏览器/截图链路的板常年带读不出
  的 flavor，此后每一次划词都静默无响应，直到板被别的复制动作改掉。而
  “读不出”本身就意味着我们无法如实恢复该 flavor：整轮放弃能保住它，靠
  的是放弃整个选区；可任何一次成功取材的恢复轮同样会切断这些 promises
  （Clear 后写回的是我们手里的字节）。两害相权：跳过读不出的 flavor，
  可读部分照常保全，选区功能保住；结构读不出（板、条目数、标识、flavor
  名单）仍整体放弃——那才是“恢复无从谈起”的边界。全部 flavor 跳光的
  条目整条跳过，恢复不写空条目。

## snapshot_pasteboard（全不可恢复板收口，2026-10-12 修订）

- **为什么“全部 flavor 都读不出”要回到整体放弃**：flavor 级跳过后，
  板上每个条目都可能被跳空，`items` 变成空 Vec——与“原剪贴板为空”的
  合法空快照（count == 0）在类型上不可区分，而恢复对空快照的语义是
  Clear 后不写任何东西：全 promised 板上注入会让恢复轮静默清空用户
  剪贴板（CRITICAL：用户数据静默丢失）。以 `count > 0 && items.is_empty()`
  把这个形态甄别出来按结构失败收口；条目数 > 0 且仍有可恢复条目的混
  合板不受影响（本条与上一条共同构成最终边界：flavor 级容错只在
  “至少还有内容可如实写回”时生效）。

## 注入实现从 rdev 换为手写 FFI（2026-10-13）

- **为什么动它**：rdev → cocoa 0.22 → block 0.1.6 触发 rustc future-incompat
  硬错误炸弹（届时全仓编译失败）；block 上游无修，rdev 无新版本可升。
  `inject_copy_key` 是 rdev 在生产代码的唯一使用点。缘由与复刻依据见
  event_comment.md。
- **行为保持逐字一致**：四步序列（Cmd 按下 → C 按下 → C 释放 → Cmd 释
  放）与键码不变，事件源状态与注入点不变；换的只是事件的构造与发送从
  rdev::simulate 换成 ffi/event.rs。错误路径的“失败即弃 + 配对补发释
  放”保留（`index > 0` 时尽力补一个 Cmd 释放），收口语义不变（任何一
  步失败即 `SelectionUnavailable`）。原 `#[allow(clippy::let_underscore_must_use)]`
  随 rdev 的 `Result` 返回一起消失——新注入助手返回 bool，无 must_use。
  写入确认窗口是成败的最终判据，注入层只负责把事件发进事件链。
- **授权语义不变**：注入助手返回 true 只代表事件构造成功；未授权时事
  件被系统静默丢弃、助手照常返回 true，成败由写入确认窗口分辨——这是
  rdev 时代就有的行为，组合通道以 AX 权限短路兜底（见 composite 降级
  边界），不是本模块新增的缺口。

## imp::inject_copy_key 事件自带 Command 修饰位（2026-10-15）

- **为什么每个事件都要带 `kCGEventFlagMaskCommand`**：应用识别快捷键看
  的是**事件自带的修饰位字段**（NSEvent.modifierFlags），不是“前面刚
  发过一个 Cmd 按下”。四步序列里的 C 事件此前由
  `CGEventCreateKeyboardEvent` 裸造、flags 为空，前台应用收到的是无修饰
  的 `c` keyDown——被当普通字符打进焦点处（用户报告“页面自动输入c”）。
  独立的 Cmd down/up 保留（照顾追踪修饰键事件的应用），但不再承担传递
  修饰位的职责。该序列从 rdev 时代逐字保留而来（rdev 同款问题）。
- **被否方案**：注入后补发退格删“c”——当 ⌘C 真复制了选区时退格会删
  掉用户选中的内容，不可接受。

## 符号契约（自代码注释迁入；流程符号随方案 A2 上移共享层，见 fallback 记录）

- `ChangeMonitor`：写入确认信号源（系统（macOS）原语）：注入前建立基线，之后轮询代数变化。
  - `new`：建立基线：同步一次消费既有 modified 标志。
  - `baseline`：以闩锁后的 0/1 表达代数，基线恒为 0。
  - `generation`：返回当前代数：任一次 Synchronize 报告 modified 即闩锁为 1（标志被消费，不能依赖后续调用，见 K_PASTEBOARD_MODIFIED）。
- `ClipboardFallbackReader`：剪贴板兜底读取器（系统（macOS）绑定）：持取材轮次闩锁句柄，把粘贴板变更计数、Pasteboard 层快照与还原、同步标志闩锁监视、HID 注入与板文本读取接上共享流程（`FallbackBoard` 原语束）；流程与收尾语义见 fallback 记录。阻塞语义归共享流程：确认目标应用写入最长两轮确认窗，期间事件线程被占用、其余命令与事件排队（事件源侧缓冲），调用方需自行处理在途重复触发。
- `keys_to_release`：失败点之前已按下、需要补发释放的键（逆序）。`index` 是失败事件的序号：只有 `sequence[..index]` 里 `key_down` 的事件才是按下态。已成功释放的键可能再收一次 key-up（重复释放幂等，无副作用）。
- `inject_copy_key`：注入复制快捷键：修饰键按下 → C 按下/释放 → 修饰键释放。C 的按下与释放自带 Command 修饰位（应用按事件自带的修饰位识别快捷键，裸 C 会被当成普通字符打进焦点处）；修饰键按下同样带位，释放不带——修饰键事件的修饰位表达事件之后的状态，带位释放会向下游宣告“Command 仍按着”。任何一步失败即放弃；失败点之前按下的键全部逆序补发释放（只放修饰键会在 C 键释放失败时留下按下态）。错误路径上的补发是尽力而为，释放失败已由紧随的 SelectionUnavailable 收口。
- `CfGuard` 接板引用（测试辅助 `preset_items`、`clear_pasteboard_for_test`、`read_flavor_data` 三处共用，2026-10-25 自源码迁入）：`PasteboardCreate` 成功后 `pb` 是 +1 引用，立即交给守卫在离开作用域时释放；守卫构造本身是安全调用，引用计数契约沉此记录——按注释纪律，不变量注释只贴不安全块，安全调用上方不留 `// SAFETY:`（邻接校验 2026-10-25 起入门禁）。

## “Clear +1 / Put +0”计数模型的实测依据（2026-10-19）

哨兵写盘归因（`RoundWrites.ours`）按“实际清板数”计的前提是一次恢复序列
只推进一次 changeCount。该模型由一次性 scratch 测试实测钉死（未入库，避免
依赖环境时序的断言进套件）：

- 方法：临时测试先 `Clipboard::set_text` 预置板面，读
  `pasteboard_change_count` 作基线；再用与 `restore_pasteboard_once` 同参的
  裸 FFI 序列（`PasteboardCreate` → `CFDataCreate` → `PasteboardClear` →
  `PasteboardPutItemFlavor`，flavor `public.utf8-plain-text`、15 字节、
  options 0）各执行一步，每步后再读计数。
- 结果：`clear_status=0 put_status=0 clear_delta=1 put_delta=0`——
  `PasteboardClear` 使 changeCount +1，`PasteboardPutItemFlavor` +0。
- 推论：恢复的归因口径必须数 Clear 的实际发生次数；“Clear 成功而 Put 失败
  → 重试成功”会清两次板，固定记 1 会把差额判成用户的外来复制（快照含图
  时误触发图片任务）。审查曾建议在 live 测试端到端断言“finish 前后计数差
  == 自报 ours”，权衡后未采纳：该断言对 1.6s 窗口内的外来写入敏感，正是
  CI 偶发的新来源；现有 `ours ∈ {0,2}` 的宽松断言保留。
