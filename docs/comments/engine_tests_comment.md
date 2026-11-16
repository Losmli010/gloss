# engine.rs 测试记录

> 按符号锚定（不写行号，防漂移）。改到相关测试时同步更新本文件。
> 测试代码禁止注释（AGENTS.md“注释纪律”节），测试的验收意图与辅助契约记录在此；`// SAFETY:` 仍留在代码内。

## 测试行为速览（BDD）

已统一迁往 docs/tests/bdd.md（git 追踪，以测试代码为唯一事实源）；本文件只留缘由与形态说明。

## 辅助

- make_service / text_task：构造 AiTaskService + MockEngine 计数对与文本 Task 的夹具助手。

- contract_invalid_direct_degrade_delivers_an_empty_violation_list：直调
  `run_lang_rounds` 喂契约失效回复，锁定直降路径空清单（生产路径该分支
  不可达，防御性兜底由纯函数驱动直测）；预算耗尽路径测试补断言已判得
  清单随 `RunOutput` 交付。

- 2026-11-02 语言规范治本方案第二期随改：修复指令措辞锁随措辞注册表迁移
  更新（裸 JSON 对象，登记词裸用形态），锁定语义不变——指令骑违约级注册
  表行。

- 2026-11-17 架构重构 C4 随改：新增三条 `run_fixed_task` 门面测试（逐轮原文按轮序交出并返回生产交付值、语言修复轮原文也在回调里、空白模型先于引擎拒绝），锁定“评测经门面进入任务段”的契约；`run_task_rounds` 直调测试与 `contract_invalid_direct_degrade` 直调 `run_lang_rounds` 的测试随新回调参数补 no-op 泵，锁定语义不变。
