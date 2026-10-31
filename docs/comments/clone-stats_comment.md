# clone-stats.py 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），文件内不留指向本文件的指针。

- 2026-10-05 文本级剥离而非 syn/rustc：无工具链依赖、秒级、跨机器可复现（基线可比的前提）；代价是病态语法（如 `<>` 内出现花括号）可能误导项体定位，接受极小误报概率。计数口径若变更，旧基线全部作废，必须整体重写并在 PR 说明。
- 2026-10-05 剥离 `#[cfg(test)]` 的实现：属性起点起连续跳过多个 `#[...]`，再找项体首个顶层 `{`（跳过括号、遇 `;` 即无项体声明），花括号配对整体置空；嵌套 cfg(test) 项靠区间去重叠处理。注释与字符串先置空再匹配属性，避免字面量里的 `#[cfg(test)]` 误触发。
- 2026-10-05 计数对象只取 `.clone(` 方法调用；`X::clone(`（如 Arc::clone）与 derive(Clone) 入库展示但不参与成败判定：前者是廉价克隆的有意惯用法，后者是类型能力而非调用点，计入会把正常写法误伤成回退。
- 2026-10-05 不进 precommit 与 CI：密度随正常重构波动、clone 常随功能演进净增，门禁会高频误报且无裁决标准；定位是 bench-check 式审计对照，是否收紧由后续需要决定。
- 2026-10-05 `CRATES` 清单是路径单点：新增 crate 或改包名需同步，漏加会静默漏计；未加脚本校验其与 workspace 成员一致，留作需要时补的约束项。

## 性能口径（2026-10-05 增补）

- 性能指标取**分配次数与字节数**（benches/clone.rs 的 criterion 自定义测量），不取
  耗时：分配是 clone 的直接开销通道，且计数对固定输入近似确定，可作精确判定；耗时
  有噪声且已有 criterion 自身基线机制（bench-check/bench-summary）与 benches/core.rs
  兜着，不重复存第二份耗时真相。
- 判罚口径：allocs 或 bytes **精确整数**上升即回退（先实测两轮稳定性确认恒等）；
  依赖升级导致的整体漂移属“有意的基线更新”，走 PR 审查。
- `machine/lifecycle` 的输入构造计入测量（iter_batched 的 setup 分配同样落进测量
  窗口，躲不掉，索性内联进 `b.iter` 让口径自明）；其余热点输入全部循环外构造，
  计数纯净。
- UI 帧经 `egui::Context::run_ui` 无头驱动：预热期推进时间越过淡入动画并建立
  egui/字体缓存，测量期时间冻结——egui 的缓存淘汰按 `now - last_used` 判定，时间
  不走就永不触发，逐帧分配恒定（实测确认）。
- 可见性连带：`ui::i18n`（`Text`/`get`/`for_error`/`for_error_detail`）、
  `settings::draw`、`popup::draw`/`RenderState`/`PopupOutput` 从 `pub(crate)` 放开为
  `pub`（纯可见性，无行为变化），i18n_comment/settings_comment/popup_comment 各有
  修订记录。

## 门禁基线改由 history 最新点承载（2026-10-08，T2）

- 基线 JSON（clone-stats.json，曾放 scripts/baselines/ 进 git）删除，门禁基线改
  读 baselines/history.jsonl **最新点**的 clone.total/density/allocs/bytes：基线随
  perf-collect.py 采集自然前移，变更经 history.jsonl 的 PR diff 显式审查（继承原
  clone-stats.json 的唯一门禁地位，设计 3.2）。`--write` 与 perf 段随之删除，本
  脚本收敛为“静态计数 + --check”。
- `--check` 判罚口径不变的部分：total 或 allocs/bytes 合计**精确整数**上升即退
  出码 1；密度仍仅记录。变化的部分：对照粒度从“任一 crate”收窄为聚合总量——
  history 只承载聚合（crate 级明细不进趋势存储），crate 间此消彼长不再判回退，
  该情形少见且源码 diff 在 PR 里本就可见；取舍如实记录，若误放行频发再议收紧。
- 降级防护延续 #109 精神（write_baseline 拒绝降级覆盖已随 `--write` 删除）：最新
  点缺 clone.total 或缺分配值即带指引失败——分配为 0 的假基线会把真回退放行，
  本脚本是判罚终点，不依赖上游采集侧防护。当前侧 criterion 无数据时只提示“当
  前无数据”不判罚（与旧 perf 段对照行为一致）：判罚依赖刚跑过 `just clone-bench`。
- read_perf 保留：--check 需要当前分配值、缺省打印需要分配热点展示；它不再参与
  任何基线写入。files 级明细只在缺省打印里呈现，不再作基线对照（history 无此明
  细，且 git diff 已可见文件级增删）。

## 配线收敛（2026-10-08，T6）

- perf-baseline 配方不再调用本脚本：`--write` 已随 T2 删除，静态计数由 perf-collect.py 经
  importlib 复用（collect_clone_static），门禁基线随采集自然前移；本脚本的配方入口只剩
  clone-stats（只读打印分布）与 clone-check（判罚），判罚口径与降级防护不变（上节）。
- clone-check 前置从“先跑 just clone-bench”扩为“clone-bench + perf-baseline”（配方注释
  同步）：基线读 history 最新点，采集完成后最新点才是新基线；漏跑采集时对照的是旧点，不会
  误放行（上升仍判回退），只是基线滞后。

## 文件头迁出的用法与口径（2026-10-24，注释纪律）

- 用法：`scripts/perf/clone-stats.py` 打印当前分布；`scripts/perf/clone-stats.py --check`
  对照 history 最新点报告变化，clone 计数或热点分配上升即退出码 1。
- 生产代码口径实名：根包 `src/main.rs` 与各包 `src/`；剥离 `#[cfg(test)]` 标注的项；
  `tests/`、`benches/` 与测试桩不计。
- 计数对象实名：实例方法 `.clone(`；静态路径形态 `X::clone(`（如 `Arc::clone`）与
  `derive(Clone)` 只入库展示，不参与基线成败判定。
- 性能口径实名：`benches/clone.rs` 的分配计数（次数/字节，criterion 自定义测量）；
  `--check` 读取 `target/criterion` 下最近一次 `just clone-bench` 的结果，对照
  `baselines/history.jsonl` 最新点的 clone 门禁基线（`total`/`allocs`/`bytes`，由
  perf-collect.py 随采集自然前移）。
- 计时不进判罚，归 criterion 自身的基线机制（`just bench-check` / `bench-summary`）。
