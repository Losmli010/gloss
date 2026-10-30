# 图像纵切面走查记录（重构计划阶段 5）

面向图像里程碑的架构保持重构计划阶段 5 的走查产出：沿现有剪贴板图片路径逐环节记录现状与缺口。依据计划文档的定位与边界一节执行：只走查、只记录，不实现里程碑功能；链路不可触发处只记缺口，不顺手补齐。行号对应本分支基线提交 `9d94111`；本走查不改生产代码，行号即验收时的行号。

## 链路现状（逐环节）

### 环节一：取材哨兵与内容读取

- 现状：哨兵与读取器双栈分工已成形。哨兵（`ClipboardWatchSource`）随平台事件线程节拍轮询变更计数，250 毫秒自节流，计数前进且类型清单含图像才产出观察事件，纯文本复制不触发（`crates/gloss-platform/src/clipboard/watch.rs:20`、`crates/gloss-platform/src/clipboard/watch.rs:120-162`）；取材轮次写盘经计数归因吞掉本应用副产物（`crates/gloss-platform/src/clipboard/watch.rs:126-136`）。读取器（`ClipboardImageReader`）先比对观察计数再读内容，竞态覆盖静默丢弃、声明图像却交付不出的路径（系统 15 及以上粘贴授权被拒）静默加警告痕（`crates/gloss-platform/src/clipboard/image.rs:183-203`）；设界逐级前置——风格字节 64 `MiB`、像素面积 4096×4096、整图解码、长边 1536 降采样、产物 20 `MiB`（`crates/gloss-platform/src/clipboard/image.rs:29-35`）。
- 测试覆盖：预算与解码内联测试齐全（`crates/gloss-platform/src/clipboard/image.rs:244-423`）；真实粘贴板活测试见冒烟记录节。

### 环节二：平台事件线程与装配

- 现状：观察事件在事件线程产出后映为通道 1 的 `PlatformEvent::PasteboardImageObserved`（`crates/gloss-app/src/channel.rs:16-21`；装配映射在 `src/wiring.rs:255-261`）；哨兵与读取器在同一线程装配，读取器持哨兵的观察句柄做竞态比对（`src/wiring.rs:209-211`）。取材命令（通道 2 的 `AcquireCommand::AcquireClipboardImage`，`crates/gloss-app/src/channel.rs:24-27`）由命令处理闭包消费，读板解码后以 `Event::InputReady` 携 `TaskInput::Image` 回传通道 4（`src/wiring.rs:287-289`、`src/wiring.rs:336-367`）；命令处理与事件源轮询都有崩溃屏障（`crates/gloss-platform/src/events/mod.rs:164-181`）。

### 环节三：触发决策图像分支

- 现状：`trigger_decision` 对图像观察只过场景闸门——安全输入态与敏感应用清单（`crates/gloss-app/src/machine/policy.rs:58-61`；闸门判据 `crates/gloss-core/src/domain/guard.rs:60-76`）；图像观察不做前台本应用抑制，与划词手势相异且由测试锁定（`crates/gloss-app/src/machine.rs:530-538`，剪贴板监听无本应用误触面）。命中闸门时壳侧留警告痕、不产取材命令（`crates/gloss-app/src/flow/probe.rs:101-107`）。状态机侧 `begin_pasteboard_probe` 只领探测编号、不动可见会话（`crates/gloss-app/src/machine.rs:108-132`）。

### 环节四：任务选项快照冻结

- 现状：选项在观察时刻一次性冻结——`begin_pasteboard_probe` 按配置快照解析 `TaskOptions`（目标语言、模型标识、提示词语言，`crates/gloss-app/src/machine/policy.rs:68-74`）存入探测载荷（`crates/gloss-app/src/machine.rs:126-130`），随待确认卡携带、确认后进 `RunRequest` 下发（`crates/gloss-app/src/machine.rs:164`、`crates/gloss-app/src/machine.rs:291-312`、`crates/gloss-app/src/flow/task.rs:127-135`）；桥与大语言模型层不回读配置（`crates/gloss-app/src/runtime/pipeline.rs:17-19`）。语义边界：模型标识取自冻结选项，端点与密钥在请求时刻读当前配置快照（`crates/gloss-core/src/pipeline/engine/llm.rs:87-96`）——在途任务不换模型但跟随当前端点，现状即设计。

### 环节五：任务模板

- 现状：图像解读模板中英两份，编译期嵌入（`crates/gloss-core/src/pipeline/prompt/templates.rs:37`、`crates/gloss-core/src/pipeline/prompt/templates.rs:47`；模板正文 `crates/gloss-core/prompts/zh/image_explain.md`、`crates/gloss-core/prompts/en/image_explain.md`）。渲染时图像输入产出系统提示加带图用户消息两段（`crates/gloss-core/src/pipeline/prompt.rs:63-70`），模板选择按固定映射落 `ImageExplain`（`crates/gloss-core/src/pipeline/prompt.rs:106`）。模态矩阵封闭：图像输入只配 `ImageExplain` 一格，其余组合以模态不支持拒绝（`crates/gloss-core/src/domain/task.rs:101-117`）。

### 环节六：引擎执行与流式回传

- 现状：图像输入固定映射 `ImageExplain` 并跳过分类，一次引擎调用即出流（`crates/gloss-core/src/pipeline/engine.rs:75-95`）；增量块边积累边经回调回传，1 `MiB` 回复上限设界（`crates/gloss-core/src/pipeline/engine.rs:128-165`）；图像部件在请求体构造处编码为数据地址（Base64）的 `image_url`（`crates/gloss-core/src/pipeline/engine/llm/request_body.rs:42-46`），请求恒带（JSON）输出模式。契约修复轮复用同一条图像会话（图随修复轮重发，`crates/gloss-app/tests/pipeline.rs:427-479`），语言体例门与降级标识同文本路径。缓存键按图像字节流式哈希、选项全量参与（`crates/gloss-app/src/runtime/cache.rs:25-41`），同图第二次复制缓存直出（`crates/gloss-app/tests/pipeline.rs:551-586`）；空注降级产物不写缓存（`crates/gloss-app/src/runtime/pipeline.rs:225-227`）。

### 环节七：图像视图

- 现状：浮层渲染入口按帧取状态机的附件图像传入绘制（`crates/gloss-app/src/app/mod.rs:145-147`）；待确认卡与流式、产物卡三态都渲染缩略图，渲染失败落“图片不可用”占位（`crates/gloss-app/src/ui/popup/views.rs:170-188`、`crates/gloss-app/src/ui/popup/outcome.rs:115-130`、`crates/gloss-app/src/ui/popup/outcome.rs:235-247`）；解码与纹理装入分两侧，纹理按字节指针跨帧复用、图像离场即释放（`crates/gloss-app/src/ui/popup/image.rs:10-30`、`crates/gloss-app/src/ui/popup/mod.rs:133-156`、`crates/gloss-app/src/ui/popup/mod.rs:199-201`）。图像错误文案双语文案表齐备（`crates/gloss-app/src/ui/i18n.rs:140-141`）。第二层（L2）快照锁定四种图像卡形态：产物、流式、待确认中英（`crates/gloss-app/src/ui/popup/mod.rs:1724-1746`，基线 `crates/gloss-app/tests/snapshots/popup_image_outcome.png` 等四张）。

### 环节八：失败与取消

- 现状：图像取材失败一律出失败卡（`ProbeSource::PasteboardImage` 不静默，`crates/gloss-app/src/machine.rs:174-180`），“图片过大”失败卡有壳侧测试（`crates/gloss-app/src/flow/task.rs:883-897`）；隐藏浮层即取消在途任务并清附件，迟到事件按代数过滤（`crates/gloss-app/tests/pipeline.rs:588-632`）；新一轮观察覆盖在途探测，旧产物按编号判陈旧丢弃（`crates/gloss-app/src/flow/task.rs:61-72`）。

## 冒烟记录

链路判定：可端到端触发。生产装配完整（配置开关默认开，`crates/gloss-core/src/domain/config.rs:167`；入口按配置装配哨兵开关，`src/main.rs:113-115`），本机配置实值 `watch_clipboard_images = true`。执行了两次机器可执行的冒烟，人工段未执行（见下）。

- 冒烟一（取材半程，活测试）：运行 `cargo test -p gloss-platform --lib clipboard::watch -- --ignored`（登记于 `docs/tests/bdd.md` 人工测试节），结果 `watch_and_reader_carry_a_real_image_copy ... ok`（1 项通过、126 项过滤）——真实剪贴板上（TIFF）与纯（PNG）两种风格写入后，哨兵各恰观察一次、读取器读回同尺寸可解码位图。
- 冒烟二（触发半程，真机全应用）：`cargo build -p gloss` 后以 `GLOSS_LOG_DIR` 指向临时目录后台启动调试版应用，启动里程碑 `01_entry` 至 `06_ready` 用时 655 毫秒；应用就绪后经 `osascript` 向系统剪贴板写入一张 320×200 的（PNG）图片。应用日志（`gloss-2026-10-30.jsonl`）在复制后的一个观察节拍内记录 `trigger suppressed by the sensitive content guard, reason: secure input active`（`crates/gloss-app/src/flow/probe.rs:103-106` 的输出）——真机触发链的前半段（哨兵观察、通道 1、探测入口、触发决策、场景闸门）活链路验证通过，取材命令按设计未下发。拦截原因为本机控制台当时处于锁屏态（`ioreg` 显示 `CGSSessionScreenIsLocked=Yes` 且 `kCGSSessionSecureInputPID=145`，持锁进程为 `loginwindow`）：登录窗持有安全输入，场景闸门按设计拦截，属预期行为而非缺陷。冒烟后剪贴板已恢复原文本内容。
- 人工段（未执行）：出卡后半段——待确认卡渲染、确认点击、真实引擎流式、产物卡人眼验收——需要解锁的图形会话与真人在场，本次未走通。可执行清单已登记：来源矩阵与上限检查（`docs/tests/bdd.md` 人工测试节 `image_commentary_sources_toggle_and_limits_manual_checks`）、系统 15 粘贴授权闭环（同节"允许粘贴"条目）、真实端点图像契约（`live_llm_streams_an_image_contract`，需 `GLOSS_LIVE_*` 三变量）。

## 框选截图路径的预期接入点（只标注，不实现）

平台事件侧：

1. 手势来源：全局监听当前只订阅左键按下与释放、只产出 `MouseGesture::Selection`（`crates/gloss-platform/src/events/mouse.rs:42-45`、`crates/gloss-platform/src/events/mouse.rs:113-115`）——框选手势在此扩展变体或另起事件源。
2. 事件源注册：`event_sources` 汇总各源（`src/wiring.rs:226-274`），截图手势源在此注册。
3. 通道 1 变体与消费：`PlatformEvent` 增截图事件变体（`crates/gloss-app/src/channel.rs:16-21`），`drain_platform_events` 增匹配臂（`crates/gloss-app/src/flow/probe.rs:30-43`）。
4. 决策与状态机：`trigger_decision` 增截图臂（`crates/gloss-app/src/machine/policy.rs:47-66`）、`ProbeSource` 增截图探测源（`crates/gloss-app/src/machine/types.rs:80-83`）、状态机增对应的 `begin_*_probe`（形态照 `crates/gloss-app/src/machine.rs:108-132`）。

取材命令侧：

1. 通道 2 变体：`AcquireCommand` 增截图取材变体（`crates/gloss-app/src/channel.rs:24-27`）。
2. 事件线程处理臂：命令处理闭包增匹配臂（`src/wiring.rs:276-292`），截图采集在平台事件线程执行（线程亲和），崩溃屏障既有覆盖（`crates/gloss-platform/src/events/mod.rs:164-181`）。
3. 产物回传：截图产物直接复用既有图像下半程——以 `Event::InputReady` 携 `TaskInput::Image` 回传（形态照 `src/wiring.rs:336-367`），后续冻结选项已在其前、提交与出卡路径零改动。截图采集不写剪贴板则无需取材轮次归因（`WatchRoundFlag` 仅服务剪贴板注入路径）。

## 缺口清单

每条含四元组：环节、现状、缺口、建议归属。本走查未发现归本计划补刀的条目（计划阶段一至四已收尾，阶段五边界为只记录），以下全部归图像里程碑。

- 缺口一（环节：平台事件——框选手势）：现状：全局监听只有划词手势一个产物，事件清单无截图语义（`crates/gloss-platform/src/events/mouse.rs:113-115`、`crates/gloss-app/src/channel.rs:16-21`）。缺口：截图手势的监听与通道 1 事件变体及来源注册（`src/wiring.rs:226-274`）。归属：图像里程碑。
- 缺口二（环节：触发决策与状态机）：现状：决策与探测源只覆盖划词与剪贴板图像两入口（`crates/gloss-app/src/machine/policy.rs:47-66`、`crates/gloss-app/src/machine/types.rs:80-83`）。缺口：截图入口的决策臂、探测源与探测入口函数。归属：图像里程碑。
- 缺口三（环节：取材命令——截图采集）：现状：通道 2 只有取词与剪贴板图片两命令，事件线程处理面只两臂（`crates/gloss-app/src/channel.rs:24-27`、`src/wiring.rs:276-292`）。缺口：截图采集命令、采集实现与失败语义（权限被拒、区域无效、采集失败三路）。归属：图像里程碑。
- 缺口四（环节：系统授权——屏幕录制）：现状：权限面只有辅助功能的预检、请求与异步补授（`crates/gloss-platform/src/permissions.rs:11-34`）。缺口：屏幕录制授权的前置检查、引导与降级路径（失败卡或指引），以及授权态跨次启动的记忆。归属：图像里程碑。
- 缺口五（环节：引擎——端点接受度）：现状：客户端侧产物上限为长边 1536、（PNG）产物 20 `MiB`（`crates/gloss-platform/src/clipboard/image.rs:31-35`），请求恒带（JSON）模式（`crates/gloss-core/src/pipeline/engine/llm/request_body.rs:42-46` 及 `crates/gloss-core/src/pipeline/engine/llm.rs:479-503` 的锁定测试）。缺口：端点对图像体积、分辨率与视觉加（JSON）模式组合的服务端限制未在客户端建模，超限只以通用引擎失败落卡；真实端点的视觉能力验证只有第四层人工测试承载、未在本机执行。归属：图像里程碑（随真机验收清单执行）。
- 缺口六（环节：验收——端到端人工冒烟）：现状：机器可执行的两半已验证（见冒烟记录），出卡后半段的人眼验收未执行——本机控制台锁屏致场景闸门按设计拦截，且确认点击无人在场。缺口：解锁图形会话内按 `docs/tests/bdd.md` 人工测试节三条清单（来源矩阵与上限、系统 15 粘贴授权、真实端点图像契约）执行并回填结果。归属：图像里程碑验收。
- 缺口七（环节：防护——截图内容的设计决策）：现状：内容闸门只扫文本，图像路径不设内容闸门、以待确认卡的确认点击为发送前唯一把关（`crates/gloss-app/src/machine.rs:8-10` 模块文档、`crates/gloss-app/src/machine.rs:139-149`）。缺口：框选截图属用户显式截取而非显式复制，画面可能含非预期入镜的敏感文本，是否沿用“确认即把关”或增设提示，需图像里程碑显式决策（本走查只登记，不预设结论）。归属：图像里程碑（设计决策）。

## 走查中的现状确认（非缺口）

- 竞态窗口（观察与取材之间板被覆盖）静默丢弃、留调试痕，与划词空选区同型处置（`crates/gloss-platform/src/clipboard/image.rs:190-196`）。
- 附件图像字节跨通道与跨线程以 `Arc` 共享，仅提交处一次克隆供附件与待确认载荷分持（`crates/gloss-app/src/machine.rs:147`）；流式视图对图像任务的原文槽恒空串、图像由附件状态渲染（`crates/gloss-app/src/machine/policy.rs:26-37`、`crates/gloss-app/src/ui/popup/views.rs:140-147`）——图像“原文”的承载位置在图像里程碑触碰视图时一并定形即可。
- 剪贴板图片监听开关热切换已闭环：设置保存即时写共享原子量（`crates/gloss-app/src/flow/settings_session.rs:52-66`），哨兵关闭零查询、重开重记基线，关闭期间旧内容不触发（`crates/gloss-platform/src/clipboard/watch.rs:138-145` 及其测试）。
