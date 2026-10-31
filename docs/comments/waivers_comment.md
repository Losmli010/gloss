# 评审豁免留痕

> 按 AGENTS.md 约定：LOW 级评审问题修复成本低则顺手修复，否则豁免；豁免条目在此留痕（步骤、问题、理由）。

## 步骤 1a（契约校验与重试方案 §10 第一条）—— README.md M5 里程碑措辞超字面范围（2026-10-11）

- 问题（评审 LOW）：README.md“开发状态”的 M5 里程碑行由“框选截图 → OCR / 图片解释”改为“框选截图 / 剪贴板图片 → 图片解释”，超出 §10 删除清单第一条的字面范围——方案把文档同步归步骤 6（AGENTS.md 与模块文档）。
- 豁免理由：该措辞漂移是删除 `TaskKind::ImageOcr` 的直接后果——OCR 任务类型已不存在，保留旧措辞会向读者描述一条仓库里没有的产品路径；改动单行（d679f0b），有决策记录（本目录 `README.md_comment.md` 2026-10-11 小节：两路图像取材来源、产物 kind 恒为 ImageExplain、M5 复选框不动）；评审明示“可接受保留在本 PR”，不挪步骤 6。

## 步骤 3（契约校验与重试方案 §12 步骤 3）—— eval 回放夹具仍为旧契约（2026-10-11）

- 问题（评审 LOW）：`crates/gloss-eval/fixtures/task_replay.jsonl` 仍是旧扁平/围栏契约回复，在新校验器下整体落降级轨，`just eval-replay` 的任务降级率在重录前虚高、不可按面值解读。
- 豁免范围与理由：**夹具重录本身**豁免（不在本 PR 修）——方案 §12 明示“夹具改嵌套 + 异常夹具（空回复/散文/缺 note/kind 越清单）”是步骤 7 的条目，提前重录会与步骤 7 的异常夹具、违约/修复轮次统计拆成两半做。已按评审建议补了过渡口径说明：`runner.rs::run_replay` 模块文档注明“存量夹具仍是旧契约录制……任务降级率不可按面值解读”（步骤 7 重录时随夹具一并更新）；`metrics.rs` 的 `TaskVerdict::degraded` 文档与本目录 `metrics.rs_comment.md` 亦有同口径留痕。

## 步骤 5（契约校验与重试方案 §12 步骤 5）—— L4 真机步骤未随本 PR 登记进 bdd.md（2026-10-12）

- 问题（评审 LOW）：§11 L4 行（vision + `response_format` 流式真机验证、空 content 自动修复、降级卡与重试的人工步骤）未登记进“人工测试/发版人工步骤”节，PR 仅在正文列为遗留验证点；评审建议按 §12 随步骤 7 的 bdd.md 义务一并落清单，或随本 PR 补记。
- 豁免理由：选评审建议的**主选项（随步骤 7 落清单）**——方案 §12 步骤 7 明列“bdd.md”为其交付物，且这些 L4 步骤的执行载体（异常夹具“空回复/散文/缺 note/kind 越清单”、live 轨的违约与修复轮次统计）正是步骤 7 的条目：步骤 5 阶段补记只能写出没有可执行载体的步骤（空 content 自动修复的验证要经 live 轨回放异常夹具），步骤 7 会被迫二次改写同一批条目。与步骤 3 的豁免先例同构（eval 夹具旧契约留待步骤 7，见上文 2026-10-11 条目）。遗留验证点已在步骤 5 提交正文与本目录 engine_comment.md 中留痕，不靠记忆。

## 步骤 7（契约校验与重试方案 §12 步骤 7）—— §11 L4 行三条目的落清单处置（2026-10-12）

- 问题（第 1 轮评审 HIGH）：步骤 5 豁免记录（2026-10-12）指派“随步骤 7 落清单”，但步骤 7 的 bdd.md 只改了单元测试节，人工测试/发版人工步骤均无 §11 L4 行（vision + `response_format` 流式真机验证、空 content 自动修复、降级卡与重试）的条目，豁免决策落空。
- 处置（三条逐项，不留整条豁免）：
  1. **vision + `response_format` 流式真机验证**——已实现为真测试：`gloss-core/src/engine/llm.rs` 的 `live_tests::live_llm_streams_an_image_contract`（`#[ignore]`，需 GLOSS_LIVE_* 与支持图像的模型），bdd.md“人工测试”节登记 `live_llm_streams_an_image_contract`；端点不支持图像/JSON 模式时以 UnsupportedModality / JsonModeUnsupported 诊断当场失败（§2 分工表两条识别路径即其失败面）。
  2. **空 content 自动修复**——按步骤 5 豁免记录写明的载体（“经 live 轨回放异常夹具”）登记进“发版人工步骤”：`empty_content_repair_via_fixture_replay`（`just eval-replay` 核对两轨修复轮次行的 empty ×1 + L1 零增量完成测试）。真端点无法按需产出空 content（不设 max_tokens 的任务请求由服务端决定收尾形态），不存在可前置检查的真机触发条件；其确定性验证载体在步骤 7 已全部自动化（异常夹具 + 重放统计 + L1 消费路径测试），人工条目按清单回放这些载体。
  3. **降级卡与重试**——登记进“发版人工步骤”：`degraded_card_and_retry_manual_check`，用确定性真机触发：配置层 `validate_base_url` 强制 HTTPS（`BaseUrlError::NotHttps`，密钥明文红线，不为测试削弱——本地 http 桩不可用），改用非 LLM 的 HTTPS POST 回声端点（httpbin.org/post，实核 POST 200；正文无 `data:` 行 → SSE 零增量 → `finish_without_error` 发 `empty completion` → EmptyReply 修复三轮 → 耗尽落降级卡），照抄步骤覆盖“重新注解”×3、降级卡形态（无正文残留、有重试）与重试不投毒缓存。
- 为什么条目 2/3 落“发版人工步骤”而非“人工测试”：人工测试条目按门禁必须有同名 ignored 测试源，而这两项没有可断言的真机行为——条目 2 的载体是离线回放（已自动化，重复建 ignored 测试只会二次锁定同一行为），条目 3 的触发依赖外部静态端点（入 CI 会违反“真实网络不进测试”的 L1 原则并在第三方抖动时假红）；“发版人工步骤”节章程即“无自动化测试源码的真机步骤”，且已有功能人工检查先例（`image_commentary_sources_toggle_and_limits_manual_checks`）。

## 步骤 7（第 2 轮评审 LOW）—— 分支携带步骤 6 重复提交的处置：选 rebase（2026-10-12）

- 问题（评审 LOW）：分支 feat/eval-contract-metrics 仍携带步骤 6 提交（9489b8d、8cf654b，内容与已 squash 合入 main 的 #168 逐字一致——已实核 `git diff 8cf654b origin/main` 整树为空），`gh pr diff` 文件清单含 AGENTS.md 与 prompt.rs 两个非本步骤文件，`git merge-tree --write-tree origin/main HEAD` exit=1 对 crates/gloss-eval/src/lib.rs 与 docs/tests/bdd.md 报冲突。
- 处置：选评审建议的**主选项（rebase）**——`git rebase --onto origin/main 8cf654b` 把步骤 7 的 4 个提交重放到 origin/main（262a79e），步骤 6 重复提交整体消解。核验：rebase 前后整树 diff 为 0 行（内容中性，门禁结果不变）；`git merge-tree --write-tree origin/main HEAD` exit=0、0 冲突；PR diff 文件清单不再含 AGENTS.md 与 prompt.rs。旧哈希 c17b34f/aaf9764/75b0c62/da85285 重写为 429a456/e380b5d/2239446/02b924a——分支历史已重写，推送需 force-with-lease（推送归状态执行器）。
- 同轮另两条 LOW（测试代码注释、总览人工测试命令不覆盖 live_llm）成本低的已顺手修复：注释删除并移入本目录 runner.rs_tests_comment.md / report.rs_tests_comment.md；bdd.md 总览命令补注 live_llm 两条另跑 gloss-core。
