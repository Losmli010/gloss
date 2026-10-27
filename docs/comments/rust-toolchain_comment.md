# rust-toolchain.toml 决策记录

> 按符号锚定（不写行号，防漂移）。
> 本文件随仓库跟踪（docs/comments/ 入库），文件内不留指向本文件的指针。

## components

- 2026-09-18 llvm-tools-preview 供 cargo-llvm-cov 使用；缺它时 llvm-cov 会停在 rustup 的安装提示上。
