# eval main.rs（gloss-eval CLI）决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。

## REPORT_DIR

- 报告目录（与 dev-plan 约定一致：报告出 target/eval/，不进仓库）。

## run_inspect_into

- inspect 轨的完整落盘段：与 replay 同一份报告与历史，多出检视件。

## write_inspect

- 检视件落盘：逐条明细 `<mode>.cases.json`（可 diff、可再加工）与单文件检视页 `inspect.html`。历史在 `finish_run_report` 追加之后才读——差值基准取「上一次同 mode 运行」，趋势图含本次。

## run_replay_into

- 重放轨的完整落盘段：目录由参数表达 `GLOSS_EVAL_DIR` 的语义，context 由调用方注入（环境读取只在 CLI 薄壳），测试直驱临时目录。

## finish_run_report

- 报告与趋势的统一落盘：先写 `<mode>.json`/`.md`，成功后追加 `history.jsonl` 并重绘 `history.svg`（任一步失败向上传播，不静默）。

## run_history_cli

- 从现有 `history.jsonl` 重绘 `<dir>/history.svg`（流水线重绘与本地补绘共用；不跑评测）。

## dir_option

- `--dir PATH` 选项：缺省回落报告目录（GLOSS_EVAL_DIR / target/eval）。

## eval_context

- 采集上下文注入：GLOSS_EVAL_COMMIT / GLOSS_EVAL_SOURCE 只在本 CLI 读取（lib 层零环境副作用）；两者皆缺省时不产 context 段。

## upsert_fixtures

- 把录制按 id upsert 进夹具文件（分类/任务各自归位）。

## merge_fixtures

- 单个夹具文件的 upsert：读原文件、剔除被录制覆盖的 id、追加录制并原子写回，返回追加的条数。
