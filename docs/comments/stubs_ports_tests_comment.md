# stubs/ports.rs 决策记录（tests/stubs/）

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 测试桩按生产代码对待（AGENTS.md“注释纪律”节）；本文件承接自代码外迁的注释。

## MemoryConfigStore

- 内存版配置存储桩：密钥键值对 + 单份配置文档，可按需注入失败；同时实现 `ConfigStore` 与 `SecretStore`（2026-10-30 端口拆分后两个 impl 各答半边，跨包同名桩逐字一致）。
- 字段 `load_failure`：Some 时 `load` 直接返回它（模拟损坏的配置文件）。
- 字段 `save_failure`：Some 时 `save` 直接返回它（模拟落盘失败）。
- 字段 `secret_failure`：Some 时 `secret` 直接返回它（模拟 keychain 授权失败）。

## 失败注入方法

- `with_load_failure`：让后续 `load` 一律失败（配置文件损坏路径）。
- `with_save_failure`：让后续 `save` 一律失败（落盘失败路径）。
- `with_secret_failure`：让后续 `secret` 一律失败（keychain 读取失败路径）。

## 平台能力桩（2026-11-05，Windows 移植 P1 · 方案 D3）

- 新增平台能力端口桩（图标、菜单装配、剪贴板写入、权限探针）与整结构替身句柄（`stub_platform`）：应用壳以构造参数接收平台句柄后，测试驱动需要一份行为可预期的替身——菜单装配报告未装配（测试不经菜单链路）、剪贴板写入成功（复制动作无断言消费面）、权限恒放行、手势源缺席。
- 桩仍按包自持：与平台层占位实现的语义刻意不共享——本包只关心“应用层消费面”的行为，不关心平台占位的错误语义。
