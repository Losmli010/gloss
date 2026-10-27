# runner.rs（gloss-eval）决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。

## 模型 id 取自快照借用、夹具轮 move（P1-1 clone 削减，2026-10-13）

- 分类轨与任务轨的 `config.snapshot().model.clone()` 改为快照绑定局部后
  `as_str()` 借用直传（`classify_traced`/`run_task_rounds` 收 `&str`）；
  judge 评分请求里的复制保留——`EngineRequest.model` 是按值的 String
  （引擎端口 'static future 要求所有权），快照是 Arc 共享取不出字段，
  那一份是端口形制下的必要拷贝。
- `live_config` 改按值收 `LiveEnv`：调用方（bin main 与测试）此后不再
  使用 env，三段字段复制（base_url/model/api_key）随之消失。
- `fixture_from_rounds` 改收 `Vec<String>` 并在轮记录最后一次使用之后
  才转交（调用点重排：夹具落盘 push 挪到 judge 之后）——逐轮文本 move
  进夹具，不再逐轮复制；每条用例至多一次 push，落盘顺序不变。

## ReplayRun

- 重放轨的一次运行产出：报告 + 逐条明细（检视页消费）。`report` 评测报告；`cases` 有夹具条目的逐条明细（跳过条目不留行，与报告分母一致）。

## run_replay / run_replay_detailed

- 重放轨：数据集 × 夹具，经生产解析函数算指标。无网络无凭据。
- 只统计有夹具的条目（其余计入 skipped）——夹具是维护的受控工件，数量决定确定性轨的覆盖面。夹具轨迹必须与生产的修复循环同形（末轮之前每轮都违约、修复轮数不超生产预算），漂移的夹具按硬错误拦下（`check_classify_trajectory` / `check_task_trajectory`）。
- `run_replay_detailed` 是 `run_replay` 的带明细版本：同样的轨迹与口径，额外逐条落行供检视页。

## replay_classify / replay_tasks

- `replay_classify`：分类轨重放——逐样例校验轨迹并结算指标与明细，无夹具的样例计入跳过。
- `replay_tasks`：任务轨重放——逐数据集校验轨迹并结算指标与明细，无夹具的样例计入跳过。

## score_classify_case

- 一条分类样例的结算（重放与 live 共用同一口径）：违约轨迹并入指标、计一条已评估，按期望 kind 判分，并落一行明细。

## TASK_DATASETS / round_texts

- `TASK_DATASETS`：任务数据集清单（名字 → 内嵌资产）。
- `round_texts`：夹具轮次 → 每轮完整回复（增量按到达序拼接，与生产流式累积一致）。

## check_classify_trajectory / check_task_trajectory

- `check_classify_trajectory`：夹具轨迹体检（重放轨专用）——夹具是受控工件，分类轨迹必须与生产的修复循环同形：修复轮数不超 `CLASSIFY_REPAIRS`，且末轮之前的每轮都违约（修复轮由违约触发）。漂移的夹具会把指标带偏，按硬错误拦下。
- `check_task_trajectory`：夹具轨迹体检（重放轨专用，任务版）——修复轮数不超 `TASK_REPAIRS`，末轮之前的每轮都违约（`gloss_core::schema::validate_task`）。

## absorb_classify_track

- 把轨迹的违约计数并入分类指标（任务指标走 `TaskMetrics::record`，计数逻辑在 `TaskVerdict` 里同源折算）。
