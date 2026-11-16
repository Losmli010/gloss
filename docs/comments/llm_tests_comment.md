# llm.rs 测试记录

> 按符号锚定（不写行号，防漂移）。改到相关测试时同步更新本文件。
> 测试代码禁止注释（AGENTS.md“注释纪律”节），测试的验收意图与辅助契约记录在此。

## 测试行为速览（BDD）

已统一迁往 docs/tests/bdd.md（git 追踪，以测试代码为唯一事实源）；本文件只留缘由与形态说明。

## 辅助

- fixture：出厂配置快照 + 内存存储（core 的 test-util 桩，测试总线一致）；密钥相关用例都走出厂配置。
- 测试字节流：预置块逐块吐完，不模拟 pending（适配器 pending 分支由真实网络覆盖）。

## 思考控制上线形态（2026-11-16，评测空回复整改）

- thinking_control_rides_the_wire_only_when_disabled：`thinking` 的 wire
  语义——`Follow` 缺键（提供方默认不上线），`Disabled` 携带
  `{"type":"disabled"}`（提供方文档形态）。动机（为何只测键的有无）：思考
  链与正文挤占同一 token 预算是 live 评测空回复的根因，开关必须精确到
  “仅在关闭时上线”，多带或漏带都改变各端点行为。（原先同构的
  max_tokens 缺键断言随该字段 2026-11-16 撤出一并删除。）
