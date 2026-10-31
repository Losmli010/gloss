# prompt_repair.rs 测试记录

> 按符号锚定（不写行号，防漂移）。改到相关测试时同步更新本文件。
> 测试代码禁止注释（AGENTS.md“注释纪律”节），测试的验收意图与辅助契约记录在此；`// SAFETY:` 仍留在代码内。

## 模块的由来（2026-10-30，重构计划阶段 3 随主题迁移）

- 修复轮与批注行主题的十四条用例随 `prompt/repair.rs` 拆分迁入本
  文件，断言零改动，`bdd.md` 清单节随迁；用例的既有缘由随用例迁入
  本文件（原 `prompt_tests_comment.md` 期三一节，批注行四条原在
  降级批注方案落地时登记于此）。

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
- violation_briefs 四条（降级批注方案 §6）：行式同构与人读词注入、只载结构
  事实零残留占位符、封顶加注册表截断注、空清单恒空——照语言修复指令测试
  口径，注入词在测试内自备（位置词是界面文案，核心只做填充）。
- repair_instruction_renders_without_leftover_placeholders：修复指令的
  `json` 字样与占位符判定扩到细节全组合（模板清单的同一时期口径见
  `templates.rs` 测试记录）。
