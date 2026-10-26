# gen_manifest_test.py 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），文件内不留指向本文件的指针。

## 文件头迁出的用法（2026-10-24，注释纪律）

- 运行：`python3 -m pytest scripts/release/gen_manifest_test.py`。
- 被测函数：`gen_manifest.run(tag, artdir, root)`，成功返回 manifest dict，失败抛 SystemExit。
- 夹具：在 pytest 的 `tmp_path` 里放已知内容的产物文件；manifest 的 `size` 与 `sha256` 实测自产物文件。
