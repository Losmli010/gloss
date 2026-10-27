# benches/core.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关符号时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），文件内不留指向本文件的指针。

## 范围（为什么只测这四组）

- 2026-09-19 cache_key 是全链路每次执行的必经点，且 Image 输入把整块 PNG 字节经 serde（rc 特性）逐字节序列化成 JSON 数字数组——1MB 截图约 3.9MB JSON 文本，是真实截图路径的最大单点开销，1mb/4mb 两档验证线性度；parse_structured 是产物落地的解析热点，用例覆盖成功 / 缺失围栏 / 坏 JSON 三条路径；prompt_render 每任务必经，短词 vs 8KB 代码两档；moka_cache 测缓存端口吞吐——get 含返回值 clone（真实读路径），set 分同 key 覆盖与新 key 的 TinyLFU 驱逐稳态。gloss-app 状态机暂不 bench：无已知性能疑点，且会拉起 egui_kittest/wgpu 的 bench 编译。

## 无 panic 写法（clippy 纪律）

- 2026-09-19 bench 目标 harness = false，不在 clippy.toml 的 test 豁免范围（allow-*-in-tests 只认 #[test] / #[cfg(test)]），unwrap / expect / panic / print 与生产同罪。所以 fixture 全部 struct 字面量 + 字符串拼接构造（不经 serde 解析、不打印）；PromptRegistry::render 的 Result 用 .ok() 交给 black_box；moka set 用 iter_batched 把产物构造移出测量区间。

## text_task 的 expect 豁免

- `#[allow(clippy::expect_used)]` 的缘由：基准夹具的 kind 恒为文本类，模态矩阵保证构造成功——expect 只在测试夹具的恒真路径上，不影响生产纪律。

## 基线（2026-09-19，bench profile：lto = true、codegen-units = 1）

- 2026-09-19 运行环境：Intel Core i5-7360U @ 2.30GHz（x86_64）/ macOS 13.7.8 / rustc 1.96.1；criterion 默认采样。中位数：
  - cache_key：text/short 595ns、text/long 81µs、**image/1mb 41.6ms、image/4mb 79.8ms**——Image 任务每次执行都要付这笔序列化开销，是全链路已知最大单点，优化候补从这里开始。
  - parse_structured：word_card/ok 9.4µs、word_card/many_senses 52.6µs、fence_missing 1.4µs、json_broken 1.1µs、ocr/ok 2.5µs。
  - prompt_render：word/short 1.6µs、code/long 2.0µs。
  - moka_cache：get/hit 1.3µs、get/miss 675ns、set/same_key 4.4µs、set/new_key 3.2µs。
- 2026-09-19 完整输出留存：/tmp/gloss-bench-run.log（临时）；criterion 自身数据在 worktree 的 target/criterion/。
