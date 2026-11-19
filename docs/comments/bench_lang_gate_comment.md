# bench_lang_gate 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），改名与合并用 git mv 保留历史；文件内不留指向本文件的指针。

## 缘由（为什么补语言门基准）

- 2026-11-20（方案 D7 前半）语言门全仓无任何量化数字，而它是 C2（下标越界门禁逐包开启）改写的落点之一——改写前先存命名基线作对照锚点，改写后按同一基准复跑比对。判定器是纯函数（不读配置、不碰网络），基准只用字面量输入，零桩参与。

## 用例设计（四档）

- 2026-11-20 word_card/clean：词卡整卡干净通过——注解、例句、注音三字段全扫描且零违约，量化通过路径的成本。
- 2026-11-20 sentence/overlap：长句任务译文回引原文整段（原样照抄触发原文重合判定），量化重合比对的扫描成本；例句携带中文片段，计入字段级扫描。
- 2026-11-20 sentence/long：长句任务的整段中文译文（多小句长正文）对照长英文原文，量化解析器（pulldown-cmark）解析与逐文字段扫描随正文长度的伸缩。
- 2026-11-20 code/containers：代码任务带围栏代码块容器（代码任务不开引文容器），量化容器扫描与结构节点判定的成本。

## 无 panic 写法（clippy 纪律）

- 2026-11-20 bench 目标 harness = false，不在 clippy.toml 的 test 豁免范围（allow-*-in-tests 只认 #[test] / #[cfg(test)]），unwrap / expect / panic / print 与生产同罪。fixture 全部 struct 字面量 + 字符串拼接构造（不经 serde 解析、不打印），判定返回的违约表交给 black_box。

## 基线（2026-11-20，C2 改写前的对照锚点）

- 2026-11-20 运行环境：Intel Core i5-7360U @ 2.30GHz（x86_64）/ darwin 22.6.0 / rustc 1.96.1；criterion 默认采样。锚点名 c2-lang-gate-pre（经基准配方 bench-save 存命名基线，数据落在各基准目录下）。锚点跑次中位数（均值点估计）：
  - word_card/clean 8.4µs、sentence/overlap 45.9µs、sentence/long 98.9µs、code/containers 96.3µs。
- 2026-11-20 双核移动级处理器的跑次间波动可观（同夹具三跑：code/containers 68µs 到 117µs，word_card/clean 8.4µs 到 12.1µs）——C2 对照按同机同条件复跑，以区间与判读结论（criterion 判定）取数，不以单点数字定论。
- 2026-11-20 C2 改写落点后按同一基准与同一锚点名复跑比对（just bench-check c2-lang-gate-pre），对照数字随 C2 的拉取请求（PR）正文一并呈现。
