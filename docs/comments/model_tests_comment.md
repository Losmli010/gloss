# model.rs 测试记录

> 按符号锚定（不写行号，防漂移）。改到相关测试时同步更新本文件。
> 测试代码禁止注释（AGENTS.md“注释纪律”节），测试的验收意图与辅助契约记录在此。

## 测试行为速览（BDD）

已统一迁往 docs/tests/bdd.md（git 追踪，以测试代码为唯一事实源）；本文件只留缘由与形态说明。

## error_debug_output_never_carries_the_payload（2026-11-11，架构重构 C6）

- 锁定 `Debug` 的安全契约：带载荷变体（`EngineResponse`、`Config`）调试格式化只出变体名与载荷长度，不回显载荷本身。与 `error_display_never_carries_the_payload` 成对——`Display` 是日志出口，`Debug` 是调试出口，两条出口各挂一条测试。
- 断言形态刻意用 `assert!(条件, "静态消息")` 而非 `assert_eq!`：后者失败时会打印实际渲染值，一旦回归（手写 `Debug` 被改回派生或改成转述载荷）就会把载荷泄进测试失败输出——与“敏感文本不进测试断言输出”同向。诊断代价（失败时不显示实际渲染内容）是刻意接受的取舍，定位时本地构造即可复现。
