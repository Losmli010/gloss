# detail.rs（gloss-eval）内联测试辅助契约

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），改名与合并用 `git mv` 保留历史；文件内不留指向本文件的指针。

- 2026-11-01 `case_detail_round_trips_through_serde` 用一份四层俱全的
  明细（逐轮轨迹、契约检查项、评审四维、回归五态）钉住序列化形状：
  类型化违约枚举、`EvalKind` 新类变体、snake_case 检查项与回归词面都
  必须在（JSON）里按默认命名往返——iterate.cases.json 的取数面由本测
  试锁定，检视页与报告层的字段假设坏掉先红。
- 2026-11-01 `eval_kind_display_names_cover_the_fixed_groups` 钉五键展
  示名（分类、词卡、句译、代码、通用）与 `task_kind` 拆解：报告分组
  键、评审提示词 `{{kind}}` 实参与筛选器文案共用这一份映射，改词必须
  显式过此测试。
- 2026-11-01 `label_verdict_wire_names_match_the_labels_file` 钉标注词
  面恰为 `pass`/`fail`（labels.jsonl 的 verdict 域，计划 §6.2）：导出
  与导入双向同构依赖这两个词面，词面漂移先红。
