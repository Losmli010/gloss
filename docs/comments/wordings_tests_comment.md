# wordings.rs 测试记录

> 按符号锚定（不写行号，防漂移）。改到相关测试时同步更新本文件。
> 测试代码禁止注释（AGENTS.md「注释纪律」节），测试的验收意图与辅助契约记录在此。

## 完整性与一致性双道（方案 §5.3，期三，2026-10-29）

- registry_frame_wording_is_present_for_both_locales /
  registry_covers_every_format_key_with_worded_rows /
  registry_covers_every_language_action_and_classify_definition：三区全覆盖
  锁「变体 × `locale` 键齐备且措辞非空」，缺行即失败。装载入口本身
  `expect` 解析成功，等于把「嵌入数据可解析」前置锁进每个测试。
- registry_keys_match_the_enums_exactly_in_both_directions：双向集合相等
  ——每键反解回枚举变体（多余键、拼写漂移即失败）加键数对账（缺键即失
  失败）。类别名区无枚举可反解，以校验器常量四标识对账。
- registry_carries_identical_key_sets_across_locales：跨 `locale` 键集合
  相等——措辞分语言、键空间不分语言，新增键漏翻一侧当场报。
- 降级道：missing_keys_and_broken_sources_degrade_to_none_with_a_trace 用
  未登记键与非法（TOML）源驱动缺席路径，断言返回空且警告留痕；生产数据
  触发不了这条路径（完整性测试在前），故直接以解析与查键入口驱动。
- 规则名区（降级批注方案 §3.4）：语言动作与分类定义覆盖测试、键与枚举双向
  相等测试、键集中英对齐测试同步扩到规则名区，缺键有痕降级测试补该区
  查键——扩枚举须同步两份表数据的锁点不变。
