# popup/mod.rs 测试记录

> 按符号锚定（不写行号，防漂移）。改到相关测试时同步更新本文件。
> 测试代码禁止注释（AGENTS.md「注释纪律」节），测试的验收意图与辅助契约记录在此。
> 命名说明：mod.rs 同名冲突带父目录前缀；本文件记录 `crates/gloss-app/src/ui/popup/mod.rs`
> 的 `#[cfg(test)]` 模块（kittest harness 测试），快照基线总览见 popup_tests_comment.md。

## repairing_streaming_view（步骤 5，契约校验与重试方案 §6，2026-10-12）

- 夹具形态：raw 恒为空串 + repairing=true——真实时序里 `accept_repairing`
  与清稿同帧发生（machine 侧同帧清累积并置位），修复中的流式卡正文不存在
  「带上一轮违约正文」的中间态；骨架条顶注位，页脚切「重新注解」。原夹具
  内两行注释出体至此。

## a_long_selection_keeps_the_footer_inside_the_window（2026-10-18）

- **夹具为何用英文原文**：kittest 的字体表无 CJK（见 `ui::context` 的
  `install_kittest_fonts`），中文会落成统一宽度的豆腐块，量宽与折行不代表
  真实排版；长选区测试要靠真实行数把内容顶到溢出，故夹具用英文句子重复。
  后续维护别「顺手改回中文」——那会让内容高缩水、溢出前提消失（实测同一
  长度的中文夹具只量到 ~277px，不足以触发滚动）。
- **两态夹具**：`TranslateSentence` 走宋体 `source_block`，`ExplainCode` 走
  代码面板（`Frame` + `set_width` + `break_anywhere` 折行），两条经位路径的
  高度行为不同，共用断言一次跑两态。
- **2× 钳制高的阈值**：断言的是「draw 上交的完整内容高」，不是视口高——
  若 `record_scrolled_height` 丢掉 overflow 项，长选区窗口就不再申请到壳侧
  钳制上限（实测阈值下 ~587px），测试必须为此变红。测量宽度钉在 `WIDTH`
  （首帧生产宽度），不依赖 kittest 的默认窗口尺寸。
- **↓ 滚动用 `run_steps` 而非 `run`**：流式页脚每 16ms 请求重绘，`run()`
  会因超过 kittest 默认 `max_steps=4` 直接 panic；滚动动画是固定步进的，
  8 步足够走满一个 `ARROW_SCROLL_STEP`，不依赖墙钟。

## `word_card_exposes_entries_to_accesskit`

- 疏条目经 markdown 渲染：段落文本是 TextRun 节点，精确标签查询会
  被标签提供者过滤规则吞掉——沿用注文的 contains 惯例。
