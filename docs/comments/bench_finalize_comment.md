# bench_finalize 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），改名与合并用 git mv 保留历史；文件内不留指向本文件的指针。

## 范围（为什么测完成态解析）

- 2026-09-19（沿革：基准随包落位，自根包 benches/core.rs 迁来，见文末）完成态解析是模型回复落到任务产物的必经热点，用例覆盖词卡少例 / 多例、长正文成功与纯文降级（回复不是 JSON）四条路径，量化 JSON 解析与结构抽取的成本随例数与正文的伸缩。

## 无 panic 写法（clippy 纪律）

- 2026-09-19 bench 目标 harness = false，不在 clippy.toml 的 test 豁免范围（allow-*-in-tests 只认 #[test] / #[cfg(test)]），unwrap / expect / panic / print 与生产同罪。所以 fixture 全部字符串拼接构造（不经 serde 解析、不打印），解析结果的降级形态照常交给 black_box。

## 基线（2026-09-19，bench profile：lto = true、codegen-units = 1）

- 2026-09-19 运行环境：Intel Core i5-7360U @ 2.30GHz（x86_64）/ macOS 13.7.8 / rustc 1.96.1；criterion 默认采样。中位数：完成态解析组 word_card/ok 9.4µs、word_card/many_examples 52.6µs（沿革：当时的用例名与少例 / 多例两档对应）、long_note/ok 与 prose/degraded 同量级微秒段。
- 2026-09-19 完整输出留存：/tmp/gloss-bench-run.log（临时）；criterion 自身数据在 target/criterion/。
- 2026-11-20 同机复测（落位后首跑，criterion 默认采样）中位数：word_card/ok 2.53µs、word_card/many_examples 8.48µs、long_note/ok 1.80µs、prose/degraded 108ns。

## 沿革

- 2026-11-20 基准随包落位（方案 D7 前半）：本组自根包 benches/core.rs 的 complete 组迁入，组名与用例名不变（趋势编号连续）；本文件承接原 core_comment.md 中与本组相关的条目。
- 2026-11-20 基准回迁根包：自主线程应用层（gloss-app）benches/ 迁回根包 benches/finalize.rs，组名与用例名不变（趋势编号连续）。
