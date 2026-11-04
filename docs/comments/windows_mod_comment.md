# windows_mod 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），改名与合并用 git mv 保留历史；文件内不留指向本文件的指针。

## 占位实现总纲（2026-11-05，Windows 移植 P0 §3.1/D2）

- 类型面与系统（macOS）实现同名镜像，覆盖消费方（组装段 src/wiring.rs、应用层 menu::install 与 clipboard::copy_text、入口 scene/storage）触及的公开面；macOS 实现内部子模块（ffi、辅助功能读取器等）无消费方，不建占位——无假实现。
- 行为口径：有结果通道的一律返回 `GlossError::PlatformUnsupported`（D2 新变体，不用 unreachable/panic 顶替）；事件源轮询恒产出空集；菜单装配报告未装配（壳侧按“无菜单栏运行”既有降级）。
- 镜像只服务编译面（P0 验收 = 两端 check 与 Clippy 绿），不承诺 Windows 可用；P2 能力落地时本目录整体替换为真实现，届时 AGENTS.md 架构速览随动（I4）。

## 桩角色随端口收敛（2026-11-05，Windows 移植 P1）

- 消费方（组装段与应用层）不再引用本目录的类型与函数：桩从“消费方触及类型的同名镜像”转为端口桩——同名类型（`MouseSource`、`CompositeReader`、`ClipboardWatchSource`、`SystemImageReader` 等）改以端口实现的面目存在，未实现端口照旧返回平台未支持或产出空集；装配句柄（`Platform::connect`）是唯一构造点。
