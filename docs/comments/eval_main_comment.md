# eval main.rs（gloss-eval CLI）决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。

## REPORT_DIR

- 报告目录（与 dev-plan 约定一致：报告出 target/eval/，不进仓库）。

## run_report_cli

- 读评测产物目录里最近一次迭代报告（`iterate.json`）重渲染为 Markdown 打到
  标准输出；缺文件带修复指引报错（先跑 `eval iterate`），不静默空输出。
  2026-11-01 评测工具链重构步骤 1：重放与实跑轨删除后，报告子命令指向
  迭代产物的目标契约（重构计划 §6.1），迭代实跑在后续步骤接线。

## run_history_cli

- 从现有 `history.jsonl` 重绘 `<dir>/history.svg`（流水线重绘与本地补绘共用；不跑评测）。

## dir_option

- `--dir PATH` 选项：缺省回落报告目录（GLOSS_EVAL_DIR / target/eval）。

## run_gate_cli

- 2026-11-01 门禁子命令在精度召回表之后追加动作收敛表（remedy 模块渲染）：
  判定器的第二口径与门禁指标同出一份 gate.md；两份内嵌数据集装载失败都带
  前缀报错上抛，不静默跳过。
- 2026-11-01 评测工具链重构步骤 1：`GLOSS_LIVE_JUDGE=1` 双保险随旧实跑轨
  删除，评委复核只凭 `GLOSS_LIVE_*` 凭据显式选择参与（凭据缺席即带修复指引
  报错）；复核有真实计费，仍不进持续集成。

## LiveEnv / live_config / EnvSecretStore

- 端点装配（凭据环境变量读取、引擎配置组装、密钥脱敏 Debug）收口在 CLI
  薄壳：库层零环境副作用。2026-11-01 自 runner 模块迁入——实跑轨删除后
  唯一消费者是门禁评委复核；密钥只经 SecretStore 端口注入引擎，Debug 形态
  只显式区分“在/不在”，值不落任何输出。
