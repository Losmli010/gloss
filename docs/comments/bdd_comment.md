
- 2026-10-28 随语言体例门新增 30 条测试登记（核心 lang_gate 19、engine 3、prompt 2、pipeline 1、popup 1、eval gate 4），`popup_degraded_outcome` 夹具改空注真实形态、`popup_unverified_outcome` 基线入册（沿革见浮层基线行）。总览计数：单元 643 → 672、快照 38 → 39。

## 按包拆分（2026-11-14，方案 F1）

- 登记文件按包落位：根 `bdd.md` 留总纲（组织段改写为按包落位的口径，总览节升级为 `just test-index` 生成的类别×包计数矩阵加登记文件行数）、人工测试与发版人工步骤；包清单 `gloss-core.md`、`gloss-platform.md`、`gloss-app.md`（集成加快照加单元三类）、`gloss-eval.md`、`gloss.md`（单元加性能自检）各登记本包条目。拆分点前的逐条沿革靠条目自带的更新时间与“最新沿革见”指针承载，见版本库历史。
- 单元测试章内逐源文件小节按源路径稳定排序（同源文件多小节相邻，如 `flow/task.rs` 的预热发送端组），给后续登记确定的插入点、缩小合并冲突面；快照章保持整表原节不按限定符重排。
- 顺带修正既有漂移：15 个小节标题指向平台层 `macos/` 化之前的旧路径，标题改写为现路径；四处登记行搬回定义文件——`config_handle.rs` 小节的 3 行回 `domain/config.rs`、`ui/context.rs` 小节的字体行回 `ui/fonts.rs`、菜单 2 行建平台 `macos/menu.rs` 新小节、`classify.rs` 小节的集成测试行（定义在 `gloss-app/tests/pipeline.rs`）回 gloss-app 的集成章表格。总览计数自此机器生成，旧总览表四处手工失真（人工、发版、集成、快照）一并消除。
