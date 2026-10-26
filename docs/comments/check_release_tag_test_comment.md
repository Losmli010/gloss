# check_release_tag_test.py 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），文件内不留指向本文件的指针。

## 文件头迁出的用法（2026-10-24，注释纪律）

- 运行：`python3 -m pytest scripts/release/check_release_tag_test.py`。
- 被测函数：`check_release_tag.run(tag, root)`，返回 `(tag 版本, cargo 版本, 错误串)` 三元组。
- 夹具：在 pytest 的 `tmp_path` 里放定制的 `Cargo.toml`（版本单点放 `[workspace.package]` 段）。
