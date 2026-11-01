# report.rs（gloss-eval）决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 测试形态说明见 report.rs_tests_comment.md。

## EvalReport

- 一次迭代实跑的完整报告（`target/eval/<mode>.json` 的形状，也是
  history.jsonl 一行一份的历史行）。
- 字段：`mode` 运行模式（当前恒为 iterate）；`generated_at` 报告生成时间
  （UNIX 纪元秒，不引入时间库依赖）；`context` 采集上下文（历史行的
  upsert 键用）；`groups` 按类型展示名分组的聚合（键固定五类：分类、
  词卡、句译、代码、通用）；`totals` 全体用例的合计聚合；`latency` 延迟
  摘要（有实跑样本才有值）。
- 2026-11-01 评测工具链重构步骤 4：分组聚合的新指标模型落地（步骤 1
  删旧字段时预告的重建），历史行自此含分组聚合与标注快照。旧形状历史行
  （重放/旧实跑，缺分组聚合段）反序列化即失败，改由历史读取的坏行容错
  跳过——计划的既定口径是不做兼容垫片，故 `groups`/`totals` 不设 serde
  缺省。
- `LatencySummary` 字段按计划 §6.3 改名 `p50_ms`/`p95_ms`（毫秒）；趋势
  图延迟面板的取数点随之适配。曲线序列清单的唯一出口归步骤 5 的
  `trend` 重建，本步不动序列。

## EvalContext

- 采集上下文（可选；无此段照常解析为 None）。`commit` git 短 SHA（流水
  线经 GLOSS_EVAL_COMMIT 注入；本地运行缺省）；`source` 采集来源
  （GLOSS_EVAL_SOURCE 注入）。环境读取归流程装配层（步骤 6），装配层
  在 `assemble` 产物上回填。

## LatencySummary

- 延迟摘要（毫秒）——count、p50_ms、p95_ms。分位折算住执行编排的
  `Latency`（已有插值测试），聚合层只收折算结果。

## GroupStats

- 单类型组的聚合行，也是合计行的类型（合计=全体用例一次重算，不是组行
  相加——计数可加、比率与均值不可加）。
- `golden_count`/`label_count` 由资产映射按组内用例标识归并（检视页
  “基线列”悬停的前两个数）；`label_count` = 标注三态的 pass+fail。

## ContractStats

- 契约四态计数（经 `contract::derive_state` 单点派生，聚合层不另写判定
  逻辑）+ 违约实例总数（逐轮违约的累计和）+ 修复轮次均值。均值分母为
  组内全部用例：引擎故障用例 0 轮照常计入，否则跨组均值不可比。

## RuleCounts

- 回归五态计数。引擎故障用例无回归字段（回归恒待比但字段置空），不进
  任何档——故障数已在 `ContractStats.engine_error` 单列。

## LabelStats

- 该轮标注三态快照（合格、不合格、未标注）——随报告行入历史，标注合格
  率曲线的取数面；三态分母即组内全部（pass+fail+unlabeled = cases）。

## QualityStats / DimStat

- `coverage_rate`：已评用例 ÷ 组内全部用例（分类与跳过者计入分母）。
- `agree_rate`：人机一致率——标注结论与总体分同过合格线（合格且总体
  ≥80，或不合格且总体 <80）的用例数 ÷ 既有标注又有总体分的用例数。
  分母为零记 0（无一致率信号）；渲染层可凭标注数区分“无数据”与“零”。
- `dims` 维序固定：准确、完整、表达、呈现（与 `JudgeScores` 字段序一
  致）；`overall` 总体分 = 逐例四维均值，聚合层派生，评审模型不打总体
  分；总体维与单维同口径统计。
- `DimStat`：单维分布——count（组内已评数）、mean、五档分桶（0–59、
  60–69、70–79、80–89、90–100；边界 59|60、69|70、79|80、89|90 各归
  右档）与单维合格率（≥80）。比率一律存 0–1 小数，渲染按百分数。
- 质量聚合置空条件：组内无已评用例即 None。评审关闭（全组无分）与组内
  全为分类用例（评审资格天然排除分类）都落到这条；评审开启但组内全部
  因契约降级或故障跳过同样置空——空分布无均值可算，是计划两条置空条件
  的必然超集，避免了 count=0 的 0/0 均值。

## valid_rate / regression_pass_rate / label_pass_rate

- 三率的唯一出口（检视页指标表、柱状图与趋势序列共用）；分母恒为组内
  全部用例，待比、未标注、故障一律不剔除，保证跨轮可比。零例组记 0
  （`ratio` 单点防除零）。

## assemble

- 聚合唯一入口：按类型展示名（`EvalKind::display_name`）分组、合计行按
  全体用例一次重算。签名按计划 §6.4：cases + golden + labels + latency；
  `latency` 收 `Option<LatencySummary>` 而非 `execute::Latency`——依赖
  方向是 execute → report，聚合层不反向依赖执行编排，分位折算由调用方
  （执行或流程装配层）完成。
- `mode` 恒置 iterate（当前唯一实跑模式）、`generated_at` 取装配时刻
  （测试覆写定值）、`context` 留空由入口注入。

## render_markdown 及各渲染函数

- Markdown 报告：标题行（模式与 UTC 时间）+ 分类型指标表（每类型一行、
  合计行收尾）+ 延迟分位表（有样本才出）。表头中文，与语言门报告同风
  格；三率按百分数一位小数；质量聚合置空的类型三格显示 —。四维均分与
  逐例明细不进 CLI 表——四维归检视页指标表，逐例归明细文件。

## format_utc / format_utc_short / civil_from_days

- `format_utc`：UNIX 秒 → UTC 的 `YYYY-MM-DD HH:MM`。一律按 UTC 固定换
  算——跟随本地时区会让同一时间戳在不同机器漂一天。
- `format_utc_short`：UNIX 秒 → UTC 的 `MM-DD HH:MM`（趋势图 x 轴刻度
  用）。
- `civil_from_days`：UNIX 天数 → UTC 民用日期（Howard Hinnant 的
  civil_from_days 算法，零时间库依赖）。

## write

- 报告落盘 `report-dir/<mode>.json`（pretty JSON）。由 write_report 改
  名并去掉 .md 旁路输出与 Markdown 返回值——计划的迭代产物清单里
  Markdown 只经 report 命令走标准输出，不落盘；调用方是流程装配层
  （步骤 6 接线）。
