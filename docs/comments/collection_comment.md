# collection 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），改名与合并用 git mv 保留历史；文件内不留指向本文件的指针。

## 拆分决策（2026-11-05，Windows 移植 P0）

- 字体集合（ttc）面序解析是纯字节解析、无平台依赖，从发现器（imp）拆出独立成档：Windows 占位期它只被父模块测试消费，P2 Windows 发现器（DirectWrite + 固定候选）与 macOS 发现器同用一份。
- 模块声明带 `#[cfg(any(target_os = "macos", test))]`：Windows 库目标下无发现器消费它，整体排除避免死码判罚；测试目标下随父模块测试编译。cfg 落点在模块声明文件（fonts.rs）的属性上，符合“业务文件内部零 cfg”边界。
