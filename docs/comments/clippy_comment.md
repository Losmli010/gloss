# clippy.toml 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关配置时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），文件内不留指向本文件的指针。

## allow-*-in-tests

- 2026-09-18 unwrap/expect/panic 在测试代码（`#[test]` 与 `#[cfg(test)]`）放行的原因：测试断言天然依赖「失败即 panic」，错误传播反而是噪音。
- 2026-09-18 print_stdout/print_stderr 在测试代码放行的原因：调试输出与失败现场打印只发生在测试里。
