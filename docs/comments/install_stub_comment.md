# install_stub 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），改名与合并用 git mv 保留历史；文件内不留指向本文件的指针。

## 替换桩（2026-11-05，Windows 移植 P0）

- Windows 侧自更新替换（exe 加旁置 dll 的 zip 校验、运行中可执行文件的重命名交换）按移植方案在实施前另立定案（§6 文件锁风险）；本桩 `install` 一律带 `InstallError::NotABundle` 失败——上层按失败卡降级，与“当前进程不在可替换的应用包内”同型。
- 错误枚举与 macOS 实现同名镜像（含 `Unwritable`），共享流程（update/mod.rs）与测试零改动编译；macOS 侧代码与测试原样保留（`#[cfg]` 分派在 update/mod.rs 的模块声明上）。

## 收窄为未适配目标占位（2026-11-07，Windows 移植 P3a）

- 系统（Windows）实现在 P3a 落地（`install_windows.rs`，换装编排走共享核心 `install_shared.rs`），本桩的覆盖面随之收窄为“两平台之外的其余目标”，恒失败语义与错误镜像不变；`#[cfg]` 分派仍在 update/mod.rs 的模块声明上，桩内零 cfg。

## 失败缘由改为平台未支持（2026-11-07，移植评审遗留 A3）

- 为什么恒失败不再借 `NotABundle` 表达：非应用包（NotABundle）的语义是“当前进程不在可替换的应用包内”（macOS 侧按真实包检测、系统（Windows）侧按可执行文件定位失败使用），桩路径的真实缘由是“平台未适配、替换策略未实现”，借用同型错误会让失败归因失真。
- 处置：桩枚举新增专用变体 `PlatformUnsupported` 并由 `install` 恒返回，行为契约不变（上层照常 `accept_install_failed` 按失败卡降级）；错误枚举仍与两平台实现同名镜像（其余变体原样保留），共享流程（update/mod.rs）零改动。本地错误类型带不动 `GlossError::PlatformUnsupported`（`InstallError` 是模块内类型，不跨端口契约），平台未支持的完整表达留在平台端口层的既有路径。
