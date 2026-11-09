# lib.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。

## stubs 模块

- 测试桩只有一份源（tests/stubs/），经 `#[path]` 并入本 crate 的单测编译；不要另建副本。

## 按编译目标的模块分派（2026-11-05，Windows 移植 P0 §3.1）

- 平台实现按目录分派：`mod macos` / `mod windows` / `mod linux` 各带 `#[cfg(target_os…)]`，入口再把所选目录的子模块重新导出为平台无关路径（`pub use macos::{appearance, …}`）——消费方 `gloss_platform::<module>` 的 import 面不随目标变化，平台差异只在本文件与各级 mod.rs 的 `#[cfg]` 属性上，业务文件内部零 cfg。
- `ffi` 经 `pub(crate) use` 单独重导出：macos 内部 `pub(crate) mod ffi` 加包内重导出让迁入文件里的 `crate::ffi::…` 路径一字不改（FFI 唯一落点规则不变）。
- `live_test_support`（辅助功能授权快失败）随 macOS 目标门控（`all(test, target_os)`）：消费它的 live 测试全在 macos/ 目录内。
- linux/ 只留空占位且不重导出模块：该目标的可编译面到适配器库为止（见 linux_mod 记录）。

## 装配契约模块（2026-11-05，Windows 移植 P1 · 方案 D3）

- 新增装配契约目录（bootstrap）的声明：只在系统（macOS）与系统（Windows）目标编译，公开为 `gloss_platform::bootstrap`——入口与应用层经它取平台装配句柄，本入口的平台模块再导出面（appearance、clipboard 等）保持不动，供平台内装配与测试引用。

## shortcut 模块随平台分派（2026-11-10，Windows 移植 P3b 后续）

- 新子模块 `shortcut`（桌面入口装配）随两侧平台目录声明并重新导出为平台无关路径，与既有能力模块同一套分派写法；系统（macOS）侧为恒无操作镜像。
