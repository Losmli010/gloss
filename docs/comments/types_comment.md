# types.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。

## ErrorAction（失败卡的动作按钮）

- 状态机按错误映射表给出该显式给用户的出口，渲染层照画、壳执行。
- `Retry`：可重试类（网络/限流）：原样重发失败的那个任务。
- `OpenSettings`：配置/鉴权类：打开设置页（密钥、模型绑定都在那里修）。

## AppState（五个稳定态）

- 探测不进本枚举（探测段在状态机之外）；提交落待确认卡进 `PendingConfirm`（2026-10-27 前直接进 `Translating`），确认从 `PendingConfirm` 进 `Translating` 并取消旧任务，收起回 `Idle`。它不是独立字段——状态由 `TaskStateMachine::overlay_view` 唯一决定，见 `TaskStateMachine::state`。
- `PendingConfirm`：待确认卡在场（探测产物已到、推理未下发）。与 `Idle` 同为无令牌的态——待确认阶段没有在途推理，通道三零流量。

## OverlayView（浮层内容视图）

- 状态机的可视化投影，由 `ui::popup` 按 TaskKind 分发渲染。
- `Pending`：待确认卡（2026-10-27，发送前确认方案）：探测产物已到、等用户拍板。持有 `input + options` 的冻结快照——确认时原样组装下发请求，不回读配置；取消则整卡丢弃。取材产物与确认之间因此有一道用户闸门。
- `Streaming`：推理中：原文 + 已到达的原始回复。`raw` 是模型原始 JSON 流（`note` 的渐进提取在渲染层）；`classified` 在 LLM 层分类前为 `None`，`accept_classified` 到达后精化（代码解释从分类帧起按代码排版）；`repairing` 置位时页脚切“重新注疏”（无横幅无计数）。字段：`raw` 是模型原始回复的累积（含 JSON 结构，提取归渲染层）；`classified` 是 LLM 层回传的任务类型，分类前为 `None`；`repairing` 表示修复轮进行中（`accept_repairing` 置位）。
- `Outcome`：产物卡：按 `TaskKind` 精排或展示 markdown 注文。`source` 是本次任务的原文，供注疏排布的“经”位；代码语言由渲染层从 `structured.code_language` 现算（唯一来源是产物的 LLM 判定），`degraded` 时渲染降级卡。
- `Failed`：失败信息与动作出口：`action` 为 `None` 表示没有按钮可给（重新划词即可）。

## FailureCause（失败卡的错误来源）

- 状态机只记来源不记文案——文案随界面语言变，属渲染层（`crate::ui::i18n::Text::for_error`），不随任务冻结。
- `TransportChannel`：推理通道不可用（通道③发送失败）。

## InputOutcome（commit_probe 的结果）

- 三态而非 `Option`——“内容疑似敏感”与“陈旧丢弃”在壳侧要做不同的事（前者记 warn，后者只记 debug），合并成 `None` 就分不出来了。
- `Pending`：待确认卡已落定（单元变体，2026-10-27 前的 `Dispatch` 变体随发送前确认删除——提交不再直接下发，壳无需携带 `RunRequest`，确认时由 `confirm_pending` 现组装）。壳据此置挂起显形（出的是待确认卡）。
- `Blocked`：内容疑似敏感：探测丢弃，当前显示保留（它属于上一个会话）。
- `Ignored`：陈旧探测编号或模态错配：不采纳，浮层与通道都不动。

## FailureOutcome（失败处置的结果）

- 三态而非 bool——“静默丢弃”要留排查日志（划词探测失败的唯一痕迹），“陈旧丢弃”只记 debug，合并后就分不出来了。
- `Shown`：已落 `Error` 态，浮层弹失败卡（壳记 warn）。
- `SilentlyDropped`：划词探测遇“无选区可读”：纯误滑——探测丢弃、不弹卡，状态机与当前显示一律不动（壳只记一条带前台应用的 info）。
- `Ignored`：探测编号不符、代数陈旧或已隐藏：不采纳，浮层与状态都不动（壳记 info）。

## RunRequest（下发请求）

- 确认（`confirm_pending`）或重试（`retry`）组装的下发请求：壳把它经通道③发送（2026-10-27 前由 `commit_probe` 直接产出）。
- `generation`：请求代数，与触发同值。
- `input`：取材输入，原样转发给 LLM 层。
- `options`：触发时按配置快照冻结的任务选项（见 `PendingTask`），执行途中不再回读配置。
- `cancel`：随任务下发的取消令牌（App 侧同时留存，新触发时取消）。

## PendingOptions（探测时定下的任务选项）

- 一次配置快照解析出全部参数，产物到达（提交）时随请求下发——单次任务的配置从探测那一刻起就固定了，取材途中换配置不会让同一个任务用上两个版本的参数。任务类型不在其中：kind 由 LLM 层在执行前分类。

## ProbeSource（在途探测的取材模态）

- 决定提交时接受的产物形状，也决定失败时的静默名单。

## ProbeTask（在途的探测）

- 划词或剪贴板图片：同一时刻至多一个，新探测整体替换它；产物到达时经 `TaskStateMachine::commit_probe` 才接管状态。
- `id`：与代数同一计数器分配，提交时提升为代数。
- `source`：决定提交时接受的产物形状与失败时的静默名单。
