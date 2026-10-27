# finalize.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关符号时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），文件内不留指向本文件的指针。

## ImageExplain 分支（2026-10-10，剪贴板图片方案 §5.5）

- 为什么 ImageExplain 从与 ImageOcr 同映 `Extracted` 拆出、改映 `ImageCommentary { interpretation }`：图像解读任务的产物契约是「注 = 内容描述（note，流式上屏）、疏 = 内容解读（interpretation，完成态逐条）」，与词卡的「注 = 义释、疏 = 例句」同构；OCR 的产物即经文本身（note 即提取文本），保持 `Extracted` 不动。拆分后 `ImageOcr` 仍走 `Extracted` 的既有断言（`json_main_path_covers_plain_and_extracted_kinds`、`both_layers_agree_on_the_two_layer_handoff`）顺势成为回归锁。
- 为什么疏证解析复用 `parse_examples` 而不新写：容错语义完全一致（非字符串坏条目跳过、字段缺失回退空表），一条规则一处实现；`interpretation` 只是换了字段名与落点。
- 为什么 `note` 缺失仍整路退围栏 fallback、全文落注位：三层降级（JSON → 围栏 → kind 兜底）是本模块的契约不变量——模型跑偏时产物必有、且无损保留原文（有痕迹降级，不判死）。此时模型已给的 interpretation 不抢救（它嵌在未按契约输出的原文里，全文进注位反而是可审计的痕迹），疏落空表由 UI 的「疏位空则不渲染」自然消化。
- 连带同步：`gloss-eval` 的 `metrics::mirror_complete`（自述「与 gloss-app finalize 同一条规则」的镜像，eval 只依赖 core，规则靠人肉同步）三处 `ImageOcr | ImageExplain` 合并臂同拆，`dataset::required_fields` 对 ImageExplain 改要求 `interpretation`（评测从严口径：prompt 模板自 #154 起把 interpretation 列进输出契约，必需键表随后）；不同步则未来出现 ImageExplain 夹具时评测会按 `Extracted` 误判。

## ImageOcr 臂删除与 both_layers_agree_on_the_two_layer_handoff 改词卡（2026-10-10，契约校验与重试方案步骤 1a）

- `parse_json_outcome` / `parse_structured`（含 fallback）的 `ImageOcr => Extracted` 三处臂随变体删除，图像侧只剩 `ImageExplain => ImageCommentary` 一条臂。
- `both_layers_agree_on_the_two_layer_handoff` 原用 `ImageOcr` 钉「坏 JSON + 坏围栏」三层衔接；变体删除后若改 `ImageExplain` 会与既有 `image_explain_falls_through_the_three_layers` 完全重复（同一输入同一断言），故改用 `TranslateWord`——文本 kind 的同一组合此前没有覆盖点，改后两测试互补不重叠（bdd 场景描述同步改写）。

## 围栏 fallback / keep-raw 整体删除，收敛为两分支

- `complete(kind, raw)` 现在只有两分支：`schema::validate_task` Ok → 正常产物；Err → 降级产物（note 空串 + 空变体 + `degraded: true`）。旧的 `finalize_outcome` / `parse_structured`（围栏剥离 + keep-raw 全文兜底）随本步删除——方案 §10/§7 定稿"旧围栏恢复不算降级"是靠"该路径整体删除后不存在"实现的，不是靠标记。
- **降级 warn 放在 finalize（app）而不是 schema（core）**：schema 是生产与 eval 共用的纯校验器，eval 重放逐条判定时打 warn 会淹没评测日志；"降级必有痕"的痕迹属于生产完成态这一侧（方案 §1.3"日志只记违约类别"）。步骤 5 的修复轮 warn 在 core 编排层，与本条不同面。
- warn 用 `log::capture`（线程局部）做测试断言而不是 `capture_global`：capture_global 不重建 tracing 兴趣缓存，全量并行跑时先于全局订阅者求值的调用点会被缓存成 never、整条收不到（`gloss_core::log::capture` 文档记载的坑）——全量套件里真实翻过一次车，改线程局部窗口后确定性成立。
- 降级产物的 `note` 恒为空串是**生产侧保证**；渲染层的降级卡对非空 note 也不渲染（kittest 夹具带残留内容钉住渲染侧防线，见 popup/mod.rs 记录）。两侧各自成立，不互为前提。
- `an_image_reply_lands_on_the_plain_variant` 等测试的断言面是 `OutcomeStructured` 相等比较——变体收敛后 ImageCommentary 消失，图像解读的断言归 Plain，行为面（注位 note + 疏位条目）不变。

## General 完成态与句译同形（2026-10-12，契约校验与重试方案步骤 4）

- `complete(General, …)` 无特判：校验与降级两分支都是 kind 无关的，General 落 Plain 变体；`a_general_reply_lands_on_the_plain_variant` 与图像那条对称，钉住「通用任务回复按 Plain 解析、不降级」。

## complete（完成态产物组装，注释迁出）

- 契约纯函数：校验过出正常产物，违约落降级产物。引擎失败的任务到不了这里，因此产物恒可回填缓存——但降级产物由调用方按 `degraded` 分流（不进缓存，见 `crate::runtime::pipeline`）。
