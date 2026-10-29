# prompt.rs 测试记录

> 按符号锚定（不写行号，防漂移）。改到相关测试时同步更新本文件。
> 测试代码禁止注释（AGENTS.md「注释纪律」节），测试的验收意图与辅助契约记录在此；`// SAFETY:` 仍留在代码内。

## 测试行为速览（BDD）

已统一迁往 docs/tests/bdd.md（git 追踪，以测试代码为唯一事实源）；本文件只留缘由与形态说明。

## 辅助

- text_task：按 kind/text/hint 构造 Task 的夹具助手。
- localized_task（2026-09-22）：在 text_task 上再指定 prompt_locale，省掉每个 locale 用例手工改 options。模板语言的用例都要它，因此单列一个助手而不是在用例里内联。

## 形态说明（2026-09-22，R1 模板文件化）

- 渲染器的用例（`render_template_*`）直接测私有函数：这几条锁的是「占位符怎么替换、哪一行会消失」这类渲染器自身的契约，走 `PromptRegistry::render` 只能间接看出结果，断言会绕远。
- 完整性用例（`every_locale_renders_without_leftover_placeholders`）刻意枚举 locale × kind × hint 三者的笛卡尔积：模板是文件资源、编译器看不见，唯一能在合入前发现「模板写了占位符而代码没喂值」的手段就是这条全枚举。新增 kind 或 locale 时要把它一并扩到新维度。
- 文件内容的断言只取关键词（"translation assistant" / "词典助手" / 提示行标签），不做整段逐字比对：逐字比对会让每次改提示词都必须动 Rust 测试，与「模板文件改动不需要碰 Rust 代码」的验收直接冲突。
- locale 相关用例分三条（默认中文 / 英文模板 / 模板语言与目标语言解耦）：这三件事会各自被不同改动打破（改默认值、改英文模板、改目标语言解析），合成一条会在失败时看不出是哪一层。

## 占位符配对判据（契约校验步骤 3 评审，2026-10-11）

- `every_template_placeholder_is_declared` 的括号检查从「{{ 计数 == }}
  计数」改为「每个 {{ 之后存在 }}」（配对判据）：嵌套契约的模板示例 JSON
  以 `}}` 收尾，`}}` 计数天然可以多于 `{{`，计数相等从此是错误判据；未声明
  占位符的白名单检查不变。

## 模板重设计期一的锁点迁移（2026-10-29，prompt-template-redesign-plan §3、§8、§9 期一）

- `every_task_system_prompt_ends_with_the_shared_clause` 更名为
  `every_task_system_prompt_ends_with_the_discipline_after_the_shared_clause`：
  拼装从「任务书 + 目标语言条款」变为「任务书 + 目标语言条款 + 纪律块」，
  锁点从「条款垫底」改为「纪律块垫底、条款在其前」，两段各自 strip_suffix
  逐字锁定；覆盖四文本 kind 与图像任务，与旧行为同覆盖面。
- 新增 `task_templates_avoid_forbidden_legacy_markers`：§3.6 禁用标记清单落
  成单元测试——中文十二条（框架模式加训诂词簇）对 zh 五份、英文十二条
  （传统框架短语，前置试跑定稿）对 en 五份，比对统一转小写。放行边界：
  界面印章（经、注、疏）与字段名是产品语言不在受检面；「时间轴」等新造
  现代语汇不在清单——禁的是行话回潮，不禁新造语汇。
- 关键词锁点随体例现代转写调整：`text_kinds_render_system_and_user_with_
  kind_content` 的 General 关键词从「经注疏」改「研究专家」（角色按任务
  功能命名，不携带传统名号）；`image_explain_renders_system_and_image_parts`
  的格式断言随 §3.8 全角括注改「（JSON）格式」。
- `every_template_mentions_json` 与 `every_template_placeholder_is_declared`
  扩到 `discipline.md`：纪律块同为模板资产，端点 JSON 字样要求与占位符
  白名单同样适用。

## 期三组合与红线（提示词模板重设计方案 §8，2026-10-29）

- repair_instruction_composes_the_detail_wording_onto_the_skeleton：细节
  位置与动作上指令、行带违约规则标识、骨架齐备，红线断言扩到细节措辞
  （不含回复内容标记）。
- repair_instruction_without_detail_rides_the_violation_level_row：散文类
  `NotJson` 与无细节违约走违约级措辞行（本地化注册表行，不再是英文
  `Display` 直灌）。
- repair_row_falls_back_to_the_display_wording_when_the_registry_is_missing：
  骨架缺席与查键未中两分支都以纯函数驱动（生产数据触发不了，完整性测试
  在前）。
- language_repair_aggregates… / language_repair_caps_rows… /
  language_repair_within_the_cap… / language_repair_degrades…：聚合排序
  封顶矩阵与降级行，输入故意乱序以钉排序职责在组合层。
- classify_prompt_composes_the_skeleton_and_the_registry_category_section：
  骨架逐字起头、类别段随后，类别定义迁出模板后的拼装锁点。
- every_template_mentions_json / every_template_placeholder_is_declared /
  repair_instruction_renders_without_leftover_placeholders：模板清单缩至
  七份与八份，修复指令的 `json` 字样与占位符判定扩到细节全组合。
- violation_briefs 四条（降级批注方案 §6）：行式同构与人读词注入、只载结构
  事实零残留占位符、封顶加注册表截断注、空清单恒空——照语言修复指令测试
  口径，注入词在测试内自备（位置词是界面文案，核心只做填充）。
