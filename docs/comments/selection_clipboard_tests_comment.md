# clipboard.rs 测试记录

> 按符号锚定（不写行号，防漂移）。改到相关测试时同步更新本文件。
> 测试代码禁止注释（AGENTS.md「注释纪律」节），测试的验收意图与辅助契约记录在此；`// SAFETY:` 仍留在代码内。

## 测试行为速览（BDD）

已统一迁往 docs/tests/bdd.md（git 追踪，以测试代码为唯一事实源）；本文件只留缘由与形态说明。

## CLIPBOARD_LIVE_LOCK

- 2026-09-18 本模块所有触碰系统剪贴板的测试（含 `--include-ignored` 下的手动测试）首行都取这把锁，互斥串行；锁中毒 `unwrap_or_else(into_inner)` 照常继续，不让前一条失败放大成整批失败。为什么必须串行：libtest 默认并行，一边的 Pasteboard 预置/读回与另一边的 arboard 读写撞并发时，NSPasteboard 抛 NSException 穿过 Rust 帧，把整个测试二进制 abort 掉（CI 实测）；互斥串行是唯一正确做法。

## live_tests 形态（不设 #[ignore] 的自动化 live 测试）

- 2026-09-18 `fallback_read_restores_*` 系列测试需要真实系统粘贴板但不需要辅助功能授权：无授权时注入被系统忽略、走 2s 超时降级路径，而验收点是「原内容恢复」而非「读取成功」，降级路径照样能验收——故不设 `#[ignore]`、不调 `require_accessibility`，随 `just test` 在 CI 常跑。这是 AGENTS.md 登记 L4 两种 opt-in 形态（真机授权型 / 环境变量型）之外的第三形态，因不碰授权与凭据未入册。
- 2026-09-18 运行注意（全系列适用）：会覆写本机系统剪贴板，并向前台应用注入一次 Cmd+C；CI 无授权时走 2s 超时降级路径，本机运行会短暂打断当前焦点应用。

## fallback_read_restores_original_clipboard

- 2026-09-18 自动化验收（08 §7.1）：预置剪贴板内容 → 兜底读取（成败皆可，验收点是恢复）→ 断言原内容完整恢复。
- 2026-09-18 文本断言不用 assert_eq、只比对并报长度：恢复被跳过时现值可能是用户窗口期里的真实拷贝，assert_eq 的失败输出会把用户剪贴板内容打印进测试日志。

## fallback_read_restores_multiflavor_clipboard

- 2026-09-18 预置「纯文本 + 自定义 flavor」双 flavor 条目，跑一次兜底读取（成败皆可），断言恢复后两种 flavor 字节原样保留（text 逐字节、自定义 flavor 按字节）。
- 2026-09-18 证伪力边界：只锁「恢复保真」，对「兜底曾因富文本直接放弃」这一原始回归（#37 之前的行为）没有区分力——旧实现直接拒绝兜底、不动剪贴板，本测试照样绿；「富板下兜底必须真的执行」的断言在 `reads_live_selection_from_rich_clipboard_via_simulated_copy`。
- 2026-09-18 文本断言不用 assert_eq（同姊妹测试，防泄用户真实拷贝）；自定义 flavor 只可能是 None 或测试预置的字节，无泄密面，可用 assert_eq。

## fallback_read_restores_empty_clipboard

- 2026-09-18 空剪贴板回归：快照为空 Vec 时恢复是「Clear 后零写入」——兜底读取（成败皆可）后剪贴板应保持为空，不得残留注入产物或半写入状态。

## fallback_read_restores_multi_item_clipboard

- 2026-09-18 多条目恢复归并：预置两个条目（各带文本与自定义 flavor），兜底读取（成败皆可）后断言四个 flavor 各自回到原条目，不串条目。

## fallback_read_unwind_restores_the_clipboard_without_finish

- 2026-10-23 验收双臂：板上恢复为预置文本，或恢复重试耗尽后展开窗口内出现「failed to restore clipboard」error 留痕。背景：CI 高负载虚拟机的粘贴板服务不可用窗口可以超过恢复预算（6×200ms，restore.rs 常量注释记载的既有现象；main 与 feat/popup-fixed-width 的 CI 各挂过一次同签名失败），此时 Drop 已走完恢复路径并留痕，物理恢复不可能，强断言板上内容会把环境故障误判成回归。两臂合起来仍有证伪力：Drop 没跑（skip 被错误置位、守卫未入 settle）则板上无恢复、窗口内亦无留痕，两臂皆空、测试照红。
- 2026-10-23 日志捕获用 capture_global 而非 capture（log.rs 文档对共享调用点的明文规定）：error! 调用点被同进程并发捕获窗口的调用点兴趣缓存竞争污染成 never 时，线程局部窗口整条收不到——2026-10-23 的 CI 失败里「clears=0 且日志为空」正是这个竞争叠加重试耗尽的表现。展开窗口以前后两次 `text()` 快照的字节差切出（缓冲只追加，前快照必为字节前缀；锁内无并发粘贴板测试，窗口内该消息只能来自本测试的恢复路径）。
- 2026-10-30 断言改事件同步，弃时间窗单点观测。CI 连续四次同签名失败（clears=0、留痕空）暴露旧断言的两个缺陷：其一，展开后立即单点读板，恢复写盘与读回之间没有同步点，粘贴板守护进程提交滞后即误判（违反测试纪律「同步点用事件，不用时间」）；其二，快照瞬时失败走整轮放弃路径时本无恢复契约，旧断言不区分，直接把放弃误判成恢复丢失（2026-10-29 main 三次失败与此吻合）。改动：FallbackSettle 新增 `restores_on_unwind` 观察器（有 `WatchRoundFlag::peek` 先例，公开契约观察而非摸内部状态）区分两路——放弃路断言「无注入无恢复、板上保持覆写产物（外部写入不被兜底覆盖，覆写发生在展开判定之前）」；契约路在有界窗口（5s，覆盖 6×200ms 恢复预算加余量）内轮询两个事件：板上回到预置文本，或留痕出现，事件在窗即通过，两空才红。窗口是等待上界不是计时断言；Drop 没跑的回归两事件皆空仍照红，证伪力不减。

## 测试专用 Pasteboard 链接（live_tests 内独立 extern 块）

- 2026-09-18 预置/读回 helper 独立声明 C 链接与句柄类型，不复用 `imp::` 的私有 extern 块：这组 helper 只服务测试，独立声明避免为测试放大生产模块的可见面。

## unsafe fn cf_string / unsafe fn cf_data

- 2026-09-18 契约：分配失败返回 NULL；非 NULL 引用必须恰好释放一次（调用方以 ReleaseOnDrop 收口）。逐调用点的不变量由 `// SAFETY:` 注释承载。

## preset_items

- 2026-09-18 依次把若干「纯文本 + 自定义 flavor」条目写进系统剪贴板，每个元组一个条目。用 Pasteboard API 而不是 arboard：arboard 只能写纯文本，造不出浏览器/编辑器复制的典型多条目形态。任一步失败返回 false（此时板可能已被清空，调用方测试的预置断言会当场红）。
- 2026-09-18 条目标识符 1 起即可：客户端自定义，同 id 的 flavor 归并进同一条目（Pasteboard.h 语义，与 `imp::restore_pasteboard` 的归并路径对应）。

## put_text_and_flavor_for_test

- 2026-09-18 向粘贴板写入一个「文本 + 自定义 flavor」条目；创建的全部引用在本函数内释放（Put 不接管引用）。守卫先于判空建立：任一引用为 NULL 提前返回时，已建的非 NULL 引用仍由守卫释放（守卫自身容忍 NULL）。

## clear_pasteboard_for_test

- 2026-09-18 清空系统剪贴板（空板预置用），失败返回 false。

## read_flavor_data

- 2026-09-18 读回系统剪贴板第 `item_index` 个条目（1 起）上指定 flavor 的原始数据；任何一步失败返回 None（flavor 不存在也归 None）。读前先 Synchronize：本地引用可能滞后于全局板。

## ReleaseOnDrop

- 2026-09-18 测试辅助守卫：出作用域即 CFRelease，容忍 NULL。

## reads_live_selection_via_simulated_copy

- 2026-09-18 手动验收入口：在任意文本编辑器选中文字后运行 `cargo test -p gloss-platform -- --ignored --nocapture`，断言模拟复制兜底能读出选中文本。CI 无图形会话与授权，不参与常规测试。

## reads_live_selection_from_rich_clipboard_via_simulated_copy

- 2026-09-18 手动验收入口（区别性回归）：富剪贴板下兜底必须真的执行而不是放弃——预置 text+自定义 flavor 后 `read()` 必须读出选中文本，旧实现（保全不了富文本即拒绝兜底，#37 之前）在 expect 处当场失败，这是唯一能从公共 API 区分新旧实现的断言点；与 `fallback_read_restores_multiflavor_clipboard` 的恢复保真断言构成完整验收。文本恢复断言不用 assert_eq（防泄用户真实剪贴板，只报长度）。CI 无授权不参与常规测试。

## wait_for_write 单测（mod tests）

- 2026-09-18 change_is_detected_when_generation_bumps：代数变化即确认——前两轮与基线相同、第三轮变化，断言第三轮才返回且恰好轮询 3 次。
- 2026-09-18 timeout_returns_false_without_hanging：超时路径不卡死——到 deadline 返回 false；总耗时上限断言（<2s）防实现退化成永久等待。
- 2026-09-18 unavailable_probe_keeps_polling_until_deadline：probe 持续不可用（None）同样只等到 deadline，不提前放弃也不永久等。

## 豁免

- live_tests 模块级 `#[allow(clippy::let_underscore_must_use)]`：兜底读取的返回值按场景语义可成可败（剪贴板本就是任意状态），用 `let _ = reader.read()` 驱动流程，断言落在恢复结果上。
