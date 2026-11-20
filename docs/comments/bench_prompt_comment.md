# bench_prompt 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），改名与合并用 git mv 保留历史；文件内不留指向本文件的指针。

## 范围（为什么测提示词渲染）

- 2026-09-19（沿革：基准随包落位，自根包 benches/core.rs 迁来，见文末）提示词渲染每任务必经，词卡短词与 8KB 代码长文两档，量化注册表按任务拼装整段提示词的成本。

## 无 panic 写法（clippy 纪律）

- 2026-09-19 bench 目标 harness = false，不在 clippy.toml 的 test 豁免范围（allow-*-in-tests 只认 #[test] / #[cfg(test)]），unwrap / expect / panic / print 与生产同罪。fixture 全部 struct 字面量 + 字符串拼接构造（不经 serde 解析、不打印）；提示词渲染的 Result 用 .ok() 交给 black_box。

## text_task 的 expect 豁免

- `#[allow(clippy::expect_used)]` 的缘由：基准夹具的 kind 恒为文本类，模态矩阵保证构造成功——expect 只在测试夹具的恒真路径上，不影响生产纪律。

## 基线（2026-09-19，bench profile：lto = true、codegen-units = 1）

- 2026-09-19 运行环境：Intel Core i5-7360U @ 2.30GHz（x86_64）/ macOS 13.7.8 / rustc 1.96.1；criterion 默认采样。中位数：prompt_render：word/short 1.6µs、code/long 2.0µs。
- 2026-09-19 完整输出留存：/tmp/gloss-bench-run.log（临时）；criterion 自身数据在 target/criterion/。
- 2026-11-20 同机复测（落位后首跑，criterion 默认采样）：word/short 11.3µs、code/long 9.4µs。较 2026-09-19 明显上升，缘由未在本拉取请求（PR）内追溯（期间提示词库经任务化智能层重构），两期数字按日期并存。

## 沿革

- 2026-11-20 基准随包落位（方案 D7 前半）：本基准自根包 benches/core.rs 的 prompt_render 组迁入，组名与用例名不变（趋势编号连续）；本文件承接原 core_comment.md 中与本组相关的条目。
- 2026-11-20 基准回迁根包：自核心库（gloss-core）benches/ 迁回根包 benches/prompt.rs，组名与用例名不变（趋势编号连续）。
