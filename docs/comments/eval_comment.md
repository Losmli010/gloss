# eval_comment.md（M3，2026-09-26）

## 隔离的三层防线

依赖方向边表（gloss-eval → core；2026-10-17 从 core/platform 收窄，与 AGENTS
「只消费 core」同口径；且无任何 crate 允许依赖它）+
check-constraints 新增的「eval 资产隔离」双断言（旁路 manifest 依赖 /
include_str! 渗入）+ 自测夹具覆盖两条拒绝路径。备选「靠 AGENTS 约定」被
否：可自动化的一律进门禁。

## 夹具定位与覆盖面

fixtures 是**维护的受控子集**（当前 8 分类 + 6 任务条），刻意覆盖失败
路径（混淆/非 JSON/被拒/坏围栏）——确定性轨的职责是钉住「解析管线在
各输入形态下的行为」，不是逼近全集准确率；全集准确率属于 live 轨。故
replay 报告里 accuracy 50% / 降级率 50% 是夹具设计使然，不是生产回退。

## live 轨的取数点

分类请求复用生产形状（render_classify + max_tokens 截断 + 生产校验器），
模型解析走 ConfigHandle 快照（classify_fallback）——与桥逐点一致。延迟
只在 live 轨有意义（replay 无网络等待）；judge（GLOSS_LIVE_JUDGE=1）是
额外一次模型调用/条，分数只进报告不进门禁。

## parse_classify_reply 转 pub

评测与生产同源的关键一格：校验器单点在 core，eval 直接调用。备选「eval
自带一份等价解析」被否——两份实现漂移后评测数字就不再度量生产行为。

## 夹具回写

live --record 按 id upsert 回 fixtures/（main 的 upsert_fixtures）：分类/
任务条目按数据集归属分流。写入的是仓库资产，属于显式的人类决策（命令
行 opt-in），不由任何门禁自动触发。

## 评审补记（2026-09-26）

- 引擎失败单列为 engine_errors：与「模型输出格式差」分列（指标语义），
  但同计入回退率（生产对两者都落兜底）。任务轨引擎失败计一条降级并
  继续——两条轨共享「跑完全集」策略。
- 任务轨围栏提取改 rfind 镜像 parse_structured；分类轨维持首个围栏镜像
  parse_classify_reply——各自与生产同源，取哪端随生产而定。
- required_fields/degraded 的口径：prompt 输出契约（评测从严），生产
  parse_structured 对缺 word/title 有兜底——文档已写明差异，不再声称
  「同源」。
- judge 门禁：--judge 需 GLOSS_LIVE_JUDGE=1 双确认（额外成本显式化）；
  judge 回复上限独立 512（reason 需要余量）；未计入任何分数时不产
  JudgeSummary（count=0 的均分 0 会被误读）。
- LiveEnv 手写 Debug 脱敏 key（密钥红线）；collect 返回增量序列，--record
  保留真实 SSE 增量形状；回复累积复用 core classify::push_capped（同一
  道 4KiB 界）。
- 重放测试的覆盖断言改下限（>= 当前夹具数），去掉 <全集 上限——覆盖率
  增长不该打破 CI。

## 夹具改修复轨迹（rounds）与异常夹具（2026-10-12，契约校验与重试方案步骤 7）

- 为什么 `Fixture` 从单轮 `deltas` 改为 `rounds: Vec<Vec<String>>`：修复轮（步骤 5）上线后，「一次真实交互」的生产形态是「违约轮 → 修复轮 → …… → 末轮」，单轮夹具只能录到第一轮——按它算出的降级率度量的是「首轮质量」而不是生产行为。轮结构让重放轨还原完整轨迹：修复轮次 = 轮数 − 1（与 `RunOutput.repairs` 同口径），违约类别经 `schema` 校验器逐轮还原（不录违约标签，录了会与校验器漂移——类别永远从原文现算）。
- 为什么异常夹具（空回复/散文/缺 note/kind 越清单）是手工构造的受控样例而不是录制：这四类违约在真实端点上无法按需复现（模型不可控），而确定性轨的职责恰是钉住「解析管线在各输入形态下的行为」（本记录「夹具定位与覆盖面」节的既有定位）。样例按生产修复形态构造：末轮之前的每轮必违约、轮数不超生产预算——runner 的轨迹体检（`check_classify_trajectory`/`check_task_trajectory`）把漂移夹具按硬错误拦下，预算收紧时陈旧夹具当场失败而不是静默算错。
- 为什么单轮违约的旧夹具必须改造成多轮轨迹：生产里单个违约回复永远不会终止任务（必然进修复轮），单轮违约轨迹不是任何可能的生产执行序。改造时按语义分流——围栏/截断/空回复改「修复成功」（末轮判型正确），散文与越清单保留「耗尽落兜底」（终态 InvalidJson/Rejected，维持 invalid_json/rejected 指标的既有覆盖）。
- 为什么异常夹具要配数据集新条目（task-word-03/sent-03/code-03）：夹具 id 必须存在于数据集（`fixtures_align_with_dataset_ids` 双向对齐），而每个 id 只有一条轨迹——异常轨迹不能挂在已有正常条目上。
- 轮记录与增量切分的关系：live `--record` 的分类轨迹来自 `classify_traced` 的轮回复（落盘按单增量成轮）、任务轨迹来自 `on_chunk`/`on_repair` 闭包还原的逐轮真实增量——重放只消费拼接结果，切分不进任何指标，两轨同格式。
- 存量夹具是旧契约（扁平/围栏）录制的说法（见 lib.rs 旧注释）随本次重写解除：全部夹具已是嵌套契约格式，任务降级率恢复按面值可读。

## 违约与修复轮次进评测统计（2026-10-12，契约校验与重试方案步骤 7）

- 统计口径：`ClassifyMetrics`/`TaskMetrics` 各加 `violations: BTreeMap<Violation, usize>`（每一违约轮记一次，含耗尽轨迹的末轮）与 `repair_rounds`（总数）。轮级而非条目级——方案 §6 定 `RunOutput.repairs` 「供评测统计」时指的就是轮次成本；条目级已有 degraded/invalid_json 等四率承担。`Violation` 加 `Ord`（BTreeMap 键稳定迭代）与 serde 派生（报告 JSON 的键形态）归 schema.rs 记录。
- 为什么重放与 live 走同一折算函数（`violation_track`）：两轨的轮来源不同（夹具轮次 vs 生产循环轮记录），折算必须同一份——校验器传参化（`validate_classify`/`validate_task` 各绑一份），数字才可横向比较。
- 为什么 live 轨改走生产循环（`classify_traced`/`run_task_rounds`）而不是继续裸请求：修复轮上线后裸单请求不再是生产形态——它量出的回退率会把「本可修复的违约」全算成失败（系统性悲观）。改走生产循环后 `RunOutput.repairs` 直接进统计（方案 §6 的原话），评测数字度量生产行为；这也是步骤 5 在 core 留 `repairs` 字段的兑现。
- live 分类轨的引擎失败分账得以保留：`classify_traced` 刻意**不消化**请求异常（原样上抛，消化归 `classify`），eval 据 `Err` 单列 `engine_errors`——这与「分类失败与模型行为分账」的既有口径（见「评审补记」）衔接，否则内部消化会让网络故障混进 invalid_json。
- live 任务的回复累积改随生产循环（无 4KiB 上限）：旧 `collect` 对任务回复也套 `push_capped`，那是分类编排的界；生产任务循环本就不设回复上限（SSE 单行界 + 服务端 max_tokens 兜底）。改走 `run_task_rounds` 后此漂移一并消除。
- 报告新增「修复轮次」行（总数 + 违约类别短名计数，`violation_tag` 短名因 `Display` 措辞是整句不适合表格单元格）。
