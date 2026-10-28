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

## format_detail 矩阵（期三，2026-10-29）

- format_detail_maps_fence_truncation_and_non_object_shapes：围栏（含前后
  空白与围栏内原文不随行的红线断言）、合法非对象（标量、数组、`null`）、
  输入耗尽判截断、首字节语法错不判截断，四形态各归其键。
- format_detail_maps_missing_note_shapes_to_key_or_string_details：缺键与
  非字符串或空白分落两细节，不可解析原文无细节（校验器先于细节，细节函数
  自身防御）。
- format_detail_returns_none_for_undetailed_violations_and_legal_input：
  无细节违约与契约合法输入恒无细节——合法输入返回无细节是矩阵的收尾格。
- classify_category_names_match_the_validator_exactly：类别常量逐个过
  `validate_classify`，类别段渲染与校验器共用同一集合（一致性测试的
  分类一份）。
