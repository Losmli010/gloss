# pipeline.rs（gloss-app/tests）集成测试记录

> 按符号锚定（不写行号，防漂移）。改到相关测试时同步更新本文件。
> 测试代码禁止注释（AGENTS.md「注释纪律」节），测试的验收意图与辅助契约记录在此。
> 命名说明：与 `src/pipeline.rs` 的单测记录（pipeline_tests_comment.md）区分，本文件记录 tests/ 下的 L1 集成测试。

## 层级边界

- L1 库级集成：状态机 + 通道③④ + tokio 消费桥 + MockEngine + moka 缓存的全链路时序（见 AGENTS.md）。真实事件线程（通道②消费、RunLoop、CompositeReader）属于 OS 边界归 L4 opt-in 层——取材产物以 `machine.accept_input` 直接注入，等价于事件线程回传的产物。驱动全部经公共 API，`cargo test` 直接跑。

## 测试行为速览（BDD）

已统一迁往 docs/tests/bdd.md（git 追踪，以测试代码为唯一事实源）；本文件只留缘由与形态说明。

## 辅助

- pipeline：接好 tokio 桥的完整管线，返回驱动所需全部端点。测试辅助函数不在 `#[test]` 内，clippy 的 test 豁免探测不到——显式豁免并沿用「失败即 panic」的测试断言语义（缘由按约束 9 记入本文件）。
- PipelineEndpoints（驱动端点集合）：运行时配置句柄（触发时取快照，可 `save` 模拟设置页保存）；通道②发送端（真实消费者在事件线程 L4 层，L1 不接）；tokio 运行时保活句柄（drop 即关停，消费循环随通道③关闭退出）。
- trigger_then_acquire：触发（划词手势）→ 模拟取材产物回传 → 待确认卡
  → 确认（2026-10-27 起多这一步：提交只落卡不下发，测试点一次「注疏」
  才见通道③的 `RunTask`）→ 下发通道③。
- outcome_body：从状态机取当前产物正文。
- wait_done：收事件直到本任务完成（缓存命中时没有 chunk，只有 TaskDone）。5 秒只是故障兜底：正常路径的同步点是 TaskDone 本身而不是时间；任务被静默丢弃时应当场失败并说明在等什么，而不是挂到 CI 作业超时。
- config_change_invalidates_cache_for_the_next_task 的可观察面：目标语言在管线里的效果就是缓存未命中（TaskOptions 整体进 key）；prompt 文本不外露（execute 渲染完即丢），故不从 prompt 断言。该层能证伪的正是「配置只改在 UI、没进管道」：配置经快照解析进任务选项（machine），再由桥送进 AiTaskService 参与 key（pipeline）。分步：配置未动时同文本同模型命中缓存、引擎不再被调用；保存新模型（与出厂默认不同的 id）后重新请求；再改目标语言同样重新请求。

## 违约回复的降级链路（契约校验步骤 3，2026-10-11）

- `a_contract_reply_that_violates_the_schema_degrades_without_cache` 取代
  `legacy_fence_contract_falls_back_to_a_complete_card`：同一旧契约脚本
  （分类照旧从围栏 kind 标签判 ExplainCode——分类容错到步骤 5/6 才动），
  完成态从「围栏 fallback 出完整卡」改为「违约落降级卡 + 不写缓存」。断言
  三面：degraded 标记与空 note（原文垃圾不展示）、retry 可用（active_request
  保留）、同输入再触发引擎调用数 +2（不投毒缓存）。
- `image_flow_skips_classification_and_settles_the_plain_examples` 取代
  `image_flow_..._commentary`：夹具改嵌套契约，断言面从 ImageCommentary/
  interpretation 改为 Plain/examples——行为面（注位 + 疏位逐条、引擎恰 1 次、
  Arc 同一性）不变。
- `classify_failure_falls_back_and_the_task_still_completes` 的任务脚本从
  散文改为契约 JSON：散文回复现在违约落降级（note 空串），原断言「note ==
  散文」不再成立；该测试的靶心是分类兜底（warn 无内容 + 任务照常完成），
  换契约 JSON 后靶心不变。

## 步骤 5（契约校验与重试方案 §6）——测试注释出体（2026-10-12）

- `superseded_trigger_cancels_and_filters_late_events` 在 chunk 循环后新增的
  一段：`TaskRepairing` 先于终态到达是 B 任务回复（重放脚本 A1A2）违约进修
  复轮的事件序形态——同代标记经 `accept_repairing` 采纳，随后 loop 吸收修复
  轮的 chunk/标记直至 Done(degraded)。原文「任务回复 A1A2 违约：修复轮标记
  先于终态到达，同代的标记照常采纳」出体至此。

## 行内注释出体（2026-10）

- `superseded_trigger_cancels_and_filters_late_events` 的 chunk 循环前注释：每条任务各发一次 TaskClassified——先到的一条属于已被顶掉的 A（或竞速下的 B），按代数分流，只有当前代的判定被采纳（`wait_classified_of_generation` 据此循环丢弃旧代判定）。
- `failure_lands_in_error_and_retry_succeeds` 的开头注释：两次一次性失败——第一次触发里分类吃掉第一条、任务吃掉第二条，落 Error；第二次触发时失败队列已空，照常产流。

## 分类与修复行为（自文件头迁入，2026-10-24）

- 分类完全交给 LLM 层：文本任务先花引擎调用分类（脚本桩通常解析不出类别——分类内部修复一轮后落兜底的通用类别），再花任务执行；脚本里带类别字段的裸 JSON 可让分类解析出具体类别。任务回复的契约违约由 LLM 层修复（至多 3 轮，事件序含修复中事件），耗尽后落降级产物。剪贴板图片任务在 LLM 层恒定映射图像解读、跳过分类——只有执行调用。纯流式/取消/重试语义与缓存/兜底语义按各自的调用数记账。
