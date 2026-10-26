# Cargo.toml（根包）决策记录

> 按符号锚定（不写行号，防漂移）。改到相关段落时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），文件内不留指向本文件的指针。

## package.metadata.bundle（应用图标）

- 2026-09-18 图标方案 A「划·译」；产物进仓库，矢量源与生成脚本见 assets/icons/README.md 与 scripts/dev/build_app_icon.py。

## workspace.lints.clippy（mem_forget）

- 2026-09-18 为什么 deny：泄漏的 Vec/Box 不触发 Drop，内存只增不减——长驻进程不可接受。

## workspace.lints.clippy（print_stdout / print_stderr）

- 2026-09-18 为什么 deny：绕过 gloss_core::log 的 print 既丢结构化字段，也可能把敏感信息（API key 等）直接打出去。

## dependencies（gloss-core 不开 test-util）

- 2026-09-18 生产不再需要 test-util（M4-T4 接了真实引擎，mock 装配已移除）：测试桩只出现在各 crate 的 dev-dependencies 构建里，不进发布二进制（check-constraints 门禁盯着依赖边）。

## dev-dependencies（winit）

- 2026-09-18 为什么 dev 依赖要 winit：L3 显隐自检（overlay_selftest）自带 main()、不经测试收集器，直接驱动 winit 窗口栈——winit 事件循环需要主线程。

## [[bench]]（core）与 dev-dependencies（criterion）

- 2026-09-19 为什么 criterion 关默认集：default 带 rayon / plotters（多线程统计与绘图）基准用不上，只留 cargo_bench_support 保住 cargo bench 接口；check-constraints 对 dev-dependencies 同样强制 default-features = false。
- 2026-09-19 为什么 bench 放根包：bench 目标是独立编译单元，放根包直接吃根包既有的 gloss-core 路径依赖，不给 gloss-core 加 dev-dependencies、也不碎片化子 crate 的目标清单；harness = false 与 overlay_selftest 同一先例。
- 2026-09-19 开发方式：feat/bench-core 分支在独立 git worktree（../gloss-bench）完成，与本地并行开发的重构分支零接触；合并后 worktree 可删。

## 2026-10-07 新增 [profile.profiling]（P3-1 剖析构建）

- release `strip=true` 连符号表一起剥掉，sample/vmmap 对它只能出 `???` +
  偏移；profiling 配置继承 release 全部优化（火焰图形态不失真），只把 strip
  放宽到 debuginfo（保留符号表，函数名可符号化）、debug 用 line-tables-only
  （行号表够 sample 用，不背完整 DWARF 的体积与构建时长）。仅供剖析，不进
  打包链路。

- 2026-10-07 实测成本备注：profiling 配置继承 release 的 fat LTO，本机
  （Intel i5-7360U）首次全量构建约 2.5h（其中并行会话把负载顶到 150 有放大；
  根包 gloss 单 crate 的 LTO 编译+链接即 ~50min）。构建一次后缓存，剖析属
  低频动作可接受；嫌重可改 `lto = "thin"`（一行），代价是火焰形态与 release
  有细微偏差，暂不换。

- 2026-10-07（PR #121 评审修正）`debug = "line-tables-only"` 是死配置：
  strip="debuginfo" 会把行号表一并剥掉（评审以 hello-world 实测 521→420KB，
  符号表保留），而 sample 符号化只需要符号表——删掉 debug 键，注释改
  「保留符号表」。若日后要 atos 行号级归因，改 strip="none" 再谈。

## 2026-10-07 PGO 试验（P3-2，未合入任何构建配置）

- 试验在独立 worktree 以环境变量完成（RUSTFLAGS 进 env 不进 Cargo.toml），
  结论不接入打包链路，故仓库不留 PGO 相关配置；完整流程与数字见计划文档
  P3-2 节内记录。
- 方法学：profraw 必须用 rustc 配套的 llvm-profdata（rustup llvm-tools
  组件）合并——CommandLineTools 自带的版本读不了新格式；SIGTERM 死亡也能
  落盘 profraw（LLVM 运行时行为，无需优雅退出）；`-Cllvm-args=
  -pgo-warning-threshold` 在 rustc 1.96 的 LLVM 里不存在，裸 `-Cprofile-use`
  即可。PGO 使 06_ready 中位 -5.4% 且方差收窄，但绝对量 ~30ms 不值
  +8% 体积与双构建链路。

## profile.release（panic = "abort"，P2-1，2026-10-13）

- 为什么只上 abort 不动 opt-level：2026-10-12 二次决策把原 P2-1（opt-level=z +
  panic=abort）拆分——z 级体积收益（15.18→10.78 MB 实测，两项之和）伴可感知的
  运行时代价，不做；abort 单独做，收益是去掉 unwind 展开表与着陆点（预期体积
  小幅下降，以流水线趋势点为准），opt-level 维持 3。
- 为什么四处 unwind 降级路径保留而不删除（events/mod.rs 的 `guarded`、
  events/mouse.rs 的 tap sink 屏障、runtime/pipeline.rs 的任务 `catch_unwind`、
  update/mod.rs 的 panic 类 JoinError 降级）：release（abort）下它们的降级分支
  不可达，但 dev/test profile 仍是 unwind（test 目标由 cargo 强制 unwind），
  屏障照常生效并被既有 L1 单测锁定；删除会让测试 profile 的行为与测试一起退化，
  且日后恢复降级语义不再是「删一行配置」。处置为逐处补「生效边界」旁注（按
  AGENTS.md 注释纪律），删除与 `#[cfg(panic)]` 双路径都不取。
- 行为变更（须在 PR 描述显式记录——截至 2026-10-13 评审，分支
  perf/p2-panic-abort 仅本地、PR 尚未开出，开 PR 时必须补上该段描述）：
  release 下后台任务 panic 不再转 TaskFailed 失败卡、进程直接崩溃；事件线程
  与鼠标 tap 的 C 回调屏障防 abort 保护同步消失。`[profile.profiling]` 继承
  release 同为 abort——剖析形态与 release 一致是本意。

## mimalloc(2026-10-16,内存优化方案)

- 为什么换全局分配器:macOS libmalloc 把峰值后的空闲页扣在 size-class magazine 里不归还,长驻进程 RSS 只涨不落——「空闲 160MB」里相当份额是它。mimalloc 周期性 purge 空闲页,常驻贴活跃字节。备选 jemalloc+decay 参数不取:多一个调参面,mimalloc 默认档即达标;只进根包 bin 依赖,库 crate 与测试目标不受影响(Miri/valgrind 跑 gloss-core 单测,不经此分配器)。

## workspace.lints.rust（missing_docs 移除，2026-10-17）

- 为什么移除：它把「每个公开项都要有文档注释」变成硬门禁，于是成批产生了只把标识符读
  一遍的占位注释（构造函数的「创建 X」、字段的「X」）——降不了未来维护成本，还稀释了
  真正有信息量的注释。注释纪律改按「重述型注释一律删除」执行（见 AGENTS.md 注释纪律），
  强制手段从 lint 交回评审：「有没有维护价值」本就不是 lint 能判定的。
- 备选不取：降为 `warn`——warn 同样诱导写占位；保留 deny 而只删注释——删完 `just lint`
  直接失败，两者不能并存。
- 连带影响：测试目标的 crate 级 `//!` 头原先由它强制（AGENTS.md 注释纪律的例外项之一）。
  该 `//!` 头继续保留，理由改为「测试」节要求的层标签载体，注释纪律的例外措辞已同步改写。
- 移除后 `[workspace.lints.rust]` 成为空表，一并删除；各 crate 的 `[lints] workspace = true`
  继承不变。`[workspace.lints.clippy]` 全部保留。
