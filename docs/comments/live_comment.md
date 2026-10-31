# live.rs（gloss-eval runner 子模块）决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。

## LiveRun / LiveOptions

- `LiveRun`：live 轨的一次运行产出——报告 + 逐条明细 + 夹具录制（调用方决定落盘哪些）。`report` 评测报告（含延迟与可选 judge 摘要）；`cases` 逐条明细（含引擎失败条目——它们也要被检视到）；`recordings` `--record` 时的修复轨迹录制（分类 + 任务，按 id 可 upsert 回夹具）。
- `LiveOptions`：live 轨的选项。`record` 把逐条修复轨迹交回调用方（main 据此 upsert `fixtures/*.jsonl`）；`judge` 启用 judge 评分（额外一次模型调用/条）；`limit` 每个数据集最多评测多少条（冒烟用）。

## LiveEnv

- live 轨的端点配置（全部来自 `GLOSS_LIVE_*` 环境变量，缺一即失败）。
- `Debug` 手写脱敏：key 不进任何日志面（密钥红线），排查时只能看到“有/无 key”。
- 字段：`base_url` OpenAI 兼容端点；`model` 本次评测使用的模型 id（对全部 kind 生效，含分类）；`api_key` API key（只进请求头；不进日志与报告）。

## LiveEnv::from_env

- 从 `GLOSS_LIVE_BASE_URL` / `GLOSS_LIVE_MODEL` / `GLOSS_LIVE_API_KEY` 读取；缺失时带修复指引报错（与平台 live 测试同一套变量）。

## live_config

- 组装 live 轨的配置句柄：端点与模型对全部 kind 生效，密钥经内存存储直查（不落盘、不进日志）。

## LiveCtx / LiveAcc / JudgeTracker

- `LiveCtx`：一次 live 运行共享的执行环境——引擎、配置句柄与选项；两个轨段与 judge 评分都要用，捆在一起避免逐函数透传三件套。
- `LiveAcc`：一次 live 运行的横切账本——延迟样本、逐条明细与夹具录制，两个轨段顺序追加，末尾随 `LiveRun` 一次交出。
- `JudgeTracker`：judge 评分的横切账本——尝试数与得分数分开记（全失败时 `attempted` 仍要出现在摘要里）。

## JudgeTracker::summary

- judge 启用即产摘要：全失败（count=0）时 `attempted` 仍记录尝试数，与“未启用”（judge 段整体缺席）区分开——付费调用全失败不能无痕迹。

## run_live

- live 轨：数据集逐条打真实 LLM。延迟只在这一轨采集。
- `engine` 与 `config` 由调用方组装（`main` 从环境变量构造）——runner 本身不读环境，保持可测。分类走 `classify_traced`、任务走 `run_task_rounds`（生产的同一循环）：违约类别与修复轮次来自轮记录，与重放轨同口径。

## live_classify 的引擎失败分账

- 引擎失败单列（网络/限流与模型行为无关），但也计入回退率（生产编排对这类条目同样落兜底）；逐条继续——评测要跑完全集而不是中途夭折。

## live_task_dataset 的记账与轮记录

- 与分类轨同一“跑完全集”策略；差别在记账——分类轨把引擎失败单列 engine_errors，任务轨并入 degraded（live 的降级率因此混入传输失败，见 TaskMetrics 文档）。
- 轮记录来自 run_task_rounds 本身：修复轮次与 RunOutput.repairs 同源同值（on_repair 与修复计数器逐一对应），折算走与重放轨同一份明细结算（task_parts 内部即 for_reply + 逐轮轨迹）。
- 夹具落盘放在轮记录的最后一次使用之后：逐轮文本 move 进夹具，不再逐轮复制（落盘顺序与逐条顺序不变——每条至多一次 push）。分类轨同此。

## run_task_case

- 任务请求：与生产 execute 同一条渲染路径（`PromptRegistry`）+ 生产的任务段修复循环（`run_task_rounds`）。返回 `RunOutput` 与逐轮完整回复——轮边界由 `on_chunk`/`on_repair` 还原，与 `RunOutput.repairs` 一致（生产循环里两者逐一对应）。
- 轮记录取走即用：闭包与 Arc 的存亡不再关心（数据已 take 出来）。

## judge_one

- judge 单条评分；judge 模型与任务模型同端点同 id。

## collect

- 消费一条引擎流并累积完整回复（judge 评分请求专用——评分不计修复轨迹，单轮口径）。回复累积走生产的 `push_capped`（与分类编排同一道 4KiB 界，不守约的端点不能把内存灌穿）。

## fixture_from_rounds

- 轮回复 → 夹具轮次：live 的轮数据是完整回复文本，落盘按单增量成轮（重放只消费拼接结果，增量切分不进任何指标）。轮文本随 `Vec` move 进夹具——调用方在轮记录的最后一次使用之后才转交所有权。

## EnvSecretStore

- live 轨用的密钥存储：key 从环境变量来，只进请求头。两个端口的其余方法按 `ConfigStore` 与 `SecretStore` 契约给无害实现（评测不落盘、不读文件；`ConfigStore` 那份只为喂配置句柄的构造子）。
