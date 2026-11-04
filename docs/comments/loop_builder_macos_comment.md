# loop_builder_macos 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），改名与合并用 git mv 保留历史；文件内不留指向本文件的指针。

## 差异项收口（2026-11-05，Windows 移植 P0）

- winit 的 `with_default_menu(false)` 是 macOS 专属扩展方法，Windows 目标下方法不存在——差异收进本模块（`suppress_default_menu`），调用点（app/events.rs）保持零 cfg。模块声明在 app/mod.rs 用 `#[cfg] + #[path]` 双文件分派，cfg 落点符合“模块声明文件”边界。
- 只收口这一个差异项：事件循环构建的其余参数两平台一致，不为此立通用构建器抽象。
