# eval main.rs（gloss-eval CLI）决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。

## 定位（薄壳）

- 六个子命令（iterate、label、accept、report、history、gate）的参数解析、环境读取与分派：三条流的编排住在库里的 flow 模块，本壳只传参与打印——库层零环境副作用，凭据与目录环境变量的读取收口在此。

## run_iterate_cli

- 2026-11-02 评测工具链重构步骤 6 接线：评审默认开启（评审是流程核心信号，删除清单明令撤掉 `GLOSS_LIVE_JUDGE` 双保险），`--no-judge` 跳过额外一次模型调用；`GLOSS_LIVE_*` 三变量缺一即错退出码 1（实跑本身就需要模型端点，与评审开关无关）。
- 报告上下文（`GLOSS_EVAL_COMMIT`、`GLOSS_EVAL_SOURCE`）在此读环境注入 `EvalContext`——历史行的同键原地覆盖以提交标识为键，环境缺席即每次追加新行。
- 产物目录 `GLOSS_EVAL_DIR` 缺省回落 target/eval；成功后打印一行汇总并把报告的 Markdown 表打到标准输出（迭代本身写四件产物，标准输出只是人读的即时回执）。

## run_label_cli / run_accept_cli

- 2026-11-02 步骤 6 接线：离线两命令，无凭据要求。`--import` 缺参即用法的用法错；集外标识的跳过与未标注的固化都只打警告不阻断（裁决权在人），逐条警告到标准输出。

## run_report_cli

- 读评测产物目录里最近一次迭代报告（`iterate.json`）重渲染为 Markdown 打到标准输出；缺文件带修复指引报错（先跑 `eval iterate`），不静默空输出。
- 文件名从报告模式的单一来源（`ITERATE_MODE`）派生，不再写死字面量。2026-11-01 评测工具链重构步骤 1：重放与实跑轨删除后，报告子命令指向迭代产物的目标契约（重构计划 §6.1）。

## run_history_cli

- 从现有 `history.jsonl` 重绘 `<dir>/history.svg`（流水线重绘与本地补绘共用；不跑评测）；文件名引用历史层的常量，不写字面量。

## dir_option / parse_usize / report_dir / env_value

- `--dir PATH` 选项：缺省回落报告目录（GLOSS_EVAL_DIR / target/eval）；`--limit` 数值解析全命令共用一个助手；环境值读取统一过滤纯空白。

## run_gate_cli

- 2026-11-01 门禁子命令在精度召回表之后追加动作收敛表（remedy 模块渲染）：判定器的第二口径与门禁指标同出一份 gate.md；两份内嵌数据集装载失败都带前缀报错上抛，不静默跳过。
- 2026-11-01 评测工具链重构步骤 1：`GLOSS_LIVE_JUDGE=1` 双保险随旧实跑轨删除，评委复核只凭 `GLOSS_LIVE_*` 凭据显式选择参与（凭据缺席即带修复指引报错）；复核有真实计费，仍不进持续集成。

## LiveEnv / live_runtime / live_config / EnvSecretStore

- 端点装配（凭据环境变量读取、引擎配置组装、密钥脱敏 Debug）收口在 CLI 薄壳：库层零环境副作用。2026-11-01 自 runner 模块迁入；2026-11-02 步骤 6 抽出 `live_runtime` 供迭代实跑与门禁评委复核共用——引擎与配置句柄的组装只此一份；密钥只经 SecretStore 端口注入引擎，Debug 形态只显式区分“在/不在”，值不落任何输出。
