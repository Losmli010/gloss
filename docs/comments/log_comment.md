# log.rs 决策记录

> 按符号锚定（不写行号），改到相关代码时同步更新。行为与不变量写在代码注释里，
> 决策、备选方案与实测数据记在这里。

## task_span：代数改用 span 字段全链路携带（R3）

- 2026-09-23 之前每个日志点自己带 `generation = N`（共 13 处），深层的日志点（取材读选区、推理引擎）根本拿不到这个值——一次任务的日志链在最需要的地方断掉，排障靠时间戳人肉对齐。
- 设计：触发点（App 收到平台事件、状态机赋值代数处）创建 `task_span(generation)`；句柄装进 `Traced` 载荷随通道②③下发；事件线程 `enter()` 包住单条命令处理（取材 + AX + 剪贴板兜底全部覆盖）；tokio 侧对 `service.execute(...)` 用 `.instrument(span)`，引擎与 SSE 的日志一并带上。
- **为什么传句柄而不是各线程各建一个 span**：同一个 span id 才让一条任务的日志在 tracing 语义上属于同一条链路（时间轴连续），各建一个只会得到字段相同、彼此无关的 span。
- **减法做到哪一步**：删掉的是「被 span 覆盖」的手工字段——事件线程两处（`acquiring text` / `text input acquired`）、tokio 两处（取消与 panic）、触发点一处。**UI 侧（App）的字段保留**：那里本来就握着代数，且其中一半是「比代数」的日志（陈旧回传丢弃要同时打 `generation` 与 `current`），删掉反而丢信息。
- **级别取舍（评审后改为 `WARN`）**：初版取 `INFO`（照任务卡），评审指出——span 被级别压掉时字段也不出现，而任务作用域里最该留住的是 warn 及以上那几行（拒绝授权、后台 panic），`RUST_LOG=warn` 恰是排障时常用的档位。作用域内没有 error 级日志，故取 `WARN` 即覆盖「任何可能打印的级别」；`RUST_LOG=error` 时这些行同样被压掉，不丢信息。测试钉住这一点：`capture` 用与 `init` 同档的过滤器，把 span 级别降回 `debug` 测试当场红。

## capture：让别的 crate 也能断言日志

- 2026-09-23 加它是因为「代理由 span 携带」的链路级断言只有事件线程/引擎侧能证伪，而那些代码不在 gloss-core；而除 gloss-core 外谁都不能直接依赖 tracing（AGENTS.md 的日志出口约束）。`capture` 用线程局部的 `with_default`（不碰全局订阅者、可并行），把格式化后的行交回调用方断言。
- 触发点与进入点必须在同一个 `capture` 范围内：tracing 的 span 在**创建时**按当时订阅者的兴趣决定启用与否，没有订阅者时创建出来的 span 永远是禁用态，事后再装订阅者也补不回来——测试里踩过这个坑（先创建后捕获，断言必然落空）。

## capture_global：跨线程断言

- 2026-09-23 评审指出「tokio 侧 `.instrument(span)` 零覆盖」：`capture` 是线程局部的，而 tokio 任务落在 worker 线程上，断言不到。`capture_global` 装进程级订阅者（`OnceLock` + `set_global_default`），测试进程里没人调用 `init`，因此不冲突；同一进程重复调用拿到同一个读取端。用于 `crates/gloss-app/tests/pipeline.rs` 的 `engine_logs_carry_the_task_span`（断言引擎的「cache hit」行同时含 `generation=2`），变异验证：去掉 `.instrument(span)` 即红。
- 失败姿态：全局订阅者若已被别人占用，`set_global_default` 失败被忽略（捕获不到任何日志），断言会如实失败而不是假绿。

## 线程局部捕获 vs 进程级捕获（并发测试实证）

- 2026-09-23 任务 13 整改时踩到：`capture`（线程局部）里断言**与其它测试共享的调用点**（如 App 派发行）会在全量并行跑时**随机落空**——单线程跑过、多线程跑红。原因在 tracing 的兴趣缓存是全局的：事件调用点的 Interest 由「已注册的 dispatcher 集合」算出，而线程局部 `with_default` 的进出会让该集合反复变化，别的线程正在打的那条调用点就可能被判为 `never` 而整条事件被跳过。
- 所以：断言**本测试独有的调用点**（如探针行）用 `capture` 即可；断言**共享调用点**或**跨线程**（tokio worker）时用 `capture_global`（全局订阅者常驻，Interest 稳定）。gloss-app 的两条断言都改用 `capture_global` 后连跑三轮全绿。
- 另一条：`capture` 的过滤器与 `init` 同档（info），既是「生产默认级别下打得出来」的保证，也让「span 级别被降到 debug」这类错误当场暴露。

## 输出格式改为 JSON Lines（方案 A，owner 决定）

- 2026-09-23 owner 在三个备选里选 A：**两路 sink 都是 JSONL**（终端 stderr 与文件 `gloss.log.<date>` 同一格式，文件名不变，`just logs` 照旧）。启用 `tracing-subscriber` 的 `json` 特性（只多拉 `tracing-serde`；`serde_json` 本就是 gloss-core 的直接依赖）。
- 行形态：`{"timestamp","level","target","message",<事件字段…>}`，span 字段挂在 `span` 下——**任务代数因此在 `span.generation`**。`flatten_event(true)` 把事件字段摊到顶层；`with_span_list(false)` 避免同一个 span 再抄进 `spans` 数组；ANSI 关掉（颜色转义会破坏 JSON）。
- **查询口径变了**：不再 `grep "generation=2"`，而是 `grep '"generation":2'`（JSON 仍是行导向，grep 依旧够用）；`jq` 在 stock macOS 上没预装，文档不假定它存在。
- 测试影响：`capture` / `capture_global` 也换成 JSON layer（否则断言的是文本层），三处纯文本断言改为 JSON 口径；新增 `json_lines_carry_the_structured_contract` 钉格式契约（顶层键名 + 无 ANSI），其余 crate 的断言据此可以用子串而无需引 `serde_json`。
- 过渡期：改动当天同一个日志文件里旧行是文本、新行是 JSON（旧行的 grep 不受影响）；不改文件名是为了不破坏 `just logs` 与既有排障习惯。
- 代价：行体积约 2–3×（按天滚动保留 7 份，磁盘可忽略）；终端在 `just run` 时不再人读——这是方案 A 已知的取舍。

## 评审补充（2026-09-23）

- **`with_ansi(false)` 是多余的**：JSON 形态本身不产生颜色转义（json.rs 里没有 ansi 路径），该 flag 形同 no-op，已删除；初版注释说「否则破坏 JSON」不准确，已改。
- **重复键风险**：`flatten_event(true)` 把事件字段与 `timestamp`/`level`/`target`/`span` 写在同层，若将来哪个日志字段取名 `level`/`target`/`span`/`message`，同一对象里会出现重复键（解析时后者胜）。当前没有这样的调用点；写新日志时避开这几个名字。
- **`level` 与时间戳**：`level` 是大写字符串（`"INFO"`），`timestamp` 是 UTC RFC3339（微秒精度 + `Z`）——契约测试只钉前者与消息，时间戳格式留给 tracing 官方实现。
- **bdd 总览计数**：`单元测试` 一栏本次由 272 改为 283（按该节表格行数核对；此表不被 `check_test_bdd.py` 校验，属长期漂移，顺带修正）。

## owner 追加三项（2026-09-24）

### 1. 文件名 `gloss-<日期>.jsonl`

- `tracing-appender` 的日轮换把名字固定拼成 `前缀.日期.后缀`，出不了中划线，故自写 `DailyFileWriter`：写入时跨天即换文件、顺手清理超出 `MAX_LOG_FILES` 的旧档（名字按字典序即按日期序）。日期取 UTC，与它原本的口径一致。
- 日期分解用 Howard Hinnant 的 `civil_from_days`（不引日期库；只做日历分解，无时区与闰秒），用已知锚点测试钉住。
- 旧文件 `gloss.log.<日期>` 不再生成（历史文件留在目录里，新脚本只认新名字）。

### 2. 代数摊到顶层：`"span":{"name":"task","generation":2}` → `"generation":2`

- 先试过两条更"正统"的路，都被卡住：**自定义 `FormatEvent`** 需要 `tracing_serde::SerdeMapVisitor`（它是 tracing-subscriber 的传递依赖，直接使用要新增直接依赖），而**读 span 扩展**要 `tracing-core` 的 `registry` 特性（`tracing` 本身没有这个特性，声明不出来）。
- 最终做法：**保留内建 JSON 层，在写入器上做一层行整形**（`shaped` 纯函数 + 按行缓冲的 `ShapedHandle`）：把 `span.generation` 提到顶层、删掉 `span`/`spans`。解析失败的行原样透传——整形绝不吞日志。所有 sink（终端、文件、`capture` 系列）都接同一层。
- 代价：每行多一次 JSON 解析+序列化（日志不在热路径上）；好处是整形规则是我们自己的产物契约，可单测、可变异，且不依赖库内部结构。

### 3. 流程内日志的代数审计

- 逐个文件核对了流程上的日志点（`main.rs` 取材处理器、`pipeline.rs`、`engine/llm.rs`、`selection/{accessibility,clipboard}.rs`、`app/{channels,overlay}.rs`）：深层日志由任务 span（事件线程 `enter()`）与 `.instrument(span)`（tokio）覆盖，App 侧本来就带着 `generation` 字段。
- 只有两处流程内日志缺代数，已补：`overlay dismissed`（任务结果卡的收尾）与 `stale retry click dropped`。
- 流程外的日志（启动、热键注册、GPU、字体、设置窗、Dock 图标、鼠标监听降级等）**不加**代数——它们不属于任何任务。

## 里程碑 id 语义化改名（T3，2026-10-07）

- `m0_entry`…`m5_ready` → `01_entry`…`06_ready`：`m` 前缀与 JSON 字段名 `milestone` 冗余，且 m5a/m5b/m5c 与 m5_ready 的「同号异段」易误读；零垫序号保证「字典序 == 时序」，06a–06c 保留 a/b/c 子步骤分组。设计出处：`docs/plans/2026-10-07-perf-metrics-redesign.md`（六稿，搭 T3）。
- 消费面同步：`app-metrics.py` 的 `READY_MILESTONE` 常量与 docstring/help、`bisect_measure.py` 注释、`perf-chart.py` 面板说明、`tests/startup.rs` 头注释、`docs/tests/bdd.md` startup 条目。app-metrics 其余解析按 `milestone` 字段通用匹配，不依赖具体 id；单测探针 `m_test_probe` 不在启动序列，不改。

## 捕获串行锁 `test_support::lock_dispatchers`（2026-10-07，PR #129 CI 实证）

- 二次踩坑实证：gloss-core 单测二进制里 `classify_failure_falls_back_and_the_task_still_runs` 在 CI 并行跑时随机丢「classification failed」行（本地 30 连跑不复现，macos runner 负载谱不同）。此前「断言本测试独有的调用点用 capture 即可」的启发式**不可靠**——该调用点（engine.rs 的 fallback warn）在全二进制内只有这一个测试触发，仍被吞：Interest 缓存重建的吞事件窗口不区分调用点归属，任何线程进出 `with_default`（各测试的 capture 作用域）都可能撞上别的线程正在评估的事件。
- 修复选型：`capture_global`（gloss-app 的既有解法）在本二进制不可用——`init_is_idempotent` 调 `init(None)` 走 `subscriber.init()`（全局订阅者被占即 panic），与 `capture_global` 的 `set_global_default` 双向冲突（先后都坏，测试内注释早有记载）。故改用**测试互斥锁串行化**：`log::test_support::lock_dispatchers()`（cfg(test) pub(crate)）把本二进制全部会切换 dispatcher 集合的测试（log.rs 四个 capture 测试 + engine.rs 的 fallback 测试 + `init_is_idempotent`）圈进互斥区——捕获窗口期间不再有并发进出，重建窗口闭合。代价是这六个测试失去并行（均为亚毫秒级，可忽略）；断言若真失败锁会中毒，`into_inner` 继续拿锁让后续测试照常报告而非连锁 panic。

### capture 公开文档口径修正（PR #129 评审二轮，2026-10-07）

- 「线程局部捕获 vs 进程级捕获」一节的并发实证（Interest 缓存全局、线程局部
  订阅者进出使共享调用点事件被整条跳过）在 `capture` 的公开文档里没有体现，
  文档反而承诺「不干扰其它线程，因此可以并行调用」——评审指出承诺与实证直接
  矛盾。修正：文档改述「多线程可各自并发使用（各捕获各自线程的日志），但并发
  窗口内其它线程的共享调用点事件可能被整条跳过；独有调用点用 `capture`，共享
  调用点/跨线程一律 `capture_global`」。只改文档不改实现：实现语义本就如此，
  2026-09-23 起的测试用法（独有/共享分流）与此一致。

### capture 窗口内重建兴趣缓存：CI Coverage 偶发失败的根因修复（2026-10-08）

- CI Coverage job（PR #129，commit 7658ea3）实证 `engine::tests::classify_failure_falls_back_and_the_task_still_runs` 偶发 panic「the fallback must leave a trace」——cd050cf 的互斥锁没有闭合全部竞争。读 tracing-core 0.1.36 源码定根因：`NoSubscriber::register_callsite` 返回 **`Interest::never`**（subscriber.rs），而调用点兴趣缓存是**进程级单份**（`rebuild_callsite_interest` 的结果 `set_interest` 到全局调用点上）；宏层对缓存的 `never` 直接短路（`DefaultCallsite::interest` 命中缓存即返回，不进 `get_default`），线程局部订阅者无从生效。
- 竞争路径：任何**不持锁的普通测试**（engine/classify/config 的非捕获测试）在捕获窗口之外触发同一 warn 调用点的首次求值/注册时，线程上无订阅者语境（`get_default` 给到 `NONE`），`never` 被缓存——锁只串行化捕获窗口，管不住普通测试的求值时机。`init_is_idempotent` 的 `init` 触发的全量重算是唯一自愈点，重算之后再度污染则无处恢复。
- 修复：`capture` 在进入 with_default **窗口内**调用 `tracing::callsite::rebuild_interest_cache()`——重算走调用线程的 `get_default`，此刻是捕获订阅者，全部已注册调用点按它重新注册（warn → sometimes），窗口外的 `never` 污染被冲洗；窗口内首次求值的调用点注册同样落在捕获语境，自愈。残留边界：两个并发窗口之间普通线程对**尚未注册**调用点的首次求值仍可污染——共享调用点的确定性捕获仍归 `capture_global`（全局订阅者常驻后 `get_default` 恒有订阅者语境，不再产生 never）。
- 验证：`cargo test -p gloss-core --lib` 默认并行连跑 20 次 + `--test-threads=8` 连跑 8 次全绿（偶发竞争无法本地确定性复现，以机制分析 + 高频压测为证）。

## init 返回目录改拆包 move（P1-1 clone 削减，2026-10-13）

- `open_file_writer` 的三元组在 `Some` 臂里直接拆包：writer 进日志闭包、
  guard 进守护槽、目录作为返回值 move 出，原先为返回值额外
  `dir.clone()` 的一份 PathBuf 复制消失。init 进程级一次，纯省一次拷贝。

## re-export 词汇块：不按「零引用」删（失活代码清理，2026-10-16）

- 背景：全仓失活代码清理时扫到 `pub use tracing::{...}` 这一块，其中若干名字当日没有调用点
  （可靠判定为零引用的：`Level`、`debug_span`、`error_span`），按「零引用即删」的判据
  本应整块消失；最终判定**不删**。
- 理由：这块是 `AGENTS.md`「除 gloss-core 外不得直接依赖 tracing」那条约束的**公共面**
  ——下游要用哪个宏，只能经这里转发。按引用数增删等于把「下游将来能不能打这一类日志」
  绑死在当前调用点上，删了会让下游为了用 `trace!` 去改这个模块的公共 API。模块文档
  本身也明文推荐下游用 `info_span!` 配 `Instrument`（`task_span` 的用法），属对外契约。
- 因此源码里只留行为陈述（这块转发什么、新条目往哪里加），本条动机记在此处；
  后续任何按引用数做的清理都应跳过这一块。源码的措辞不复述「完整清单」——实际
  `span!` 等并未转发，`#[instrument]` 是过程宏也无法经 `pub use` 带出。
- **词频不可靠**：`trace` / `info_span` / `enabled` / `event` 这几个宏名与常见标识符
  同名，`git grep -w` 的计数在它们身上区分不出「宏调用」与「变量/字段/字符串」——
  第一版记录据此把 `warn_span` 也写成零引用，实际它是 `task_span` 的实现依赖
  （`log.rs` 内 `warn_span!("task", generation)`）。判断某个转发名是否有用，看它是否
  构成下游能力，不看计数。
- 同批删掉的 `LevelFilter` 判定不同：M1-T7 把过滤器改为字符串 `EnvFilter` 之后，全仓
  再无该类型的使用点，转发它不为下游提供任何能力（下游按级别过滤走 `EnvFilter`
  字符串或 `RUST_LOG`），属真残留。

## re-export 词汇块（源码注释迁入）

- 转发日志词汇（tracing 的宏与 `Level` / `Span` / `Instrument`）：下游 crate 经此入口用日志，需要的新条目也从这里加。

## thread 模块（源码注释迁入）

- 线程角色标签，供 `thread` 字段使用。`UI` 主线程：winit 事件循环 + UI；`EVENT` 平台事件线程：鼠标手势与取材命令消费；`MOUSE_TAP` 鼠标 tap 监听线程：全局事件 tap 的独立宿主；`FONTS` 字体装载线程：系统字体延迟装载的后台宿主；`TOKIO` tokio 后台：网络请求与缓存。

## 常量与静态（源码注释迁入）

- `DEFAULT_FILTER`：基准级别，始终作为打底项，`RUST_LOG` 的模块指令在其上叠加（全局指令可压制它）。
- `FILE_PREFIX` / `FILE_SUFFIX`：日志文件名 `gloss-<UTC 日期>.jsonl` 的两段。
- `MAX_LOG_FILES`：按天保留的日志文件个数（约一周）。
- `FILE_GUARD`：非阻塞写盘的 guard，必须活到进程结束。

## init（源码注释迁入）

- 初始化全局日志：`info` 打底，`RUST_LOG` 在其之上追加或覆盖。例如 `RUST_LOG=gloss_platform=debug` 让该模块更详细、其余保持 `info`；`RUST_LOG=off` / `warn` 等全局指令仍可整体压制。
- `dir` 给出日志目录时终端与文件双写（按天滚动，只留最近 7 份）；目录建不出来则退回 stderr 单路——日志不可用不应该拖垮启动。返回实际启用的目录，供入口打印出来方便定位。
- 全进程只调用一次（入口 `src/main.rs` 启动第 1 步）；重复调用为空操作，库 crate 永不调用。
- 目录随拆包 move 出来（writer 进日志闭包、guard 进守护槽），不为返回值另存一份副本（详见「init 返回目录改拆包 move」）。
- `FILE_GUARD` / `FILE_DIR` 的 `OnceLock::set` 在 `INIT.call_once` 内首次执行，已占用 Err 不可能发生，丢弃即可。

## task_span（源码注释迁入）

- 任务 span：以 `generation` 字段承载请求代数，范围内所有日志自动带上它。在触发点创建（代数在那里赋值），句柄随通道下发到平台事件线程与 tokio——深层的日志点（取材读选区、推理引擎）因此不必自己携带代数。无订阅者的进程里是禁用 span，进入它是空操作。
- 级别取 `WARN` 而不是 `INFO`：span 被级别压掉时**字段也不会出现**，而任务作用域里要留住的恰是 warn 及以上那几行（拒绝授权、后台 panic）——排障时最需要它们。作用域内没有 error 级日志，故 warn 覆盖了「任何可能打印的级别」；`RUST_LOG=error` 时这些行同样被压掉，不丢信息。

## MILESTONE_START 与 milestone（源码注释迁入）

- `MILESTONE_START`：启动里程碑计时原点，首次 `milestone` 调用即进程入口，其后各里程碑的 `elapsed_ms` 都相对它累计。
- `milestone` 记一条启动里程碑：`milestone`（id）与 `elapsed_ms`（相对进程入口累计，单调时钟）两个顶层字段。埋点位置按启动序列定：进程入口、日志/配置/引擎就绪、事件循环装配完成、渲染栈建立（再分窗口/设备/帧三段，06a–06c）与预热帧后就绪（06_ready）——量化脚本按 id 分段归因启动耗时（消费方见 `scripts/perf/app-metrics.py`）。

## json_layer / shaped / shaped_handle（源码注释迁入）

- `json_layer`：日志行格式（两路共用），JSON Lines 一行一个对象——`timestamp` / `level` / `target` 固定，事件字段与任务代数摊在顶层（见 `shaped`）。JSON 本身不带颜色转义，无需 ANSI 开关。
- `shaped`：日志行整形——内建 JSON 形态把 span 字段嵌在 `span` 对象里，本仓只要代数——把它提到顶层（与事件字段同层），并去掉 `span` / `spans`。解析失败的行原样写出，整形不会吞日志。
- `shaped_handle`：按行缓冲、整行到手再整形的写入句柄；交给 `with_writer` 的闭包构造。

## open_file_writer / DailyFileWriter（源码注释迁入）

- `open_file_writer`：按天滚动的文件写入器；目录不可用时返回 `None`，让日志退回 stderr 单路。
- `DailyFileWriter`：按天滚动的 JSONL 写入器，当前文件是 `gloss-<UTC 日期>.jsonl`，写入时跨天即换文件并清理超出 `MAX_LOG_FILES` 的旧档。日期取 UTC——与 tracing-appender 的日轮换同一口径，换掉它的原因是文件名要 `gloss-<日期>.jsonl`（它固定拼成 `前缀.日期.后缀`）。
- `open_today`：打开今天的文件；跨天时才换文件、顺带清理旧档。

## today_utc / civil_from_days / prune_old_logs（源码注释迁入）

- `today_utc`：今天（UTC）的 `YYYY-MM-DD`，只做日历分解，不涉时区与闰秒。
- `civil_from_days`：自 1970-01-01 起的天数 → 公历年月日（Howard Hinnant 的 `civil_from_days`）。
- `prune_old_logs`：只保留最近 `MAX_LOG_FILES` 份日志——名字里的日期可按字典序比较。

## capture（源码注释迁入，补充公开契约）

- 在捕获订阅者下运行 `f`，返回这段时间里格式化后的日志文本（含 span 字段）。
- 订阅者是**线程局部**的：与全局 `init` 互不影响，多个线程可各自并发使用，各自捕获各自线程上产生的日志。但**并发不等于不干扰**：tracing 的调用点兴趣缓存是进程级单份，未被捕获窗口覆盖的线程打日志时会把调用点缓存成 `never`（无订阅者语境的注册结果），此后任何捕获窗口都整条收不到该调用点——因此本函数在窗口内先重建一次兴趣缓存（按捕获订阅者重新注册全部调用点），把窗口外的污染冲洗掉。并发窗口之间仍有残留竞争：断言共享调用点或跨线程一律用 `capture_global`（全局订阅者常驻，缓存按它计算后不再被无订阅者语境改写）。
- 窗口内重建的实现机理：缓存是进程级单份而兴趣按订阅者计算，窗口外的无订阅者求值会把调用点缓存成 never（`NoSubscriber::register_callsite` 的返回值），macro 层对 never 短路、根本不进 `get_default`，线程局部订阅者无从生效——在窗口内重建，让全部已注册调用点按捕获订阅者重新计算。
- 过滤器取 `init` 的同一档基准级别，所以断言同时钉住了「这条日志在生产默认级别下也打得出来」。
- 两个坑：**span 必须在 `f` 里创建**——tracing 在创建时就按当时订阅者的兴趣定启用与否，没有订阅者时创建出来的 span 永远是禁用态，事后再捕获也补不回来；以及 `f` 内新起的线程不在此订阅者覆盖范围内（跨线程断言用 `capture_global`）。

## capture_global / CaptureReader（源码注释迁入）

- `capture_global` 装上**进程级**捕获订阅者，返回读取端；日志来自别的线程时用它。与 `init` 互斥（一个进程只有一个全局订阅者）：测试进程里没人调用 `init`，同一进程重复调用本函数拿到同一个读取端（只装一次）。
- `set_global_default` 失败被忽略的语境：测试进程里没有别的全局订阅者；万一已有，说明调用方用错了工具，这里按「捕获不可用」继续，断言会如实失败。
- `CaptureReader`：`capture_global` 的读取端，按需取走累积到此刻的日志文本；`text()` 每次调用取一份快照，不清空缓冲。

## build_filter（源码注释迁入）

- 基准级别打底 + `raw` 追加：全局指令后写者胜，故 `off` / `warn` 仍能压制打底级别，而模块指令只影响自己的 target。非法指令由 `EnvFilter` 丢弃并打印原因。
