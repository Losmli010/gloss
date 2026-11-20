# bench_cache 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），改名与合并用 git mv 保留历史；文件内不留指向本文件的指针。

## 范围（为什么测键派生与缓存吞吐）

- 2026-09-19（沿革：基准随包落位，自根包 benches/core.rs 迁来，见文末）cache_key 是全链路每次执行的必经点，且 Image 输入把整块 PNG 字节经 serde（rc 特性）逐字节序列化成 JSON 数字数组——1MB 截图约 3.9MB JSON 文本，是真实截图路径的最大单点开销，1mb/4mb 两档验证线性度；任务缓存测缓存端口吞吐——get 含返回值 clone（真实读路径），set 分同 key 覆盖与新 key 的 TinyLFU 驱逐稳态。

## 无 panic 写法（clippy 纪律）

- 2026-09-19 bench 目标 harness = false，不在 clippy.toml 的 test 豁免范围（allow-*-in-tests 只认 #[test] / #[cfg(test)]），unwrap / expect / panic / print 与生产同罪。所以 fixture 全部 struct 字面量 + 字符串拼接构造（不经 serde 解析、不打印）；缓存写入用 iter_batched 把产物构造移出测量区间。

## 基线（2026-09-19，bench profile：lto = true、codegen-units = 1）

- 2026-09-19 运行环境：Intel Core i5-7360U @ 2.30GHz（x86_64）/ macOS 13.7.8 / rustc 1.96.1；criterion 默认采样。中位数：
  - cache_key：text/short 595ns、text/long 81µs、**image/1mb 41.6ms、image/4mb 79.8ms**——Image 任务每次执行都要付这笔序列化开销，是全链路已知最大单点，优化候补从这里开始。
  - task_cache：get/hit 1.3µs、get/miss 675ns、set/same_key 4.4µs、set/new_key 3.2µs。
- 2026-09-19 完整输出留存：/tmp/gloss-bench-run.log（临时）；criterion 自身数据在 target/criterion/。
- 2026-11-20 同机复测（落位后首跑，criterion 默认采样）中位数：cache_key：text/short 170ns、text/long 1.89µs、image/1mb 272µs、image/4mb 1.10ms；task_cache：get/hit 584ns、get/miss 124ns、set/same_key 2.19µs、set/new_key 1.99µs。图像序列化一档较 2026-09-19 大幅下降（期间缓存与序列化路径经多轮重构），两期数字按日期并存。

## 沿革

- 2026-11-20 基准随包落位（方案 D7 前半）：cache_key 与 task_cache 两组自根包 benches/core.rs 迁入同一基准目标（两组同源被测模块 runtime/cache.rs），组名与用例名不变（趋势编号连续）；本文件承接原 core_comment.md 中与这两组相关的条目。
- 2026-11-20 基准回迁根包：随基准目标自主线程应用层（gloss-app）benches/ 迁回根包 benches/cache.rs，组名与用例名不变（趋势编号连续）。
