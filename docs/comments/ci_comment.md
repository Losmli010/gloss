# ci.yml 决策记录

> 按符号锚定（不写 job/步骤名，不写行号，防漂移）。改到相关配置时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），文件内不留指向本文件的指针。

## 顶层 permissions

- 2026-09-18 artifact 上传在 upload-artifact v4 下同样只需 read token，故顶层 contents: read 即可。

## Lint (clippy) job

- 2026-09-18 单独占一个 macOS job 的原因：Clippy 需要编译整个 workspace，目标产物链接平台框架（ApplicationServices / CoreGraphics 等），只能在 macOS 上构建。

## Test job

- 2026-09-18 测试在 macOS 跑的原因：窗口服务、GPU 与辅助功能行为只有目标平台能覆盖。

## Run coverage 步骤

- 2026-09-18 与本地共用 just coverage：避免 CI 与本地两套命令漂移。
- 2026-09-18 关掉 cargo 的着色：避免 ANSI 转义进 job summary。

## 性能信号 artifact（Test job）

- 2026-09-21 只有 Test job 设 GLOSS_PERF_OUT、coverage job 不设的原因：一次 PR 两处都会跑全部测试，设两份只产生重复记录；覆盖率统计不需要性能文件。（该环境变量已于 2026-10-07 废除。）
- 2026-10-07（T3）改为 wrapper 提取物：GLOSS_PERF_OUT 文件导出废除，Test job 在 just test 之后跑 just selftest-report（双自检再跑一遍，构建缓存下成本低）产出 target/perf/selftest.jsonl，上传该文件替代原 overlay.jsonl artifact；决策细节见 selftest_wrapper.py_comment.md。
- `if: always()` + `if-no-files-found: ignore` 语义保留：预算失败运行的数字同样要留证，wrapper 未产出文件时不应让上传步骤报错。
- 保留 90 天、artifact 名带 run id 的原因：CI 数字负载敏感、不进趋势对照，只作事后审计取证，按 run 可追溯即可。

## Upload HTML report 步骤

- 2026-09-18 `if: always()` 的原因：阈值没过时也要拿到报告，否则无从判断是哪个文件把覆盖率拉了下来。
