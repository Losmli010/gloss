# history.rs（gloss-eval）决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。

## upsert 键改借用比对（P1-1 clone 削减，2026-10-13）

- `upsert_key` 改返回 `(&str, &str)` 借自报告本体，`parsed_key` 因行内
  报告是临时解析产物维持有副本；比对处先绑定 `parsed_key` 结果再比
  `&str` 元组。行为不变（无 commit 的行不产键恒追加）。
- 行内命中后的 `line.clone()` 保留：upsert 语义是“同键行**全部**原位
  重写”，正常文件至多一行命中，但手改文件出现重复键时 take 式改写会把
  第二行置空——为守住这一未定义输入下的旧行为，这份复制不省。

## write_atomic

- 原子写盘：写同目录临时文件再 `rename` 覆盖。`std::fs::write` 直接写目标，进程中断会把受控工件截断成半截；临时文件 + `rename` 让中断只丢临时文件，目标保持完整。临时文件与目标同目录，`rename` 不跨文件系统、原子生效。
- 临时名带进程 id：跨进程并发写同一目标（如两条评测命令同目录落盘）不竞争同一个临时文件。
- rename 失败时清理临时文件（尽力而为，清理失败无关紧要），错误原样上抛。

## HISTORY_FILE / SVG_FILE

- `HISTORY_FILE` 历史文件名（与报告同目录）；`SVG_FILE` 趋势图文件名（每次记录后全量重绘）。

## record_run

- 记录一次运行：upsert 一行并从全量历史重绘 `<dir>/history.svg`；失败向上传播（趋势断更好过静默缺数）。
- upsert 键 = `(mode, context.commit)`：`commit` 为 `None` 永不参与匹配（本地运行恒追加）；同键原位重写该行，其余行不动。JSONL 本身不裁剪。

## load_history

- 容错读取 `history.jsonl`：缺文件=空历史；空行跳过；坏行 stderr 警告（带行号）后跳过——进程中断留下的半截尾行不炸整个趋势。

## upsert_key

- upsert 键：`(mode, context.commit)`；`commit` 缺省即不产键——本地行永不参与匹配，恒追加。借自报告本体（`report` 在整个 upsert 比对期间存活），不复制两段文本。

## parsed_key

- 已有行的 upsert 键；解析失败或无 commit 的行返回 `None`（不参与匹配、原样保留）。行内报告是临时解析产物，键随之取自有副本。

## warn_bad_row

- 坏行警告经 stderr 直写：本模块服务于评测 CLI 的降级路径（无 subscriber、无长驻进程），与 main.rs 同一豁免面，故就地显式豁免。
