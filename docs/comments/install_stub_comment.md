# install_stub 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），改名与合并用 git mv 保留历史；文件内不留指向本文件的指针。

## 替换桩（2026-11-05，Windows 移植 P0）

- Windows 侧自更新替换（exe 加旁置 dll 的 zip 校验、运行中可执行文件的重命名交换）按移植方案在实施前另立定案（§6 文件锁风险）；本桩 `install` 一律带 `InstallError::NotABundle` 失败——上层按失败卡降级，与“当前进程不在可替换的应用包内”同型。
- 错误枚举与 macOS 实现同名镜像（含 `Unwritable`），共享流程（update/mod.rs）与测试零改动编译；macOS 侧代码与测试原样保留（`#[cfg]` 分派在 update/mod.rs 的模块声明上）。
