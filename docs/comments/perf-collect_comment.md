# perf-collect.py 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），文件内不留指向本文件的指针。

## 定位（2026-10-08，T2）

- 取代 bench-baseline.py + perf-history.py 三段搬运链（criterion → 按源 JSON →
  嵌套趋势点）：每加一个数据源过去要改两处格式，现在只在 collector 加一个
  折算函数、在 metrics.py 登记一列 id。入库工件收敛为 history.jsonl 一份，
  按源基线 JSON 全部删除（趋势、门禁基线、预算、env 都由点自承载）。
- 「只快照、不判罚」的原则归 metrics.py 注册表的 judge 字段承载（T1 起）；
  collector 只折算不判定，判罚权在 clone-check 与 L3 自检退出码。
- 点模型 {commit, date, env, budgets?, metrics}：budgets 仅在该点折算了自检
  stderr 行时写入（数值随行流出，源头是 Rust 常量，任何环节不手写数值）；
  env 各源尽力合并、缺失留空对象（图表回退取最新非空 env，v1 迁移点 env
  为空对象正落在该回退路径上）。

## 继承自旧管线的原则（bench-baseline.py / perf-history.py，2026-10-05 起）

- 历史点沉淀进 history.jsonl 的缘由不变：criterion/自检/片段全是本机
  target/ 产物不入库，趋势序列只能靠每次采集主动沉淀；一行一点，聚合不记
  明细（曲线要整体口径，明细留在 target/ 本地产物，避免第三份明细源）。
- 同 commit 重跑**原位**覆盖（PR #109 审查修订）：去重防膨胀，且不挪动既有
  时序位置——图表 x 轴按位置绘制，移位即失真。
- 缺数据不造 0（旧「缺 perf 段拒绝记录」「core 缺失记 null 跳段」的推广）：
  0 值点进趋势线就是假坠崖，故某源缺席 → 对应 id 整体缺席，图表跳段。
  全部源都缺席时连点也不写（空指标点只是噪音）。

## 本 collector 的新决策（2026-10-08）

- **date 升序插入**（设计 7.3，八稿）：新点插在首个 date 更大的点之前，与
  「原位覆盖」并存——覆盖保位（位置即时序），新点按 date 插序，手动补采
  历史区间不打乱时序。date 解析失败当场失败（时序是文件的核心语义，不容
  静默错排）。
- **v1 就地迁移**：写回时把嵌套点（clone/core/app 分组键）规范化为 v2 扁平
  点，保序、一次性，迁移点 env 置空。v1 的单值 `app.binary_kb` 折算为
  `app.binary_x64_kb`：v1 的 7 个点全部产自本仓 x86_64 采集机（2026-10-05
  至 07 的本机基线），这是 v1 数据的既成事实而非运行时推断——collector 不
  读运行机架构来猜历史数据的架构。
- **criterion 只收注册表已登记的 id**：target/criterion 是滚动状态，基准
  改名/删除会留下陈旧目录，旧管线照单全收（会以未注册 id 入档）；现按
  metrics.py 注册表过滤，未登记目录警告跳过。注册表是 id 唯一登记处。
- **clone.allocs/bytes 逐基准均值取整后合计**：与旧链逐字节同口径
  （read_perf 每基准 round → 合计），保证门禁基线数值跨管线衔接，迁移点与
  新点可直接对比。
- **数据源容错分级**：criterion 单目录残缺警告跳过（滚动数据，个别坏目录
  不该挡整点采集）；selftest.jsonl/app-runtime.json 在场但坏行/坏 JSON 当场
  失败（它们是对应 id 的唯一来源，静默跳过等于无痕丢数据）；文件缺席一律
  视为源缺席。
- **信号行取各源最后一条**：wrapper 每次整文件覆写落盘，正常只有一条；取
  末条与 perf-report 的读法一致，兼容将来改追加语义。
- **连字符脚本加载**：clone-stats.py 文件名含连字符，`import` 语句加载不
  了，用 importlib 按路径加载；不为此改名（justfile 配方引用文件名，改名
  归 T6 统一配线时一并考虑）。
- app-metrics 输出的 v2 片段（target/perf/app-runtime.json）是本机 handoff
  不入库：采样逻辑不变，只把已在采集的 footprint 双口径、空闲 CPU、峰值
  RSS 一并纳进片段（覆盖评估结论：生产者已在采集的指标一律入表，零采集
  成本），体积按本机架构填 binary_x64_kb / binary_arm64_kb 之一，识别不了
  架构就失败——猜一个 id 写错架构比失败更糟。

## 全链验收（2026-10-08，T6）

- 首次真实全链跑通（T6 验收，PR #133）：criterion 墙钟、clone 分配、clone 静态计数、自检
  信号、app 片段五源同时在场折算为一点（39 项指标），overlay/startup 并入与 budgets 折算
  （100 ms / 4,000 ms / 2,048 KB）实测正确；同 commit 重跑原位覆盖生效（追加→覆盖→剔除验
  收点后与 main 逐字节一致，验收数据点未进 PR）。
- eval live 源因 GLOSS_LIVE_* 缺席未实测（折算路径 T2 起由单测覆盖）：live.json 缺席即对应
  id 整体缺席，与本节其余源的行为一致，T7 流水线首跑前保持该状态。

## 文件头迁出的数据源与用法（2026-10-24，注释纪律）

- 用法：`scripts/perf/perf-collect.py`；`--history /tmp/h.jsonl` 覆盖 history 路径
  （相对路径按仓库根解析）。写回 `baselines/history.jsonl`。
- 数据源实名（全部 target/ 本地产物，某源缺席 → 对应 id 整体缺席，不造 0）：
  - criterion core 墙钟：`target/criterion/<组>/<用例>/new/estimates.json` 的
    `mean.point_estimate` → `core.<full_id>`；只收注册表 CORE_GROUPS 内且已登记的 id。
  - criterion clone 分配：`target/criterion/clone_allocs/*` 与 `clone_bytes/*` →
    `clone.allocs` / `clone.bytes`。
  - clone 静态计数：复用 clone_stats 的计数函数 → `clone.total` / `clone.density`。
  - L3 自检信号：`target/perf/selftest.jsonl`（selftest_wrapper 产物，取各源最后一条
    信号行）→ `overlay.*` 延迟分位/帧时间/尾段/句柄与 `startup.elapsed_ms`；行内
    `budget_ms` / `rss_tail_budget_kb` 折算进该点 budgets 段。
  - app 运行时片段：`target/perf/app-runtime.json`（app-metrics 产物）→ `app.*`
    （含 `env.cpu`）。
  - eval live 报告：`target/eval/live.json` → `eval.task_latency_p50/p95_ms`，仅
    `mode=="live"` 且字段在场时并入。
- 点模型：`{commit, date, env, budgets?, metrics}`；v1 嵌套点按 clone/core/app
  分组键识别，`app.binary_kb` 按采集机架构（x86_64）折算为 `app.binary_x64_kb`。
