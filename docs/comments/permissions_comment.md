# permissions.rs 决策记录

> 按符号锚定（不写行号，防漂移）。本文件随仓库跟踪（docs/comments/ 入库）。

## 两项独立授权与各自的引导方式

- 2026-09-29 划词链路依赖两项互不相同的 TCC 服务：CGEventTap 需要「输入监控」（kTCCServiceListenEvent），AX 选区读取需要「辅助功能」（kTCCServiceAccessibility）。系统设置里是两个独立开关，只开其一的表现完全不同（tap 失败 = 手势不触发；AX 拒绝 = 弹窗只剩失败卡），新装用户极易只开一个——2026-09-28 owner 真机实际踩坑：开了输入监控、漏了辅助功能，划词手势正常但选区读取全部被拒。
- 2026-09-29 输入监控**没有公开预检 API**（SDK 里不存在 CGPreflightListenEventAccess 之类符号，它们是屏幕录制权限的 API，勿混淆）——授权状态只能从 tap 成败推断，引导方式是 tap 降级时直接打开输入监控设置面板（`open_input_monitoring_pane`，面板 URL 指向 Privacy_ListenEvent）。
- 2026-09-29 辅助功能有公开查询 API 且支持让系统弹授权引导（`AXIsProcessTrustedWithOptions` + kAXTrustedCheckOptionPrompt=true），启动预检缺失时即触发（`request_accessibility`），引导对话框非阻塞，结果以用户后续操作为准——因此引导后仍缺权限只记 info，不重试。

## 与 TCC 责任进程机制的关系

- 2026-09-29 从终端/IDE 里直接跑 binary 启动的实例，TCC 判定归属**责任进程**（父应用，如 dev.zcode.app）而非 Gloss 自己：用户给 Gloss 开的授权对这类实例无效，且 AX 调用被拒时 tccd 日志的 attribution 字段里 responsible 指向父应用可资鉴别。从 Finder/Dock 双击或 `open`（LaunchServices）启动的实例责任归自己。开发期正常、打包版划词失败的「灵异」现象多半源于此。

## 引导动作的频次取舍

- 2026-09-29 两条引导路径（启动预检弹辅助功能对话框、tap 降级开输入监控面板）在「TCC 责任进程归属父应用」的用户（从终端/IDE 启动实例）身上会陷入死循环：授权永远无法生效 → 每次启动都弹一次设置窗口/对话框且无法消除。已知取舍，暂不在本 PR 处理；后续跟进方向：引导动作只做一次（持久化标记）或降级为失败卡上的手动入口。
- 2026-09-29 `open_input_monitoring_pane` 用 `spawn` 即时 drop Child：open 秒退后留一个有界僵尸表项（调用点一次性标志，不累积）；若未来改为可多次触发，需先补分离线程收尸（code-review 复审 P3-B 备忘）。

## 输入监控引导的移除（2026-10-09）

- 2026-10-09 移除 `INPUT_MONITORING_PANE` 与 `open_input_monitoring_pane`，权限面收敛为辅助功能一项。动因（2026-10-09 首启实测 + 日志取证）：辅助功能授权下 listen-only CGEventTap 本就能建立，`preflight_input_monitoring` 以「tap 建立成败」推断输入监控状态，授完辅助功能后该推断恒为 true，「开面板」分支永久不可达——首启授完辅助功能后输入监控引导静默缺席；且 TCC 的「监控输入」自动弹窗一生只弹一次（零授权首启时已消耗）。见 docs/tests/bdd.md 的 first_launch 用例与当日 gloss-2026-10-09.jsonl。
- 2026-10-09 佐证：本机（macOS 13 Ventura）当日 8 次启动全部在「辅助功能已授、输入监控未授」下跑通手势，tap 建立日志齐全。原成功日志 `input monitoring granted` 是失实表述（实际干活的是辅助功能），移除时以 `event tap established for the selection gesture` 替代。
- 2026-10-09 已知取舍：手势对「辅助功能足以建 listen-only tap」的依赖是未成文系统行为（Hammerspoon/Rectangle 同款先例），Apple 不保证；若未来系统收紧到 tap 建立失败，手势整体降级并留 warn（`mouse tap creation failed`），届时再评估是否恢复输入监控引导路径。写路径（keychain 删除+重建）与 AX 读取不受影响。

## 运行中授权观察器 AccessibilityWatch（2026-10-09）

- 2026-10-09 新增 `AccessibilityWatch`：启动预检未就绪的会话由组装点武装，事件线程按 1s 节流轮询 `AXIsProcessTrusted`，落定产出一次事件并解除武装。动因：预热门控是启动时刻静态快照且无落定回调，首启会话内授权后 keychain 引导只能等重启或首次划词撞框（2026-10-09 用户诉求「授完辅助功能马上把 keychain 引导也走完」）；1s 节流压查询成本（33ms tick 远密于授权变化尺度），落定秒级延迟无感知差异。
- 2026-10-09 取舍：轮询而非 TCC 回调——系统没有授权变更通知 API（darwinstan 上下无 DistributedNotificationCenter 键可依赖），轮询纯查询是唯一可靠途径；观察器一次性（授权不会倒退），不复用为通用权限状态源。

## 符号契约（自代码注释迁入）

- `WATCH_INTERVAL`：运行中观察的查询间隔：事件线程 33ms 的 tick 远密于授权变化的时间尺度，按 1s 节流；用户刚从系统设置切回来，秒级延迟无感。
- `preflight_accessibility`：纯查询，不弹授权框：用于启动预检与运行中轮询。
- `request_accessibility`：未授权时顺带弹系统授权引导对话框（内含「打开系统设置」入口）。返回值仍是查询时刻的状态：对话框非阻塞，最终授权以用户在系统设置为准，调用方应只在启动预检这类低频节点调用。
- `AccessibilityWatch`：运行中授权观察器：预检未就绪时武装，事件线程每轮 tick 调用一次 `poll`。授权落定即产出一次 `true` 并解除武装——授权不会倒退，之后没有可等的事件。
  - `armed_if`：`grant_pending` 是启动预检结论的否定：预检已就绪时返回未武装实例，此后 `poll` 恒 `false` 且不查询。
  - `poll`：到查询节奏且已授权时返回 `true`，此后不再查询。
  - `poll_at`：`poll` 的时间与查询注入版：测试据此驱动时序与授权序列。
