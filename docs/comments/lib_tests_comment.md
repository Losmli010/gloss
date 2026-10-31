# lib.rs（live_test_support）测试记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 测试代码禁止注释（AGENTS.md“注释纪律”节），测试的验收意图与辅助契约记录在此。

## 测试行为速览（BDD）

已统一迁往 docs/tests/bdd.md（git 追踪，以测试代码为唯一事实源）；本文件只留缘由与形态说明。

## require_accessibility

- 契约：未授权时以可操作的修复指引快速失败（含“系统设置 → 隐私与安全性 → 辅助功能 → 打开运行测试的终端 App”步骤），而不是让各测试以“注入被忽略 / tap 未建立”的间接形态超时。
- 背景：rdev 注入与 AX 读选区都要求**运行测试的进程**（通常是终端或 IDE）持有辅助功能授权；缺授权时系统表现为静默忽略。
- 实现内的 AXIsProcessTrusted 外部声明：AXError.h 的授权查询，签名须与 selection/accessibility.rs 内的声明一致；纯查询型 FFI 无前置条件（SAFETY 注释留在代码内）。
