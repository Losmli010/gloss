# quality.rs（gloss-eval）内联测试辅助契约

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），改名与合并用 `git mv` 保留历史；文件内不留指向本文件的指针。

- 2026-11-01 `judge_reply_parses_only_complete_in_range_dimensions` 钉
  严格整体解析：合法回复四维与理由齐收、0 与 100 两端含界；缺任一维、
  越上界、负分、超大数、非整数（85.5）、字符串分数、缺理由、非对象
  （JSON）数组、散文与空串一律 `None`——评审分数不做兜底猜测，评审
  覆盖率的分母靠它诚实（计划 §6.5）。
- 2026-11-01 `judge_prompt_renders_all_placeholders_and_keeps_input_as_
  user_turn` 钉渲染契约：四个占位符（输入、产出、参考答案、类型展示
  名）全部替换、系统消息不残留 `{{`、用户原文另作一轮用户消息——提示
  词模板与实参的接缝由本测试看住，改模板词面不影响它。
- 2026-11-01 `judge_eligibility_skips_classify_degraded_and_engine_
  errors` 钉打分前提：任务用例有效与软缺口可评（软缺口照评，结构化缺
  失计入完整性维度），分类、契约故障、契约降级一律不评——资格判定复
  用契约四态派生，判定语义漂移先红（计划 §6.5）。
