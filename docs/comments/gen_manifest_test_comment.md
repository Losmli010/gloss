# gen_manifest_test.py 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），文件内不留指向本文件的指针。

## 文件头迁出的用法（2026-10-24，注释纪律）

- 运行：`python3 -m pytest scripts/release/gen_manifest_test.py`。
- 被测函数：`gen_manifest.run(tag, artdir, root)`，成功返回 manifest dict，失败抛 SystemExit。
- 夹具：在 pytest 的 `tmp_path` 里放已知内容的产物文件；manifest 的 `size` 与 `sha256` 实测自产物文件。

## 发布矩阵平台条件化（2026-11-07，与 gen_manifest 同批）

- 夹具补 Windows 压缩包产物（无 Windows 镜像——矩阵本就不要求）。
- `test_manifest_generated` 的 stable 键集合改为与被测脚本的发布矩阵对账
  （`set(gen_manifest.TARGETS)`），不复制三元组清单。
- 新增 `test_windows_entry_carries_no_installer_field`：Windows 条目恰为
  `url/size/sha256`，无 `dmg_url`（条目字段平台条件化的锁定）。
- 新增 `test_missing_windows_zip_rejected`：Windows 压缩包缺文件同样拒绝。
