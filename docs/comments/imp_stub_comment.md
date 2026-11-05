# imp_stub 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），改名与合并用 git mv 保留历史；文件内不留指向本文件的指针。

## 占位与死码放行（2026-11-05，Windows 移植 P0；2026-11-06，P2② 起范围收敛）

- 非 macOS、非 Windows 目标下发现器恒无发现（`load_cjk`/`load_family` 返回空），
  命名字体族由父模块既有降级兜底到内置字形；系统（Windows）的真实发现器
  （DirectWrite 枚举加固定候选探测，§2）已于 P2 ② 落地为 imp_win.rs，桩退为
  其余目标（Linux 占位等）服务。
- 模块级 `#![allow(dead_code)]` 的缘由：家族字体（FamilyFont）与字节背衬（FontBacking）在此同名镜像、只被登记流程以类型引用，桩内无构造方——镜像是为了父模块流程零改动编译；`tests/stubs` 目录同款口径。桩存续期间放行保留。
- 与 macOS 实现的分工差异：字节背衬类型从父模块迁入各平台发现器（macOS 侧随 `imp.rs` 持有）——背衬形态（mmap/拷贝）是实现细节，父模块只依赖 `as_slice` 读面。
