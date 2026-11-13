# 测试行为清单（`gloss-eval`）

本文件登记评测工具链（`gloss-eval`）的测试行为；总纲、人工测试与发版人工步骤见 `bdd.md`。

## 单元测试

### `crates/gloss-eval/src/asset.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| missing_files_load_as_empty_maps | 资产文件缺失按空表 | 给定不存在标注与基线文件的目录，当 `load_labels` 与 `load_golden`，则各自得空表且不报错（首轮无历史是常态） | 2026-11-01 |
| upserts_merge_by_id_and_keep_lf_and_note_roundtrip | 按条目合并写确定性 | 给定先后两批含同标识的标注行与重复固化的基线行，当 `upsert_labels` 与 `upsert_golden`，则同标识覆盖不叠行、行以换行符结尾、按标识排序落盘、备注随往返保留、新固化时间胜出 | 2026-11-01 |
| bad_rows_and_unknown_verdicts_are_rejected_with_line_numbers | 坏行与未知道具拒绝 | 给定坏行、`verdict` 域外词面与缺逐轮数组的基线行，当加载，则各自带行号拒绝（资产是受控工件，不做历史行式容错） | 2026-11-01 |
| outputs_overwrite_on_write_and_fail_clear_on_missing_read | 单次输出覆盖写与缺失报错 | 给定写后读回、覆盖写空表与读缺失文件三步，当 `write_outputs` 与 `read_outputs`，则读回逐字段一致、空表读作零行、缺失报清晰错误并指明先跑迭代 | 2026-11-01 |

### `crates/gloss-eval/src/contract.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| task_settlement_flags_json_note_and_field_checks | 任务结算合法回复全绿 | 给定可解析且结构齐全的任务回复，当 `settle_case`，则可解析（JSON）、字段完整检查过、不降级、零违约零修复轮次、逐轮无违约 | 2026-11-01 |
| task_settlement_degrades_on_prose_and_missing_note | 任务结算违约即降级 | 给定散文、缺注与空白三类回复，当 `settle_case`，则末轮降级、违约类别（不可解析、注缺失、空回复）逐轮计数、散文不可解析（JSON）、字段完整未过 | 2026-11-01 |
| task_settlement_counts_repairs_and_violations_over_rounds | 任务结算逐轮还原轨迹 | 给定违约轮加合法轮的两轮轨迹，当 `settle_case`，则修复轮次为轮数减一、违约轮带类别、合法轮干净、末轮定全局不降级 | 2026-11-01 |
| fields_complete_requires_the_subtype_shape | 字段完整按子类形状 | 给定词卡缺音标、代码缺代码语言、句译例证齐、无结构化对象与音标键值为 `null` 的回复，当 `settle_case`，则缺子类必带键未过、齐者过、`null` 在场、例证空数组仍过（内容归评审） | 2026-11-01 |
| classify_settlement_separates_degradation_from_kind_mismatch | 分类结算降级与类别不符分账 | 给定类别正确、可解析但类别不符、越清单类别与散文四类回复，当 `settle_case`，则正确者类别检查过、不符者不降级只判类别未过（软缺口）、越清单与散文降级、`Fallback` 落通用任务 | 2026-11-01 |
| contract_four_state_derivation_matrix_is_mutually_exclusive | 契约四态派生矩阵互斥完备 | 给定引擎故障、降级、检查项未过与全绿四类明细，当 `derive_state`，则依次得故障、降级、软缺口、有效，且引擎故障压倒降级、降级压倒检查项 | 2026-11-01 |
| case_settlement_dispatches_by_the_case_subclass | 案例结算按子类分派 | 给定代码子类用例配词卡形回复与类型参考答案不自洽的分类用例，当 `settle_case`，则前者按代码形状判字段完整未过、后者按通用兜底且 `Fallback` 判类别符合（嵌入集自洽由加载校验保证） | 2026-11-01 |

### `crates/gloss-eval/src/dataset.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| curated_loads_with_the_finalized_distribution | 精选集五类分布守卫 | 给定编译期嵌入的定稿精选集，当 `load_curated`，则分类 22、词卡 10、句译 6、代码 3、通用 3 共 44 条逐类钉数且目标语言恒中文——增删条目是走评审的定稿变更，守卫数字随评审同改 | 2026-11-01 |
| every_embedded_reference_settles_green_against_its_own_case | 参考答案与契约自洽 | 给定全部嵌入用例，当参考答案渲染产物词面后过各自用例的 `settle_case`，则不降级且子类检查项（类别符合或字段完整）全过——参考答案形状漂移或契约口径收紧先红 | 2026-11-01 |
| reference_renders_as_the_product_wire_shape | 参考答案渲染生产词面 | 给定任务侧与分类侧参考答案，当 `reply_json`，则任务侧渲染结果过生产任务校验器（注与疏证在场）、分类侧恰为裸类别（JSON）回复形状且 `Fallback` 映射通用 | 2026-11-01 |
| reference_rejects_wrong_subtype_shapes_with_line_numbers | 参考答案按子类结构校验 | 给定分类拿任务形、任务拿分类形、空注、词卡缺音标键与 `null` 音标各一例，当 `parse_curated`，则前四者带行号拒绝、`null` 在场仍过（与契约检查同口径） | 2026-11-01 |
| loader_rejects_unknown_kinds_targets_and_expected | 类型语名与期望域校验 | 给定图像类型、未知目标语名与四名之外期望类别各一例，当 `parse_curated`，则分别以未知类型、未知语名与非四名期望带行号拒绝 | 2026-11-01 |
| loader_rejects_empty_or_duplicate_ids_and_bad_lines | 标识唯一性与坏行定位 | 给定空标识、重复标识、坏行、夹空行与空数据集，当 `parse_curated`，则空与重复标识带行号拒绝（重复指出首见行）、坏行带行号、空行跳过、空数据集硬错误 | 2026-11-01 |

### `crates/gloss-eval/src/detail.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| case_detail_round_trips_through_serde | 结算明细序列化往返 | 给定四层俱全（逐轮轨迹、契约检查项、评审四维、回归五态）的明细，当序列化再反序列化，则逐字段一致且类型化违约、`EvalKind` 变体与 `snake_case` 枚举词面按默认命名出现 | 2026-11-01 |
| eval_kind_display_names_cover_the_fixed_groups | 用例类型展示名覆盖五键 | 给定分类与四个任务子类，当 `display_name` 与 `task_kind`，则分类、词卡、句译、代码、通用五名齐备且任务子类拆出对应任务类型、分类无任务子类 | 2026-11-01 |
| label_verdict_wire_names_match_the_labels_file | 标注结论词面即标注文件域 | 给定合格与不合格两枚举值，当序列化与反序列化，则词面恰为 `pass` 与 `fail` 且往返一致（labels.jsonl 的 verdict 域） | 2026-11-01 |

### `crates/gloss-eval/src/execute.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| stub_run_settles_layers_and_pends_without_baseline | 桩引擎端到端首轮全待比 | 给定脚本引擎桩（分类回复、任务回复、评审四维脚本）与头部 23 条切片，当 `run` 加空基线空标注结算，则例数与延迟样本数对齐、分类例无质量分且类别符合过、首个任务例字段完整过且评审四维进明细、全部回归恒待比 | 2026-11-01 |
| golden_and_labels_settle_stub_rounds_into_pass_and_changed | 注入基线标注出通过与改 | 给定同轮输出的桩运行与逐例基线标注（两合格一同合格、一不合格异类），当 `regression::settle`，则同文合格判通过、结构相同判通过、异类不合格判改 | 2026-11-01 |
| engine_failure_marks_the_case_and_drops_settlement | 引擎故障标记进明细 | 给定首通即请求失败的脚本引擎桩，当 `run` 后结算，则明细带故障标记、轮轨迹为空、无质量分、回归字段恒空且运行不中断 | 2026-11-01 |
| limit_slices_the_run_and_judge_toggle_keeps_quality_empty | 限位切片与评审开关 | 给定限位 2 条且评审关闭的运行，当 `run`，则恰出两条明细与两个延迟样本、质量字段全空 | 2026-11-01 |
| latency_percentiles_interpolate | 延迟百分位线性插值 | 给定四个样本，当取 `p0/p50/p95/p100`，则插值结果精确；空样本返回 `None` | 2026-11-01 |

### `crates/gloss-eval/src/flow.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| stub_iterate_writes_artifacts_records_history_and_renders_the_view | 迭代装配端到端产出全件 | 给定单例脚本引擎桩与临时产物目录，当 `run_iterate`（评审关闭、限位 1），则报告文件按模式命名且模式为 `iterate`、注入的提交标识进报告、逐例明细解析出一条、单次输出恰一行且逐轮原文即脚本回复、历史行与趋势图落盘、检视页含该用例 | 2026-11-02 |
| engine_failures_settle_but_never_reach_the_outputs_file | 故障例结算不进固化输入 | 给定首通即请求失败的脚本引擎桩，当 `run_iterate`，则明细带故障标记且契约统计计一例故障、单次输出文件在场但零行——失败输出永不可固化 | 2026-11-02 |
| import_merges_known_rows_and_rejects_bad_documents | 标注导入按条目合并与坏档拒绝 | 给定含精选集内外各一行的导出文档，当 `import_labels`，则已知行合并进标注文件、集外标识跳过并回报；坏 JSON 文档与域外 `verdict` 各带文件名硬拒绝 | 2026-11-02 |
| accept_freezes_outputs_counts_unlabeled_and_fails_clear_without_inputs | 基线固化与未标注警告 | 给定两行单次输出与其中一行已标注的资产目录，当 `accept`，则两行固化进基线文件且固化时间为协调世界时（UTC）词面、汇总计未标注一行、重复固化按条目覆盖不叠行；缺单次输出文件时带补救指引硬报错 | 2026-11-02 |

### `crates/gloss-eval/src/gate.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| gate_reply_parsing_accepts_json_verdict_only | 评委复核回复解析 | 给定合法裁决（JSON）、越界类别与非（JSON）回复，当 `parse_gate_reply`，则合法者出裁决与类别、其余为 `None` | 2026-10-28 |
| gate_metrics_compute_precision_and_recall | 门禁指标算式对账 | 给定确定的命中、漏报（引文繁体豁免是内生漏报）与放行三类条目，当 `run_gate`，则条目级精确率与召回率按算式得出、类别级对账区分期望与命中 | 2026-10-28 |
| gate_loader_rejects_bad_expect_duplicate_ids_and_category_mismatch | 门禁数据集加载期校验 | 给定合法条目、非法期望值、重复标识、违约无类别、放行带类别与坏形状行，当 `load_gate_cases`，则合法者载入、其余带行号报错 | 2026-10-28 |
| gate_dataset_agrees_with_the_gate_offline | 门禁数据集与判定器离线全一致 | 给定内嵌门禁数据集（违约与放行各一批），当 `run_gate`，则条目级精确率与召回率均为 1、各类别期望与命中对账相等、标点类判定数钉在已知过报（1，3）——未来加码过报先红（数据集是受控工件，规则演进须同步改判例） | 2026-11-01 |

### `crates/gloss-eval/src/history.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| first_run_appends_one_line | 首写追加一行 | 给定空目录与一份无 `context` 的报告，当 `record_run`，则 `history.jsonl` 恰一行、`LF` 结尾、读回 `mode/generated_at` 一致且 `context` 缺省 | 2026-10-12 |
| same_mode_and_commit_overwrites_in_place | 同键原位覆盖不叠行 | 给定同 `(mode,` `commit)` 的两份报告，当先后 `record_run`，则文件恒一行且新报告的时间戳胜出 | 2026-10-12 |
| different_commit_or_mode_appends | 换 `commit`、换 `mode` 追加 | 给定同 `mode` 换 `commit` 与同 `commit` 换 `mode` 的报告，当先后 `record_run`，则各占一行（`mode` 是 `upsert` 键的一半） | 2026-10-12 |
| missing_commit_always_appends | 无 `commit` 恒追加 | 给定两份无 `context.commit` 的同 `mode` 报告，当先后 `record_run`，则叠两行（本地行永不参与匹配） | 2026-10-12 |
| blank_lines_are_skipped_on_load | 空行跳过 | 给定夹空行与尾空行的 `history.jsonl`，当 `load_history`，则空行不计、好行照常读回 | 2026-10-12 |
| corrupt_lines_are_skipped_and_warned | 坏行跳过且有 `stderr` 警告 | 给定好行加坏行加好行的 `history.jsonl`，当 `load_history`，则坏行带行号警告后跳过、两侧好行无恙（进程中断的半截尾行不炸全量） | 2026-10-12 |
| unterminated_final_line_is_readable | 无换行尾行可读 | 给定末行缺 `LF` 的 `history.jsonl`，当 `load_history`，则末行照常解析 | 2026-10-12 |
| record_run_redraws_history_svg | 记录后全量重绘趋势 | 给定一次 `record_run` 与随后一次换 `commit` 的追加，当各自完成，则 `history.svg` 存在且标题点数随全量历史更新 | 2026-10-12 |
| legacy_rows_without_aggregates_are_skipped_as_bad_rows | 旧形状行按坏行跳过 | 给定旧形状行（缺分组聚合段）与现形状行各一的历史文件，当 `load_history`，则旧行带行号警告后跳过、现形状行照常读回（趋势不炸全量） | 2026-11-01 |
| label_snapshot_rides_the_history_row | 标注快照入历史行 | 给定一条已标注合格的用例经 `assemble` 成报告并 `record_run`，当读回历史行，则行内标注三态为合格 1、未标注 0，加载后合计行的标注快照原样可读（标注合格率曲线的来源） | 2026-11-01 |
| context_roundtrips_and_bare_rows_default_to_none | `context` 段往返与裸行缺省 | 给定带 `context` 的报告行与无 `context` 段的报告（JSON），当写入读回、直接解析，则行内 `context` 段逐字节往返、裸行 `context` 为 `None` | 2026-10-13 |

### `crates/gloss-eval/src/main.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| default_eval_dir_has_no_target_path_segment | 默认评测产物目录不落构建目录 | 给定缺省目录常量（`GLOSS_EVAL_DIR` 覆盖缺席时的回落值），当按路径段检查，则不含 `target` 段——人工标注与回归基线不被 `cargo clean` 与流水线清理抹掉（E2 定案：受控工件默认目录移出 `target/`，覆盖通道保留） | 2026-11-11 |

### `crates/gloss-eval/src/quality.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| judge_reply_parses_only_complete_in_range_dimensions | 评审回复严格整体解析 | 给定合法回复与缺维、越界、负分、超大数、非整数、字符串分数、缺理由、数组、散文、空串等坏形，当 `parse_scores`，则合法者出四维与理由且 0 与 100 两端含界、坏形一律 `None` | 2026-11-01 |
| judge_prompt_renders_all_placeholders_and_keeps_input_as_user_turn | 评审提示词渲染契约 | 给定词卡用例与模型产出，当 `render_judge`，则系统消息四占位符（输入、产出、产物词面参考答案、类型展示名）全替换且不残留占位符标记，用户原文另作一轮用户消息 | 2026-11-01 |
| judge_eligibility_skips_classify_degraded_and_engine_errors | 评审资格跳过分类降级与故障 | 给定任务与分类用例、引擎故障、契约降级与软缺口各态，当 `judge_eligible`，则任务有效与软缺口可评，分类、契约故障、契约降级一律不可评 | 2026-11-01 |

### `crates/gloss-eval/src/regression.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| label_pass_passes_on_equal_and_regresses_on_mismatch | 标注合格分支判定 | 给定与基线相等和不等（例证多一条）的本轮输出，当标注合格结算，则相等出通过（结构化匹配真、无摘要）、不等出回归（带差异摘要与结构化匹配假） | 2026-11-01 |
| label_pass_with_degradation_regresses_without_structured_match | 标注合格遇契约降级先短路 | 给定契约降级的本轮输出与齐备的基线标注，当标注合格结算，则出回归、无结构化比较（匹配为空）、摘要说明降级无输出可比 | 2026-11-01 |
| label_fail_changes_on_mismatch_and_stays_unchanged_on_equal | 标注不合格分支判定 | 给定与基线相等和不等的本轮输出，当标注不合格结算，则不等出改（带音标差异摘要）、相等出未改善（无摘要） | 2026-11-01 |
| missing_baseline_or_label_pends_and_engine_error_clears_the_rule | 待比与故障置空 | 给定缺基线或缺标注的用例与引擎故障用例，当 `settle`，则缺者出待比（无匹配无摘要）、引擎故障回归字段恒置空 | 2026-11-01 |
| normalization_ignores_whitespace_and_example_order | 规范化消除空白与顺序噪声 | 给定音标带空白与例证乱序的本轮输出，当规范化比较，则与基线判一致；例证乱序两种输入规范化相等且保持词卡形态 | 2026-11-01 |
| classify_regression_compares_parsed_categories | 分类回归按类别比较 | 给定类别一致（`Fallback` 两侧）与类别不同（代码讲解对句译）的分类用例，当结算，则一致出通过、不同出改并带类别差异摘要 | 2026-11-01 |
| golden_baseline_rounds_concatenate_chunk_fragments | 基线末轮分片拼接 | 给定末轮分片拆半的基线与含违约历史轮的两轮基线，当 `settle`，则分片无分隔直连判过、多轮只取末轮判未改善 | 2026-11-01 |
| unparsable_baseline_or_output_is_incomparable_not_equal | 不可比三态边界 | 给定本轮降级与基线损坏两种不可比输入，当结算，则本轮降级出改且匹配为空摘要说明降级、基线损坏出回归且摘要说明基线不可解析 | 2026-11-01 |

### `crates/gloss-eval/src/remedy.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| remedy_fixtures_land_clean_after_mechanical_repair | 收敛夹具表全量落地复判零判罚 | 给定内嵌收敛夹具数据集（违规样本、按动作机械落地样本、复判三列），当 `run_remedy`，则违规列类别集合与判定器输出全等、落地样本同字段零判罚，收敛条目数与总条目数相等（动作收敛是判定器精度召回之外的第二个口径） | 2026-11-01 |
| remedy_loader_rejects_duplicates_empty_categories_and_identical_repair | 收敛数据集加载期校验 | 给定合法条目、重复标识、空类别列、落地样本与违规样本同文及坏形状行，当 `load_remedy_cases`，则合法者载入、其余带行号报错 | 2026-11-01 |

### `crates/gloss-eval/src/report.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| group_aggregates_fold_into_totals | 分组聚合与合计一致 | 给定横跨分类、词卡、句译三组的用例夹具（契约四态齐备、含违约计数与修复轮次、回归三态、基线与标注部分在场），当 `assemble`，则全部计数列（例数、基线数、标注三态、契约四态、违约总数、回归五态）逐组求和等于合计行、已评数同样可折，修复轮次均值在合计行按全体用例重算而非组均值再平均 | 2026-11-01 |
| rate_denominators_are_all_group_cases | 三率分母恒为组内全部 | 给定四条任务用例（契约有效、软缺口、降级、故障各一，其中一条回归通过、一条已标注合格），当 `assemble`，则契约有效率、回归通过率、标注合格率各为其分子除以组内全部四条——软缺口、待比、未标注、故障一律不剔除 | 2026-11-01 |
| bucket_edges_split_at_59_60_69_70_79_80_89_90 | 五档分桶边界 | 给定八条已评用例（准确性维分数为 59、60、69、70、79、80、89、90），当 `assemble`，则准确性维分桶为 `[1,2,2,2,1]`（0–59、60–69、70–79、80–89、90–100 五档，四对边界各归右档）、均分 74.5、合格率 0.375（79 不过线、80 及以上过线），无方差的其余维照常计入全部已评数 | 2026-11-01 |
| coverage_and_agreement_rates_use_their_own_denominators | 评审覆盖率与人机一致率口径 | 给定五条任务用例（四条已评且已标注，两条与总体分同过线、两条不同线；一条已标注未评），当 `assemble`，则评审覆盖率 0.8（已评四条除以组内全部五条）、人机一致率 0.5（同过线两条除以既有标注又有总体分的四条） | 2026-11-01 |
| quality_stays_empty_without_judged_cases | 质量聚合置空条件 | 给定评审关闭的任务组、全分类组与分类加已评任务的混合组，当 `assemble`，则前两组质量聚合为空、混合组仅分类组为空且任务组有值——组内无已评用例即置空 | 2026-11-01 |
| overall_score_is_the_four_dimension_mean_and_respects_the_pass_line | 总体分四维均值派生 | 给定两份四维分（90/80/70/60 与 80/80/80/80），当 `assemble`，则总体均分 77.5（逐例四维均值再取均值）、合格率 0.5（恰 80 过线、75 不过线）、分桶落 70–79 与 80–89 两档 | 2026-11-01 |
| markdown_renders_group_rows_and_totals | 报告渲染分组表 | 给定跨类型聚合后的报告，当 `render_markdown`，则标题带模式与（UTC）时间、每类型一行加合计行、三率按百分数渲染、无质量聚合的类型三格显示占位符、基线数与轮次均值各占一格 | 2026-11-01 |
| latency_only_shows_when_sampled | 延迟只在有样本时出现 | 给定无延迟样本与有延迟样本两份报告，当渲染，则前者不含延迟行、后者按行渲染 `p50/p95` 分位（毫秒，两位小数） | 2026-11-01 |
| generated_at_is_a_numeric_unix_timestamp | `generated_at` 为数值纪元秒 | 给定 `generated_at` 为整数秒的报告，当序列化、反序列化，则 JSON 中为裸数值且往返一致（旧 `unix <seconds>` 字符串格式已废弃，不兼容） | 2026-10-13 |
| utc_formatting_is_fixed_and_timezone_free | 协调世界时（UTC）格式固定换算 | 给定若干 `UNIX` 秒，当 `format_utc` 与 `format_utc_short`，则恒为 `YYYY-MM-DD HH:MM` 与 `MM-DD HH:MM` 的协调世界时（UTC）结果，不随本地时区漂移 | 2026-10-13 |
| legacy_rows_without_aggregates_fail_parsing_for_tolerant_skip | 旧历史行改走坏行容错 | 给定缺分组聚合段的旧历史行（JSON），当反序列化为报告，则解析失败（分组聚合无缺省值），由历史读取按坏行容错跳过、不做缺省兼容 | 2026-11-01 |
| report_round_trips_through_serde | 报告序列化往返 | 给定含分组聚合、质量聚合与延迟摘要的报告，当序列化、反序列化，则分组与合计逐字段一致、分位字段名带 `_ms` 后缀、质量聚合完整落行 | 2026-11-01 |
| write_lands_complete_report_without_temp_residue | 报告落盘完整且无临时残留 | 给定一份聚合报告与不存在的嵌套产物目录，当 `write`，则按模式命名的报告文件落盘（目录随写创建）、内容与 `pretty JSON` 逐字节一致且读回可解析（无截断尾）、目录内除报告外无点前缀临时残留 | 2026-11-12 |
| failed_write_reports_error_and_leaves_no_partial_artifact | 写回失败上抛不留半截产物 | 给定产物目录内报告同名的临时写盘路径被同名目录占位，当 `write`，则写回错误如实上抛而非安静成功、目标报告文件不产生（失败与中断都不留截断报告） | 2026-11-12 |

### `crates/gloss-eval/src/trend.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| empty_history_renders_note_without_series | 空历史容错 | 给定空历史与无延迟样本的单轮历史，当 `render_svg`，则前者零序列并落“暂无评测历史”占位、无最近一轮标注，后者不画延迟面 | 2026-11-01 |
| summary_panel_draws_the_three_rates_on_a_percent_scale | 指标汇总面绘三率 | 给定合计行为三率各一计数的迭代报告，当 `render_svg`，则指标汇总面在场，契约有效率、回归通过率、标注合格率三序列图例齐备，纵轴带 0%、25%、50%、75%、100% 步进网格，最新一轮末值以去尾零的百分数标注 | 2026-11-01 |
| model_series_rides_judged_runs_only | 模型线只随已评轮次出现 | 给定两轮已评、末轮未评与末轮已评三份历史，当 `render_svg`，则模型总体序列为虚线、末轮缺席时无末值标注、末轮在场时标注末值，全历史无已评轮次则整条模型线不画 | 2026-11-01 |
| latency_panel_draws_only_with_samples | 延迟面只随样本出现 | 给定含延迟样本的报告，当 `render_svg`，则延迟面在场（`p50` 与 `p95` 序列与图例齐备）且最新末值标注 | 2026-11-01 |
| missing_samples_break_the_series_instead_of_inventing_points | 缺样本断线跳段不造点 | 给定首点带延迟样本、次点无样本的历史，当 `render_svg`，则圆点只落在有值的序列点位，无样本点不补点不连线 | 2026-11-01 |
| newest_point_is_enlarged_and_labeled | 最新一轮放大并标注末值 | 给定两轮三率齐备的历史，当 `render_svg`，则最新一轮每条在场序列画放大圆点（半径 4）并各带末值标注 | 2026-11-01 |
| only_recent_five_iterate_runs_are_drawn | 近五轮窗口 | 给定七轮带不同提交标识的迭代历史，当 `render_svg`，则标题计五轮、最近一轮的提交标识进悬浮、窗口外最旧一轮不落图 | 2026-11-01 |
| non_iterate_rows_stay_out_of_the_window | 非 `iterate` 行不进图 | 给定 `iterate` 与其他模式行交错的新形状历史，当 `render_svg`，则窗口只数 `iterate` 轮，其他模式既不绘制也不进标注 | 2026-11-01 |
| latency_axis_picks_a_quarter_divisible_ceiling | 延迟轴四等分落整 | 给定 95 分位 1500 毫秒的样本，当 `render_svg`，则延迟轴上界取 1600 毫秒，网格刻度落 0、400、800、1200、1600 毫秒并按秒、毫秒自适应标注 | 2026-11-01 |
| text_is_xml_escaped | 文本 `XML` 转义 | 给定提交标识含 `&`、尖括号的报告，当 `render_svg`，则悬浮与“最近一轮”标注中的提交标识以实体出现、原字符不落进文本节点 | 2026-11-01 |
| rendering_is_byte_deterministic | 渲染确定性 | 给定同一份历史，当 `render_svg` 两次，则输出逐字节一致（`SVG` 内无渲染时刻的时间戳无环境读取） | 2026-11-01 |

### `crates/gloss-eval/src/view/cases.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| worst_first_orders_failures_then_badges_then_scores | 最差优先排序 | 给定八条用例（引擎故障、契约降级、回归、待复核、未改善、最低维分 60、最低维分 50、未评，按用例集序打乱排列），当渲染测试用例区，则输出次序为引擎故障、降级、回归、待复核、未改善、低分已评、高分已评、未评 | 2026-11-01 |
| badges_cover_every_contract_and_regression_state | 徽标四态五态与逐态注释 | 给定契约四态与回归五态各一代表的用例组，当渲染，则四色徽标逐态出现且悬停带逐态注释，待比无徽标（仅契约徽标两处），图例覆盖三轴全部枚举 | 2026-11-01 |
| a_pending_case_wears_no_regression_badge | 待比无徽标 | 给定契约有效而回归待比的用例，当渲染，则回归列无徽标（全页仅契约徽标出现在列表与结算两处）且 `data-regression` 落 `pending` 筛选键 | 2026-11-01 |
| expanded_body_follows_the_status_matrix | 展开态随状态矩阵 | 给定分类判定不符、契约降级、引擎故障、新增无基线、有基线无标注五条用例，当渲染，则分类显示“未评分——分类用例不打质量分”与期望类别参考栏、判定不符契约行转琥珀、降级显示未收敛轮数与违约计数并跳过评分、故障显示请求失败说明且恒待比、无基线塌两栏并提示尚未固化基线、有基线无标注提示等待裁决 | 2026-11-01 |
| scored_cases_show_dimensions_overall_and_reason | 已评用例的评分行 | 给定四维分 90、80、70、60 的用例，当渲染，则评分行列出四维分、总体 75（四维均值）与理由，列表列显示总体分 | 2026-11-01 |
| marks_prefill_from_labels_and_drive_the_export_count | 标注预填与导出计数 | 给定三条用例（合格带备注、不合格、未标注）与对应标注资产，当渲染，则前两条按钮按结论预填、备注回填输入框，导出计数等于已标注例数 2 | 2026-11-01 |
| rounds_carry_violation_tags | 逐轮轨迹与违约标签 | 给定两轮轨迹（首轮违约）的用例，当渲染，则轨迹折叠计数为两轮、违约轮带变体名标签、各轮原文照录 | 2026-11-01 |

### `crates/gloss-eval/src/view/mod.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| page_assembles_the_four_regions | 四区结构与零外部资源 | 给定单用例的检视页输入，当 `render_html`，则页面以 `<!doctype html>` 开头，顶栏、指标总览、历史曲线（含 `SVG`）、测试用例四区齐备，导出按钮与筛选器在场，除 `SVG` 命名空间外无外链资源（无 `link`、`@import`、`url` 与外源 `src`） | 2026-11-01 |
| foreign_text_is_escaped_everywhere | 外来文本转义 | 给定输入含脚本标签、逐轮原文含闭合标签与图片标签的用例，当 `render_html`，则脚本载荷不落原文、以实体出现在列表与逐轮轨迹中，闭合标签无法截断页面结构 | 2026-11-01 |
| export_document_matches_the_import_schema | 导出导入同构往返 | 给定一行标注行（`JSON`），当以 `labels` 键包裹成导出文档再反序列化，则行内容原样读回（标识、结论、备注逐字段一致）；当渲染检视页，则内联脚本以同结构构造 `{id, verdict, note}` 行并包 `labels` 键下载 | 2026-11-01 |

### `crates/gloss-eval/src/view/overview.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| bars_render_present_groups_totals_and_zero_stubs | 柱状图分组与零值短桩 | 给定分类软缺口与词卡已评各一的报告，当渲染指标总览，则在场的组按固定次序出柱组、合计组带虚线分隔，零比率画 2 像素短桩、满比率画满柱、模型柱用中灰并标注组均分 | 2026-11-01 |
| model_column_holds_a_placeholder_without_quality | 无质量聚合不画模型柱 | 给定仅分类组的报告，当渲染指标总览，则模型列只落占位符、无柱 | 2026-11-01 |
| table_cells_carry_hovers_placeholders_and_baseline_time | 指标表悬停与占位符 | 给定分类与已评词卡各一（词卡带基线与标注）的报告，当渲染指标总览，则三率格悬停带状态计数、模型格悬停带四维均分、评审覆盖悬停带已评数与例数、基线格悬停带基线数、标注数与固化时间，质量聚合缺席的组七格全占位 | 2026-11-01 |
| totals_row_folds_the_groups_under_a_dashed_rule | 合计行折叠分组 | 给定单组报告，当渲染指标总览，则合计行在虚线之下、组计数折进合计格 | 2026-11-01 |
