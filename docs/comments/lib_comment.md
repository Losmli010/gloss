# lib.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。

## stubs 模块

- 测试桩只有一份源（tests/stubs/），经 `#[path]` 并入本 crate 的单测编译；不要另建副本。

## 按编译目标的模块分派（2026-11-05，Windows 移植 P0 §3.1）

- 平台实现按目录分派：`mod macos` / `mod windows` / `mod linux` 各带 `#[cfg(target_os…)]`，入口再把所选目录的子模块重新导出为平台无关路径（`pub use macos::{appearance, …}`）——消费方 `gloss_platform::<module>` 的 import 面不随目标变化，平台差异只在本文件与各级 mod.rs 的 `#[cfg]` 属性上，业务文件内部零 cfg。
- `ffi` 经 `pub(crate) use` 单独重导出：macos 内部 `pub(crate) mod ffi` 加包内重导出让迁入文件里的 `crate::ffi::…` 路径一字不改（FFI 唯一落点规则不变）。
- `live_test_support`（辅助功能授权快失败）随 macOS 目标门控（`all(test, target_os)`）：消费它的 live 测试全在 macos/ 目录内。
- linux/ 只留空占位且不重导出模块：该目标的可编译面到适配器库为止（见 linux_mod 记录）。
