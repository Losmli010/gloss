# stubs/ports.rs 决策记录（tests/stubs/ports.rs）

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> gloss-core 的同名桩见 ports_comment.md；本文件是它的子集（只留本 crate 用到的），同名桩逐字一致。

## MemoryConfigStore

- 内存版配置存储桩：密钥键值对 + 单份配置文档，可按需注入失败。
- `load_failure` 字段：`Some` 时 `load` 直接返回它（模拟损坏的配置文件）。
- `save_failure` 字段：`Some` 时 `save` 直接返回它（模拟落盘失败）。
- `secret_failure` 字段：`Some` 时 `secret` 直接返回它（模拟 keychain 授权失败）。
- `with_load_failure`：让后续 `load` 一律失败（配置文件损坏路径）。
- `with_save_failure`：让后续 `save` 一律失败（落盘失败路径）。
- `with_secret_failure`：让后续 `secret` 一律失败（keychain 读取失败路径）。

## StubSceneProbe

- 场景探针桩：事实可预置、可中途改写，默认「无事实」（两道场景闸门都放行）——测试不碰真机的安全输入态与前台应用。
- `with_facts`：预置场景事实。
- `set_facts`：改写当前事实（模拟两次触发之间的场景变化）。
