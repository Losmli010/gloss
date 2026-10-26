# metrics.py 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），文件内不留指向本文件的指针。
> T1（PR：feat/perf-metrics-registry）初建；T6 收尾时按最终脚本形态续写。

## 定位（2026-10-07，T1）

- 注册表是 perf-collect / perf-chart / perf-report 的唯一指标词典：id、标签、
  单位、判定语义、噪声阈值、预算常量映射都从这里取，消费方不各存一份——
  否则「加一个指标要改三处」正是旧管线（bench-baseline/perf-history）的病根。
- 预算数值不走注册表（设计 2.1 数据流）：Rust 常量 → 自检 stderr 结构化行 →
  history 点 budgets 段 → 图表取最新携带预算的点。注册表只记常量名与位置
  （BUDGET_CONSTS），值随数据走、名随代码走，任何环节不手写数值。

## CATEGORIES / 词表（2026-10-07，T1）

- 单位词表钉五种 ms/kb/count/ns/bytes，与设计 2.3 草图一致；设计 2.2 表给
  clone.density 标的「ratio」不在词表内，实现取 count——密度数值本质是
  千行计数，口径（次/千行）已写进 label；fmt 对非整数 count 保留一位小数
  恰好覆盖密度（如 5.4）。
- Metric 增加 note 可选字段（默认空）：设计 2.2 判定列带括注（eval 的
  「外部服务波动」、frame_missed 的「仅报告呈现」），而任务卡把 judge 钉死
  三档字符串；括注无处安放就会逼报告硬编码第二份口径，note 随 judge 一并
  显示，保持注册表单源。
- SOURCES 词表按草图六值（app-metrics / overlay-selftest / startup-selftest /
  clone-stats / criterion / eval-live）；clone.allocs/bytes 的源归 criterion
  （分配计数产自 target/criterion 的 clone_allocs/clone_bytes 组），设计 2.2
  表注「criterion（clone 基准）」的括注由 judge=精确计数门禁 表达，不另立源。

## METRICS 登记取舍（2026-10-07，T1）

- app.startup_ms 的 label 写「spawn→就绪里程碑」而不钉「06_ready」：T3 与
  T1 并行、正在改里程碑 id（m5_ready→06_ready），标签不随其漂移；T3 合入
  后若图表需要可在 label 里补回具体里程碑 id。
- core 15 项用 CORE_GROUPS×CORE_CASES 展开，与 benches/core.rs 的四组用例
  一一对应（组名中译：cache_key 缓存键 / complete 结构化解析 /
  prompt_render 提示词渲染 / task_cache 任务缓存）；id 取 criterion full_id
  加 core. 前缀，即 core.<group>/<case>。benches 增删用例时同步这两个表。
- eval 两项 noise=0.20：LLM 端到端延迟的服务波动远超仓内指标，默认 5% 会
  把噪声全判成回归；其余仅记录项维持默认 0.05。启发式阈值，输出仍须标
  「非门禁，仅参考」。

## fmt（2026-10-07，T1）

- kb→MB 换算阈值取 10 MB：设计 4.4 报告示例要求小值保持 KB（「340 KB」、
  「预算 2,048 KB」）而体积示例转 MB（104212 KB → 101.8 MB），阈值必须同时
  满足两例；demo SVG 里 259800 KB 渲染为 253.7 与 2,048 KB 并存，印证该分界。
  1024 进制（与 macOS 内存口径一致）。
- ns 按量级自动落 ns/µs/ms 档：criterion 值跨三个数量级（cache_key 短文本
  数百 ns，image/4mb 毫秒级），固定 ns 千位分隔不可读；与 bench-summary.py
  的 human_ns 同构，但精度取一位小数（趋势对照不需要三位）。
- 单位不在词表内直接抛错：拼错单位后静默输出错误格式比当场失败更贵，尤其
  单位字符串要从注册表流向两三个消费方。

## _validate（2026-10-07，T1）

- import 时即校验（类别/单位/判定/数据源在词表内 + 预算门禁与 BUDGET_CONSTS
  双向一致）：注册表是单源词典，漂移后第一个踩坑的是下游图表/报告的渲染
  路径，在 import 处失败最便宜；零依赖零 IO，校验成本可忽略。
- core 15 项计数不做进 _validate：计数自证（与 CORE_CASES 展开结果恒等），
  真正的同步点是 benches/core.rs（注释已标），校验它与注册表需要读源文件，
  import 时不做 IO。

## CORE_CASES["complete"] 去 ocr/ok（2026-10-10，契约校验与重试方案步骤 1a）

- benches/core.rs 的 `complete/ocr/ok` 用例随 `TaskKind::ImageOcr` 删除（它测的就是 OCR 路径的 `parse_structured`）；注册表按文件头「benches 增删用例时同步这里」的约定同删。history.jsonl 是历史采集记录，旧点里的 `core.complete/ocr/ok` 数值保留原样（趋势对照只认同机历史，不改写）。

## 文件头迁出的用法与示例（2026-10-24，注释纪律）

- 自检入口：`python3 scripts/perf/metrics.py` 打印全部注册项与 fmt 示例（核对清单）。
- 编程入口：`from metrics import METRICS, BUDGET_CONSTS, fmt`；示例 `fmt(1498, "ms")`
  → `"1,498 ms"`、`fmt(104212, "kb")` → `"101.8 MB"`。
- 消费方：perf-collect / perf-chart / perf-report；单位词表 `ms` / `kb` / `count` /
  `ns` / `bytes`；预算数值源头是 Rust 常量（随自检 stderr 结构化行流出，collector
  折算进历史点 budgets 段），注册表只记常量名与所在文件（BUDGET_CONSTS）。
