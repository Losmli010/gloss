# templates 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），改名与合并用 git mv 保留历史；文件内不留指向本文件的指针。

## 模块的由来（2026-10-30，重构计划阶段 3 自 prompt.rs 拆出）

- 重构计划阶段 3 给触及五百行上限的 `prompt.rs` 按职责拆分：模板资产
  嵌入（`Templates` 与 `include_str!` 面）整段迁入本文件；模板文件原文
  一字不动（计划决策 D5），只随落位调整 `include_str!` 的相对路径。
  模板内容的既有决策随代码迁入本文件（原 `prompt_comment.md` 各节）。

## Templates 与 Locale::templates（源码注释迁入）

- `Templates`：一个 locale 的模板文件集——各 kind 的自包含任务书（说明 + 输出契约与示例都在文件内）+ 共享目标语言条款 + 共享输出纪律块 + 分类指令 + 修复轮纠错指令。
- `Locale::templates`：本 locale 的模板文件集（编译期嵌入，无 IO）。

### 共享目标语言条款与语言修复模板（2026-10-28，输出语言体例门禁设计 §7）

- 五份任务模板的「目标语言与体例」节提为共享片段（设计建议的实现路径）：`prompts/{zh,en}/target_lang.md` 编译期嵌入，`render_task_system` 在任务书渲染后拼接——四条共同条款单点维护，五任务逐字一致由 `every_task_system_prompt_ends_with_the_shared_clause` 锁定。任务模板文件里不再有该节。
- 新模板 `repair_language.md`（中英两份，§7.6 原文 + 英译）：`{{violations}}` 单值注入行式清单（判定器渲染行，模板不管类别到动作的映射——映射在 violation.rs 单点维护）。
- 词卡字段规则例句与设计 §7.1 的偏差（protected 例句自相矛盾）取规范句「受保护的（protected）」，来龙去脉见 lang_gate_comment.md。
- 五份任务书按设计 §7 v2 全文替换：书证四段式（出处、双引号原文、括号译、按语）、句子短语级括注、代码围栏闭合与裸代码禁令、通用 markdown 全量、音标字符集约束。

## 模板文件化（R1，2026-09-22）

- 为什么把三个模板从 `format!` 里的内嵌字符串挪到文件：老形态改一句提示词就要改 Rust 并重编译，且模板与代码挤在同一个 match 里（三份散文 + 一份契约散文），改一处得读整段函数。任务卡 R1 的原话是「模板文件改动不需要碰 Rust 代码」。
- 为什么目录落在 `crates/gloss-core/prompts/{zh,en}/` 而不是仓库根：`include_str!` 的路径相对**当前文件**解析（不是 crate 根），放根目录要写 `../../../prompts/...`，路径越长越脆；放消费它的 crate 里最短、也最容易被改模板的人找到。
- 为什么每个 locale 是 5 份文件（三个 kind 指令 + `contract.md` + `hints.md`）：契约散文与提示行是三个 kind 共用的，抄成三份就是三处漂移源（改契约漏一处）。文件数从 3 变 5，换来的是每类文本各有一份单点。
- 备选与取舍：
  - `rust-embed`：能在 debug 期读盘（改模板免重编译），代价是一个新依赖 + 一套 folder/prefix 配置。本项要的是「编译期嵌入」这个已定决策，std 的 `include_str!` 就够（还顺带在编译期校验 UTF-8）。真要热重载，那是配置化模板的独立话题。
  - mustache / handlebars 模板引擎：为 5 个占位符引入引擎（section、lambda、HTML 转义语义）不划算；且它们的「只对 section/comment 生效」的 standalone 行规则**不表达**本项要的「可选行」语义——照搬会得到一个看似规范、实则不匹配的模型。
  - `format!` + 转义：占位符自身就是花括号，`format!` 的 `{}` 插值语法要求模板把每个 `{{target}}` 写成 `{{{{target}}}}`，模板里满是转义、改一句提示词就得数括号，直接否掉。
- 为什么 `contract.md` 里用 `{{fence}}` 而不是直接写围栏字面量：围栏标记是代码与解析器共用的事实源（`STRUCTURED_FENCE`），写死在模板文件里就等于开了第二处，改一处忘一处会让正文剥离静默失效。

## image_explain 模板（2026-10-10）

- 为什么单请求 JSON 双字段（`note` 在前、`interpretation` 数组在后）：描述与解读一次往返拿到；字段次序固定 note 在前，与流式提取器「note 值闭合即可渐进上屏、interpretation 完成态落」的顺序对齐。两请求链式（描述 → 解读）双倍延迟与成本，落选。
- 示例值恒为占位（`"…"`）：既有少样本偏置取舍——写死具体示例，模型照抄示例内容的比例随显著性上升。
- `PromptRegistry::render` 的 Image 分支：ImageExplain 走 image_explain 模板 + 用户消息 `Parts([ImagePng])`；ImageOcr 无模板仍报 `UnsupportedModality`（OCR 随其独立特性落地）；文本分支行为零改动。

## 任务模板整体嵌套化（契约校验步骤 3，2026-10-11）

- 步骤条目只点名「image_explain.md 改嵌套改词」，但 §3 的三行同构表对
  **全部 kind** 生效：词卡/句译/代码模板若留在扁平示例，模型照模板输出
  扁平 JSON，`structured` 恒缺失 → 软回落空变体 → 词卡永远没有音标/例句
  ——功能回归，不是风格问题。因此四份任务模板 × 2 locale 全部改为
  `{"note":"…","structured":{…}}`，image_explain.md 另做 interpretation →
  examples 改词。
- 模板示例行与 `schema::validate_task` 的唯一性关系（「示例就是解析侧所吃
  形状的唯一描述」）由此获得门禁：`every_task_template_example_passes_
  validate_task` 锁三件事——模板含 json 字样（端点 JSON 模式读它）、示例
  对象过 validate_task、structured 内键齐 per kind。扁平示例也能过
  validate_task（硬约束只有 note），所以「structured 在场且键齐」的断言
  不能省。
- `every_template_placeholder_is_declared` 的括号检查从「{{ 计数 == }}
  计数」改为「每个 {{ 有闭合」：嵌套示例 JSON 以 `}}` 收尾，闭合计数天然
  多一——配对成立即可，计数相等从此是错误判据。

## general.md 任务书（2026-10-12，契约校验与重试方案 §8 / 步骤 4）

- 双 locale 各一份三行模板，与句译/代码同构（指令行 + 纯 JSON 契约行 + 中性占位示例行）；`{{target}}` 是唯一占位符——通用任务同样以目标语言书写回应。
- 任务书刻意不设判别性引导（不偏向「翻译」或「解释」）：它是分类失败后的兜底落点，任务书越中性，兜底产物越能如实回应用户给的任意内容；示例行与句译/图像同形（`{"note":"…","structured":{"examples":["…","…"]}}`），值恒占位（少样本偏置取舍同全仓）。
- `classify_allowed_block` 补 General 臂只为穷尽匹配（该 match 无通配臂，编译器强制）；生产路径 `allowed` 恒传 `CLASSIFY_KINDS`，General 的描述行永远渲染不出来。`classify_rules_block` 不加显式臂：`_ => None` 通配已表达「清单外 kind 不产生规则行」，再写一条 `(General, _) => None` 是重复信息。
- 方案 §8 改动面的「任务标签 i18n 各加一条」在当前仓库**无落点**：按 kind 的任务标签（页头药丸 + `ui::kind_label` + 对应词条）已随经注疏落地有意删除（见 popup.rs 决策记录 2026-09-30「任务药丸与头部 spinner 的删除」——「任务由内容层（经注疏印章）自明，头部只剩图标 + 动作区」），i18n 表里不存在可扩展的 per-kind 词条集；新增无消费者的 `Text` 字段会被 dead_code 门禁拦下。故本步不加词条，任务类型经产物内容自明。若后续恢复任务标签设计，应随新设计一并补全全部 per-kind 词条，而不是只补 General 一条。

## STRUCTURED_FENCE 删除与模板完整性门禁收口（2026-10-12，契约校验与重试方案步骤 6 / §10）

- `STRUCTURED_FENCE` 常量整体删除：它的两个消费者（完成态围栏 fallback、分类围栏容错提取）分属步骤 3 与步骤 6 删除，常量随之失去存在理由——R1 时代「`{{fence}}` 单一事实源」的取舍针对的是「正文剥离解析器与模板共用标记」，那套围栏契约退役后，`every_locale_renders_without_leftover_placeholders` 里的防回归断言改用字面量 `"```gloss"`（断言保留：模板正文永远不该出现旧围栏标记）。
- 「所有模板必含 json 字样」从 `every_task_template_example_passes_validate_task` 拆出为独立的 `every_template_mentions_json`，并从五份任务模板扩到全部七份（+ classify.md + repair.md）：§3 的口径是「所有模板」——端点的 JSON 模式读的是**当次请求**的完整提示，分类请求与修复指令同样是 JSON 契约请求，同样需要 json 字样；塞在 validate_task 测试里名实不符也盖不住后两者。一个测试一个行为，bdd 各自登记。
- `repair_instruction_renders_without_leftover_placeholders` 补 repair.md 的渲染门禁：全组合扫（locale × 四类违约 × 有无截断提示 × 三种契约行，含空契约行的可选行删除路径）。此前 repair.md 的无残留断言只散在措辞测试的四个组合里；general.md 与任务模板的同一门禁由 `every_locale_renders_without_leftover_placeholders`（全量渲染）承担，两者合起来覆盖 §11「repair.md/general.md 渲染无残留占位符」。

## 模板重设计期一（2026-10-29，prompt-template-redesign-plan §3.1–§3.8、§9 期一）

- **五段骨架归一**：五份任务模板统一为「角色与任务 → 输出形状 → 字段规则 → 任务约定」四段（输出纪律块在拼装层垫底），标题层级统一为三级；general.md 原以一级标题开头、JSON Schema 用二级的层级漂移一并归一。代码的评价维度与图像的观察方法按 §3.1 归入任务约定段；词卡例证的槽位骨架按 §3.2 的定稿示例留在 `examples` 的形态要素里——§3.2 给的是近乎逐字的核心改写示例，槽位骨架与四要素在那里是一个整体，§3.1 的括注按概览读。
- **纪律块**：新增 `{zh,en}/discipline.md`，全部任务模板逐字共享（除 `{{target}}`）。§3.4 示例文本落盘时按 §3.8 与决策 12 落了两处合规点：zh 的裸 JSON 改全角括注（JSON）（JSON 子串仍在，端点要求不受影响）、`null`/`[]` 加反引号（代码字面量走既有反引号豁免，为期二收编铺路）。
- **拼装顺序**：`render_task_system` 在目标语言条款之后接纪律块（任务书 → 目标语言条款 → 纪律块），锁点测试随更名与改写（见 prompt_tests_comment.md）。
- **体例现代转写**：核心体例段整体删除，结构要求以直陈功能语汇并入字段规则——分层释义、举证与简评、异说辨于释义列于例证、「用现代语言写作」进任务约定；语源（词义的时间轴）进词卡与通用，典源（典故的时间轴）进词卡、通用与句译同语例证（决策 14 的适用面）。禁用标记清单（中英各十二条）落成单元测试，英文清单按前置试跑定稿采用（决策 13）。
- **翻译决策规则**（§3.7，决策 15 的落位）：译名判定表（含禁则）落词卡与句译 `note` 的语义要素；留种条款落词卡 `examples`——句译异语时例证恒空、无落点，不硬塞；译文尺度三行落句译 `note` 字段规则。
- **出口句式统一**：「无 X 时用 Y，不要猜测」——phonetic 用 `null`、examples 用 `[]`、code_language 用 `"unknown"`；词卡 `note` 补防杜撰出口（如实写未确证，不留空，`note` 仍是硬约束）。
- **参数定稿**（§3.5）：note 长度 120 字（英文 240 字符）落词卡、句译、代码、图像；通用 note 不设上限（模板不陈述上限即无上限）。examples「无把握时 1 条；至多 5 条」只落在原「一般 1–3 条」所在的词卡、句译、通用——代码与图像的例证数量本无上限，§3.5 该行的缘由列写明是那条弱措辞的判定化，不外溢。
- **措辞合规**（§3.8）：zh 区专有名词全角括注（JSON）（markdown），占位符 `{{target}}` 不加反引号，JSON 字面量与字段名走反引号豁免；「疏证」「书证」「按语」等训诂词簇换为现代编辑语汇（例证、引文、出处、简评）。词卡 note 的括注例句「受保护的（protected）」保留——它是形态示例，前置试跑已把它登记为期二收编时的待判点。
- **范围边界**：模板目录本期仍豁免语言规范门禁（收编在期二）；`repair.md`、`repair_language.md` 与格式修复轮措辞未动（期三）；classify.md 未动（期三注册表渲染）。en 词卡形态要素里的全角括注示例（original word）保留原文——它是英文区路由的已知残余，期二定路由时处置。
