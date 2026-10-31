# report.rs 测试形态说明

## fixture_report 的两例判定（2026-10-12）

- `report_renders_the_table_shape` / `repair_rounds_render_with_violation_categories` 共用的夹具判定：第一例走 `TaskVerdict::for_rounds`（散文违约轮 → 契约轮的修复轨迹，末轮定完成态、违约轮进类别计数），第二例走 `for_reply`（单轮纯正文，降级）；前者钉“修复轮次”行非零形态，后者钉降级率与跳过数，零违约对照另建（修复轮次行只报总数）。
