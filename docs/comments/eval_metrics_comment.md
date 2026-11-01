# gloss-eval/src/metrics.rs 决策记录

## percentile 就地排序（本地门禁第 3 轮，2026-10-13）

- `percentile` 改 `&mut self` 就地按值排序：百分位与样本顺序无关、重复
  调用幂等，可观察值不变；不再为每次查询复制整份样本。`Latency` 的
  原始样本从不序列化（history.jsonl 只落 `LatencySummary` 的
  count/p50/p95），就地排序无观察面。

## Latency / percentile

- `Latency`：延迟样本（毫秒）——评测运行的逐条端到端耗时，报告取
  p50/p95 分位。
- `percentile`：百分位（线性插值）；样本为空返回 `None`。
- 2026-11-01 评测工具链重构步骤 1：重放与实跑轨删除，模块收窄为延迟
  采样器——契约与分类的判定机（`TaskVerdict`、`ClassifyMetrics`、
  `ViolationTrack` 等）随轨删除，结算在重构计划的结算层模块重建；
  延迟指标按计划保留（删除清单“保留且非残留”）。
