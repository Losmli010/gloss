# eval.yml 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），workflow 注释里只留行为与用法，取舍记在这里。
> 需求原文：docs/plans/2026-10-12-eval-history-svg-plan.md §3.4。

- 2026-10-12 runner 用 ubuntu-latest（有意偏离 perf.yml 的 macos-26）：perf 采集
  对象是 macOS 应用性能，runner 平台即测量平台；replay 轨是纯 serde/JSON 逻辑，
  不依赖 macOS/GPU/窗口服务，ubuntu runner 更快也更便宜。执行时若发现平台差异
  （如行尾、文件系统大小写）再对齐。
- 2026-10-12 采集步骤不用 `GLOSS_EVAL_COMMIT=$(git rev-parse --short HEAD)` 内联
  求值，而用 `Record main head` step 的 output（perf.yml 同款）：一次求值两处
  复用（env 注入与数据分支命名），避免两处各自跑 git 出现理论上的不一致窗口。
- 2026-10-12 artifact 上传挂 `if: always()`：replay 失败时报告碎片（失败前的
  history.jsonl 半截等）同样是排障证据；`if-no-files-found: ignore` 兜住全无
  产物的情形。step summary 贴 replay.md 同理挂 always。
- 2026-10-12 入库只拷 history.jsonl + history.svg 两个文件、先 mkdir -p
  baselines：baselines/ 目录随首个数据 PR 才产生，workflow 不能假设它已存在；
  `<mode>.json/.md` 留 artifact 不进 git（数据分支的 PR diff 只含趋势点，
  审查面最小）。
- 2026-10-12 push 用 `--force`：数据分支键是 eval/collect-<sha>，同 commit 重跑
  是覆盖语义（upsert 键 (mode, commit) 同样覆盖），分支可再生，force 与数据
  语义一致（perf.yml 同款）。
- 2026-10-12 PR 建议命令的 body 用 `--body ''`：perf 的建议带 perf-report 对比
  段（有跨点对比工具）；eval 侧跨点对比工具不在计划范围（§3.4 摘要行），先留
  空 body，工具立项后再补。
- 2026-10-12 未加 `targets:` 到 rust-toolchain step：perf 需要双架构体积测量才
  装 x86_64 target；eval 无交叉构建需求。
- 2026-10-12 concurrency 组名 eval-data 与 perf-data 分开：两条数据线互不相关，
  共组会让一方的例行采集取消另一方在跑的采集。
