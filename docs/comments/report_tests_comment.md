# report.rs 测试形态说明

## 测试形态（2026-11-01 评测工具链重构步骤 4 后）

- 聚合测试全部经公共入口 `assemble` 造夹具（手工构造 `CaseDetail`，不触
  及私有计算函数），按计划的聚合口径逐条钉住：
  `group_aggregates_fold_into_totals` 逐列对账“各组合计等于总数”，并钉
  修复轮次均值按全体用例重算；`rate_denominators_are_all_group_cases`
  钉三率分母恒为组内全部；`bucket_edges_split_at_59_60_69_70_79_80_89_90`
  钉五档分桶边界（59|60、69|70、79|80、89|90 各归右档）与单维合格率；
  `coverage_and_agreement_rates_use_their_own_denominators` 钉评审覆盖
  率与人机一致率各自的分母（四种人机组合加未评、未标注夹具）；
  `overall_score_is_the_four_dimension_mean_and_respects_the_pass_line`
  钉总体分派生与恰 80 过线；`quality_stays_empty_without_judged_cases`
  钉质量置空条件（评审关闭、全分类组、混合组）。
- 渲染测试断言 Markdown 行文本：`markdown_renders_group_rows_and_totals`
  钉分组行、合计行、百分数与置空占位符；
  `latency_only_shows_when_sampled` 沿用双向断言延迟表的出场条件。
- 序列化面：`report_round_trips_through_serde` 钉分组/合计/延迟/质量的
  往返与 `_ms` 字段名；`generated_at_is_a_numeric_unix_timestamp` 继续
  钉时间戳为裸数值。
- `legacy_rows_without_aggregates_fail_parsing_for_tolerant_skip` 是步
  骤 1 旧兼容测试的反转：分组聚合段无 serde 缺省，旧形状历史行解析即
  败，由历史读取按坏行容错跳过（计划的既定口径）。
- `utc_formatting_is_fixed_and_timezone_free` 继续钉 UTC 固定换算。
