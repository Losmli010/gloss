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
