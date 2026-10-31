# gloss-eval/src/metrics.rs 决策记录

## 任务轨接生产同源校验器（契约校验步骤 3，2026-10-11）

- `mirror_complete`（对 gloss-app finalize 的手工镜像，含围栏 fallback）整体
  删除，`TaskVerdict::for_reply` 改调 `gloss_core::schema::validate_task`：
  eval 受依赖方向约束不能消费 gloss-app，但 core 的 schema 模块就是两侧
  共用的那份契约——“镜像 + 测试对齐两侧语义”的维护负担由同源直接消掉。
  eval 的分类轨本步仍走 `classify::parse_classify_reply`（与当步生产同源；
  schema::validate_classify 的接线归步骤 5/7，见 schema.rs 记录）。
- `TaskVerdict.outcome` 从 `OutcomeStructured` 改为完整 `TaskOutcome`（含
  degraded 标记）：UI 收到的产物就是 TaskOutcome，评测记账与生产同一形状；
  `degraded()` 从“四级标志拼合”改为直接读 `outcome.degraded`（校验器是
  唯一裁决，指标不自带第二套判罚）。
- `json_object` / `note_present` / `fields_complete` 三标志仍独立折算：
  json_object 认“合法 JSON 对象”（数组/标量不算）、note_present 认
  trim 后非空（与硬约束同口径）、fields_complete 认 `structured` 对象在场
  且含必需键——必需键表收敛为恒 `["examples"]`（phonetic / code_language
  允许缺省或 null）。
- 数据集 reference 与夹具**未动**（扁平 reference、旧契约录制留待步骤 7）：
  `dataset.rs::required_fields` 服务数据集 reference 的加载校验，与回复侧
  的 `required_structured_fields` 是两回事；旧契约夹具在新校验器下自然落
  降级轨，重放报告的降级率在步骤 7 重录夹具前会虚高——读数时注意（
  TaskMetrics 文档已有同类提醒）。

## percentile 就地排序（本地门禁第 3 轮，2026-10-13）

- `percentile` 改 `&mut self` 就地按值排序：百分位与样本顺序无关、重复
  调用幂等，可观察值不变；不再为每次查询复制整份样本。`Latency` 的
  原始样本从不序列化（history.jsonl 只落 `LatencySummary` 的
  count/p50/p95），就地排序无观察面。`assemble_report` 的 latency 参数
  相应收 `Option<&mut Latency>`（调用方均持有本地 mut 值）。

## ClassifyVerdict

- 分类判定的结果归类（比 Ok/Err 细一档：指标要区分“回复不是 JSON”与“JSON 合法但被生产校验器拒绝”）。
- 变体：`Correct` 判定正确；`Wrong` 判定成了别的（合法类别映射出的 kind，但 ≠ 期望）；`InvalidJson` 回复不是合法 JSON（含围栏提取失败）；`Rejected` JSON 合法但被生产校验器拒绝（不是 classify 模板的类别标识）。

## ViolationTrack

- 修复轨迹的违约统计：每一违约轮记一次类别，修复轮次 = 轮数 − 1（与生产 `RunOutput.repairs` / `ClassifyDecision.repairs` 同口径）。
- 字段：`violations` 违约类别 → 违约轮次数（含末轮违约——预算耗尽的收尾）；`repairs` 修复轮次总数。

## violation_track

- 从修复轨迹（每轮完整回复）折算违约统计：逐轮跑调用方给定的生产校验器（分类/任务同一份 `schema` 模块）。重放轨与 live 轨共用——live 的轮次来自生产循环本身，折算口径与重放完全一致。

## violation_tag

- 违约类别的报表短名（`Display` 措辞是句子，表格单元格里用短名）。

## ClassifyMetrics

- 分类指标：accuracy / 混淆矩阵 / 无效 JSON 率 / 回退率 / 违约与修复轮次。
- 字段：`evaluated` 进入统计的条数（无夹具的条目跳过不计）；`correct` 判定正确数；`confusion` 混淆矩阵（期望 kind → 实际 kind → 次数，仅判定错误路径）；`invalid_json` 回复不是合法 JSON 的次数（模型输出格式质量问题）；`rejected` JSON 合法但被生产校验器拒绝的次数（契约问题）；`engine_errors` 分类请求本身失败（网络/限流）的次数——与模型行为无关，不计入格式类指标，但计入回退率（生产编排对这类条目同样落兜底）；`violations` 违约类别计数（每一违约轮记一次，含修复成功与耗尽两类轨迹）；`repair_rounds` 修复轮次总数（生产 `ClassifyDecision.repairs` 同口径）。

## accuracy / fallback_rate

- `accuracy`：判定正确率（分母 = 进入统计的条数）。
- `fallback_rate`：回退率——生产编排会对这些条目落兜底 kind（无效 JSON + 被拒绝 + 引擎失败）。

## TaskVerdict

- 任务回复的契约达成情况（比“解析成功与否”细：四个指标各自独立，修复轨迹随行）。
- 字段：`json_object` 回复是合法 JSON 对象（契约硬约束之一）；`note_present` JSON 对象携带非空 `note`（注；硬约束之二，词卡/句译/讲解/图像）；`fields_complete` `structured` 对象在场且携带 kind 契约必需的疏证键（软约束项，缺失只降丰富度不降产物；JSON 对象在场才可能为真）；`outcome` 生产同源的解析产物（与 UI 收到的 `TaskOutcome` 同构，含降级标记——违约即空 note 空变体 `degraded = true`）；`violations` 违约类别计数（每一违约轮记一次；单轮判定最多一条）；`repairs` 修复轮次（生产 `RunOutput.repairs` 同口径）。

## TaskVerdict::for_reply

- 按 `TaskCase` 的 kind 与回复原文逐级判定（单轮口径：该回复即末轮，无修复轨迹）。
- 判定与生产完成态解析同源（gloss-app `finalize::complete` 同样走 `gloss_core::schema::validate_task`）；`json_object` / `note_present` / `fields_complete` 从违约类别与 `structured` 在场情况折算，供分档指标。

## TaskVerdict::for_rounds

- 按 `TaskCase` 的 kind 与**修复轨迹**（每轮完整回复）判定：末轮定完成态，全程逐轮经生产校验器折算违约类别与修复轮次——与生产 `RunOutput.repairs` 同口径（重放轨自夹具轮次、live 轨自生产循环的轮记录，共用本判定）。

## TaskVerdict::degraded

- 降级判定：契约校验器判违约（非 JSON / 空回复 / 缺 note）。

## TaskMetrics

- 任务指标：契约 JSON 率 / note 在场率 / 字段完整率 / 降级率 / 违约与修复轮次。
- 字段：`evaluated` 进入统计的条数；`json_object` 回复是契约 JSON 对象的条数；`note_present` JSON 对象带 `note`（注）的条数；`fields_complete` 必需字段完整数；`degraded` 降级数（见 `TaskVerdict::degraded`）——live 轨的任务请求失败也记入此处，降级率因此混入传输失败（与分类轨单列 engine_errors 不同），读数时注意；`violations` 违约类别计数（每一违约轮记一次，含修复成功与耗尽两类轨迹）；`repair_rounds` 修复轮次总数（生产 `RunOutput.repairs` 同口径）。

## Latency / percentile

- `Latency`：延迟样本（毫秒）——live 轨的逐条耗时，报告取 p50/p95。
- `percentile`：百分位（线性插值）；样本为空返回 `None`。就地按值排序（百分位与顺序无关，重复调用幂等），不再为每次查询复制整份样本。

## judge_classify_reply

- 对一条分类回复判定：生产校验器为主，无效 JSON 由本模块单独识别（它度量模型输出格式质量，校验器对两者同样拒绝，但指标要分列）。

## is_json_like

- 回复是否“长得像 JSON”：裸 JSON 或**首个** ``` 围栏内是 JSON——刻意比生产校验（严格裸 JSON，无围栏容错）宽一档：生产对两者同样拒绝，本判定只把“非 JSON 形态”与“形态像但被拒”分列统计。只判形态，不判 kind 合法性——那归生产校验器。

## first_fenced_json

- 首个 ``` 围栏内 JSON（语言标识行跳过）：“长得像 JSON”的形态判定专用——生产已无围栏容错，本提取不再镜像任何生产路径。
