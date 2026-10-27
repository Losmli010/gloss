# gloss-core/Cargo.toml 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关依赖时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），文件内不留指向本文件的指针。

## arc-swap

- 2026-09-18 引入出处 M4-T3（配置热更新）。

## tokio（optional，test-util）

- 2026-09-18 引入出处 M3-T7（mock 引擎的流间延迟）。

## tracing 三件套（特性取舍）

- 2026-09-18 tracing 留 std：关掉会退化成 no-op。
- 2026-09-18 不留 attributes：项目不用 `#[instrument]`（过程宏无法跨 crate 转发，见 log.rs 模块文档），顺带省掉 tracing-attributes 一支编译。
- 2026-09-18 tracing-appender 无特性可关，照写 `default-features = false` 保持「关默认集」写法统一。
- 2026-09-18 tracing-subscriber 的 tracing-log 必须留：GUI 栈（egui / wgpu / naga / font-kit）都经 log 门面记录，去掉它这些三方诊断就不再进终端与日志文件。
