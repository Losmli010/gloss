# keychain.rs 测试记录

> 按符号锚定（不写行号，防漂移）。改到相关测试时同步更新本文件。
> 测试代码禁止注释（AGENTS.md“注释纪律”节），测试的验收意图与辅助契约记录在此。

## 测试行为速览（BDD）

已统一迁往 docs/tests/bdd.md（git 追踪，以测试代码为唯一事实源）；本文件只留缘由与形态说明。

## 辅助

- test_store：测试专用服务名（与出厂条目隔离，条目随测随清）的 KeychainSecret。
- foreign_owned_entry_is_removed_by_reference_delete：外部所有者条目无法在本进程内伪造（所有权按创建二进制的代码签名归属），用 `security` CLI 预置真外部条目——CLI 的 partition 归属使查询式删除必被拒，而引用式删除应成功；存在性断言只走 `security find-generic-password`（只查属性不解密，不触发授权框），不打印其输出。
