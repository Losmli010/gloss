# install.rs 决策记录

> 自 `crates/gloss-app/src/update/install.rs` 迁出的代码注释，记录各符号的设计缘由与契约。文件头 `//!` 注释保留在源码中。

## InstallError

替换或重启失败的形态：错误卡按变体给针对性指引。

- `NotABundle`：当前进程不在 `.app` 内（开发期裸二进制）：无 bundle 可替换。
- `Unwritable { dir }`：bundle 所在卷只读或目录无写权限：替换无从下手；`dir` 是 bundle 所在目录（指引里点名它）。
- `Unzip`：解压失败（ditto 非零退出或未产出内容）。
- `InvalidStructure`：解压产物里找不到结构完整的 `.app`（`Contents/MacOS` 缺失）。
- `Replace`：改名/移入失败；已尽力回滚，错误串带两步的现场。
- `Relaunch`：重启接力失败（`open` 不可用或报错）：新 bundle 已就位，指引手动重启。

## install

替换当前 bundle 并经 `open -n` 拉起新 bundle：成功返回后调用方进程退出（模块任务负责）。

## current_bundle

当前进程所属的 bundle：从可执行路径向上找第一个 `.app` 目录。开发期裸二进制不在任何 `.app` 内，返回 `None`。

## replace

替换的纯机械部分（重启除外），可全量驱动：`bundle` 必须位于 `dir` 下且 `dir` 可写。

行内决策：中途失败的回滚只护「改名对」——旧 bundle 改名成功而新 bundle 移入失败时，把旧 bundle 改回原位，现场恢复到替换前。

## clear_stale_old_bundle

上一次替换中断留下的旧 bundle：本轮成功替换会覆盖它，先清场。

## prepare_unpack_dir

备好本轮的解压目录：先清掉上次中断遗留的同名目录，再新建。

## extract_app

解压并找到新 bundle：解压失败或包内没有结构完整的 `.app` 都不碰现有安装，解压现场清理后带对应错误返回。

## swap_bundles

换名对：旧 bundle 让位为 `.app.old`，新 `.app` 移入原位；移入失败时尽力把旧 bundle 改回原位（回滚也失败则旧 bundle 留在 `.app.old`）。

## finish_replace

旧 bundle 清理与解压现场清理都属收尾：失败不致命（旧 bundle 留在 `.app.old` 不影响新 bundle 运行，下次替换先清场），记日志即可。

## remove_unpack_dir

解压现场的清理：失败只落 debug——残留目录带进程号命名，下轮替换会先清场。

## relaunch

经 detached 辅助进程拉起新 bundle：`open -n` 由 LaunchServices 接管，本进程随后退出。接力失败带指引返回（新 bundle 已就位，手动重启即可）。

## ditto_extract

`ditto -x -k`：macOS 自带的 zip 解压（打包链路 bundle_dmg.py 同源工具）。

## extracted_app

解压产物里结构完整的 `.app`：zip 由打包链路产出（单 bundle），取第一个带 `Contents/MacOS` 的；没有即结构不符。

## dir_writable

目录可写性探测：真实建删一个探针文件（只读卷上 create 直接失败）。
