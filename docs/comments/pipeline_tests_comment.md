# pipeline.rs（gloss-app/src）测试记录

> 按符号锚定（不写行号，防漂移）。改到相关测试时同步更新本文件。
> 测试代码禁止注释（AGENTS.md“注释纪律”节），测试的验收意图与辅助契约记录在此。
> 命名说明：本文件与集成测试 `tests/pipeline.rs` 同名，后者记录在 pipeline_integration_tests_comment.md。

## 测试行为速览（BDD）

已统一迁往 docs/tests/bdd.md（git 追踪，以测试代码为唯一事实源）；本文件只留缘由与形态说明。

## 辅助

- start：起一条完整管道，返回（③发送端，④接收端，运行时）。
- text_task / run：构造文本任务与推进管道的夹具助手。

## 步骤 5（契约校验与重试方案 §6）——测试注释出体（2026-10-12）

- `engine_failure_becomes_task_failed` 的断言前注释（“分类先失败（同引擎同
  错误）内部消化落兜底并回传判定，任务段再失败才是 TaskFailed——两条错误
  同变体，事件序是本断言的靶心”）出体至此：该测试的靶心是同变体错误在分
  类段与任务段的不同归宿——分类段由 `classify` 内部消化（TaskClassified 携
  带兜底 General 先回传），任务段才落 TaskFailed；步骤 5 前分类段的兜底由
  engine.rs 编排层承担，注释措辞随收拢点迁移同步更新，现按纪律整体出体。

- a_lang_degraded_done_caches_the_full_outcome_for_replay（降级批注方案
  §6）：补断言降级产物携非空违约清单入缓存、命中回放清单逐条一致——
  “清单随完成产物恒存、回放不重判”的通道两端锁点。
