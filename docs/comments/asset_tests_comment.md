# asset.rs（gloss-eval）内联测试辅助契约

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），改名与合并用 `git mv` 保留历史；文件内不留指向本文件的指针。

- 2026-11-01 `missing_files_load_as_empty_maps` 钉首轮语义：标注与基线文件缺失按空表处理，不报错（计划 §6.4）。
- 2026-11-01 `upserts_merge_by_id_and_keep_lf_and_note_roundtrip` 钉合并写不变量：同标识覆盖不叠行、行以换行符结尾、按标识排序落盘（文件确定性）、备注随往返保留；基线重复固化以新固化时间胜出。
- 2026-11-01 `bad_rows_and_unknown_verdicts_are_rejected_with_line_numbers` 钉受控工件语义：坏行带行号拒绝、结论域外词面（`verdict` 非 `pass`/`fail`）拒绝、基线行缺逐轮数组拒绝——与历史行的坏行容错分账（资产是判定依据，静默跳过会污染回归）。
- 2026-11-01 `outputs_overwrite_on_write_and_fail_clear_on_missing_read` 钉单次输出两侧：覆盖写后读回逐字段一致、覆盖写空表后读作零行、文件缺失报清晰错误并指明补救（先跑迭代）——固化无输入不可静默成空基线。
- 临时目录助手按纳秒加进程号命名，测试相互独立、无顺序依赖。
