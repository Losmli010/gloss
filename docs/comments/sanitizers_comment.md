# sanitizers.yml 决策记录

> 按符号锚定（不写步骤名/配置项，不写行号，防漂移）。改到相关配置时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），文件内不留指向本文件的指针。

## 顶层（只覆盖 gloss-core 的原因）

- 2026-09-18 Miri 解释执行 MIR、无法执行 FFI——平台层（macOS 框架调用）与渲染层（wgpu/egui）跑不了；Valgrind 只有 Linux 版，而本仓库 macOS-only 后 Linux 上能编译的只有 gloss-core。

## Miri 工具链安装

- 2026-09-18 Miri 只随 nightly 发布，上游只对最新 nightly 测试——跑挂了先跟着升 nightly。

## Run Valgrind（先取测试二进制）

- 2026-09-18 先编译再单独包测试二进制：直接包 cargo 会把 rustc 编译进程也拖进 valgrind。

## Run Valgrind（flags）

- 2026-09-18 memcheck 非法读写立即失败；泄漏只判 definite（真不可达），possible 多为后台线程持有的误报——且 valgrind 3.22 的 errors-for-leak-kinds 默认把 possible 也算错误，不显式收紧会误伤（首次 CI 实跑踩中）。
