# size_report.py 决策记录

- 2026-10-07 符号大小取法：macOS `nm -S` 对 Mach-O 恒报 0（官方已知行为），
  改用 `otool -l` 节地址区间 + `nm -n` 符号地址、同节相邻地址差分——大小含
  对齐padding，属该口径的固有近似；跨节归属按地址落区间判定。
- 2026-10-07 符号解码：工具链默认 v0 mangling（`_RN...`，实测 1.96），仅做
  旧式 `_ZN` 解码会全军覆没。v0 解码走通用子集：标识符 `<长度><名字>` 连
  路径，`Cs<混淆>_<len><name>` 与单字符标记（v/t/N/M/s…）透明跳过；泛型
  实例化（I…E）与回引用（B…_）不解、退回原始名（诚实展示，不解不负责任
  何语义）。crate 聚合对解不动的名字用 Cs 标记正则抠 crate 名，仍无则归
  “<未解码>”——解码失败曾经 `or name` 回退污染聚合（顶层 crate 列出整条
  原始名），decoded 字段必须保 None 语义。
- 2026-10-07 节级用 `size -m`：数值是虚拟地址空间占用，__PAGEZERO（4GB）等
  非文件映射段会远大于文件；文件大小以 stat 为准，两口径都在输出里。
- 2026-10-07 记录性工具不设判罚：无退出码判定，JSON 落 --json 指定路径
  （缺省只打印）；趋势对照按 AGENTS.md 口径只认同机同环境历史。
- 2026-10-07（PR #122 评审修正）v0 下划线转义：下划线开头的标识符在长度
  前缀后多一个转义 `_`（`Cs.._7___rustc` → crate `__rustc`），漏了会产出
  幻影 crate `___rust`；V0_CRATE_RE 兜底同样剥一个前导 `_` 再按长度截。
  `Ms4_` 类实现路径消歧符实为 `s<数字>_` 原子语法，按 `s\d*_` 整体消费后
  这类符号也能正常解码（原先靠 I/B 回退兜住）。“无符号表”提示改按
  nm 可见符号总数为 0 触发——macOS nm 对 strip 产物以 exit 0 收场，
  原 SystemExit 分支不可达。

## 文件头迁出的用法与工具链（2026-10-24，注释纪律）

- 用法：`scripts/perf/size_report.py`（release 产物节级）；
  `scripts/perf/size_report.py --bin target/profiling/gloss --top 40`；
  `scripts/perf/size_report.py --json target/size/report.json`。
- 工具链实名：节级 `size -m` 读 Mach-O 段/节（虚拟地址空间口径，
  `__PAGEZERO` 等非文件映射段会偏大，文件大小以 stat 为准）；符号级
  `otool -l` 取节地址区间、`nm -n` 取符号地址。
- 符号级归因需要未 strip 的二进制：`cargo build --profile profiling` 的
  `target/profiling/gloss`，或 dev 产物（release 产物 strip=true 无符号表）。
