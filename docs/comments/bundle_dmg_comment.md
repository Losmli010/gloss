# bundle_dmg.py 决策记录

> 按符号锚定（不写行号，防漂移）。
> 本文件随仓库跟踪（docs/comments/ 入库），文件内不留指向本文件的指针。

## 为什么有本脚本

- 2026-09-18 cargo-bundle 的 osx 格式只产 .app，.dmg 由本脚本用系统自带的 hdiutil 补齐，不引入 create-dmg 之类的第三方依赖。
