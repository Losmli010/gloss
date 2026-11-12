# eval main.rs（gloss-eval CLI）内联测试辅助契约

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），改名与合并用 `git mv` 保留历史；文件内不留指向本文件的指针。

- 2026-11-11 架构重构 E2：`default_eval_dir_has_no_target_path_segment` 钉住缺省目录常量不含 `target` 路径段——受控工件（人工标注与回归基线）默认落 `eval-state/`，不随 `cargo clean` 与性能流水线的清理步骤消失；断言只读常量、不读环境（`GLOSS_EVAL_DIR` 是用户机器状态，环境读取进断言会引入负载外的偶发红）。
