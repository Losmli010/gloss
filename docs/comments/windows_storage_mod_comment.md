# windows_storage_mod 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），改名与合并用 git mv 保留历史；文件内不留指向本文件的指针。

## 组合存储桩（2026-11-05，Windows 移植 P0）

- 构造可用、逐操作失败：`load` 返回 `PlatformUnsupported` 后应用壳经 `load_or_default` 的既有降级落出厂默认（有痕），save/密钥操作一律报未支持——比入口即败更诚实：端口每次被触到都如实回答。
- 文档半边（TOML + directories）与密钥半边（Windows 凭据管理器）按移植方案后续阶段补齐，届时替换本桩。
