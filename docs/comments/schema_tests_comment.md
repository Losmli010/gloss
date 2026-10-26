# schema.rs 测试记录

> 按符号锚定（不写行号，防漂移）。改到相关测试时同步更新本文件。
> 测试代码禁止注释（AGENTS.md「注释纪律」节），测试的验收意图与辅助契约记录在此。

## 软约束测试的形态（契约校验步骤 3 评审，2026-10-11）

- `task_soft_rules_fall_back_field_by_field` 三段分别钉：structured 整体
  缺失/非对象回落空变体而 note 保留（硬约束只在 note）、变体内字段类型错当
  缺省且 null 等同缺省、kind 决定变体（清单外的疏证键——词卡键出现在句译
  任务——就地忽略）。
- 语义依据见 schema.rs_comment.md（校验分级）与 docs/tests/bdd.md 对应行
  （测试名称与场景的一览面）。
