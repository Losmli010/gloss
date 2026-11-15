# prompt.rs 测试记录

> 按符号锚定（不写行号，防漂移）。改到相关测试时同步更新本文件。
> 测试代码禁止注释（AGENTS.md“注释纪律”节），测试的验收意图与辅助契约记录在此；`// SAFETY:` 仍留在代码内。

## 测试行为速览（BDD）

已统一迁往 docs/tests/bdd.md（git 追踪，以测试代码为唯一事实源）；本文件只留缘由与形态说明。

## 辅助

- text_task：按 kind/text/hint 构造 Task 的夹具助手。
- localized_task（2026-09-22）：在 text_task 上再指定 prompt_locale，省掉每个 locale 用例手工改 options。模板语言的用例都要它，因此单列一个助手而不是在用例里内联。

## 形态说明（2026-09-22，R1 模板文件化）

- 渲染器的用例（`render_template_*`）直接测私有函数：这几条锁的是“占位符怎么替换、哪一行会消失”这类渲染器自身的契约，走 `PromptRegistry::render` 只能间接看出结果，断言会绕远。
- 完整性用例（`every_locale_renders_without_leftover_placeholders`）刻意枚举 locale × kind × hint 三者的笛卡尔积：模板是文件资源、编译器看不见，唯一能在合入前发现“模板写了占位符而代码没喂值”的手段就是这条全枚举。新增 kind 或 locale 时要把它一并扩到新维度。
- locale 相关用例分三条（默认中文 / 英文模板 / 模板语言与目标语言解耦）：这三件事会各自被不同改动打破（改默认值、改英文模板、改目标语言解析），合成一条会在失败时看不出是哪一层。

## 模板重设计期一的锁点迁移（2026-10-29，prompt-template-redesign-plan §3、§8、§9 期一）

- `every_task_system_prompt_ends_with_the_shared_clause` 更名为
  `every_task_system_prompt_ends_with_the_discipline_after_the_shared_clause`：
  拼装从“任务书 + 目标语言条款”变为“任务书 + 目标语言条款 + 纪律块”，
  锁点从“条款垫底”改为“纪律块垫底、条款在其前”，两段各自 strip_suffix
  逐字锁定；覆盖四文本 kind 与图像任务，与旧行为同覆盖面。
- 关键词锁点随体例现代转写调整：`text_kinds_render_system_and_user_with_
  kind_content` 的 General 关键词从“经注疏”改“研究专家”（角色按任务
  功能命名，不携带传统名号）；`image_explain_renders_system_and_image_parts`
  的格式断言随 §3.8 全角括注改“（JSON）格式”。

## 期三组合与红线（提示词模板重设计方案 §8，2026-10-29）

- 修复轮与批注行的期三条目随主题迁往 `prompt_repair.rs` 测试记录；
  模板两条随主题迁往 `templates.rs` 测试记录。

- classify_prompt_composes_the_skeleton_and_the_registry_category_section：
  骨架逐字起头、类别段随后，类别定义迁出模板后的拼装锁点。2026-11-15 随
  判定程序改法扩锁：四类别逐条携带所属判定步骤（中英各一步骤词表）且
  步骤序与类别常量顺序一致——清单顺序即判定顺序，重排或漏定义当场报；
  骨架含模板示例节，逐字起头断言顺带锁住示例节在拼装里的位置。

- classify_prompt_carries_the_template_example_section（2026-11-15 新增）：
  示例节锁点——双语各有示例节标题、示例对逐字在系统指令里、无残留占位
  符；示例只说模型类别（`General` 不出现在示例段），例文与评测集用例互
  不复用的口径由评审守护，机器锁只守拼装形态。

- 2026-11-02 语言规范治本方案第二期随改：图像任务输出契约断言随中文模板
  迁移更新（JSON 格式，登记词裸用形态），契约语义不变。
