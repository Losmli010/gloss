# flamegraph.py 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），文件内不留指向本文件的指针。

## 定位与链路

- 2026-10-06（P4-1，替代 profile_cpu.py）折叠与渲染交给 inferno
  （inferno-collapse-dtrace → inferno-flamegraph），本脚本只做采集编排、
  栈顶自耗汇总与失败指引——不再自研 SVG 渲染器。替代缘由见
  justfile_comment.md 的 flame / flame-startup 节。
- 2026-10-06 dtrace 程序形状：`profile-<rate> /pid == N && arg1/` 采用户态栈
  （arg1 过滤内核态命中）+ `tick-<d>s { exit(0); }` 定时收口，与
  cargo-flamegraph 同款配方；`-x ustackframes=100` 对齐其默认栈深。
- 2026-10-06 attach 模式（本脚本）与 launch 模式（just flame-startup，
  cargo-flamegraph）分立：cargo-flamegraph 不支持 attach 到既有 pid，
  attach 是“实例已在跑、交互后采样”的唯一路径；launch 模式采启动段
  则是 attach 追不上的窗口。

## top_of_stack

- 2026-10-06 折叠格式 `根帧;…;叶子帧 N` 的叶子帧即栈顶：自耗 = 各行叶子
  计数之和，等价 sample 报告的“Sort by top of stack”跨线程聚合口径。
- 2026-10-06 帧名含空格时 rpartition(" ") 取计数、rsplit(";", 1) 取叶子
  会有歧义（解 mangling 后的泛型名理论上可含空格），Top 10 是展示性汇总，
  接受该近似，不做完整折叠语法解析。

## demangle_inplace

- 2026-10-06 rustfilt 可选：工具链 1.96 默认 v0 mangling，dtrace 原样输出
  `_RN…`，inferno 不解 Rust；装了 rustfilt 就地对折叠文件解 mangling，
  没装则保留 mangled 原样并提示——不把 rustfilt 做成硬依赖（渲染链路
  没有它也能走通）。

## 未符号化检测

- 2026-10-06 dtrace 无符号帧是“模块`0x偏移`”形（有符号帧是
  “模块`符号+0x偏移`”），与 sample 的 `???` 形不同，检测正则换成
  `` `0x<hex>$ ``；>60% 提示改用 profiling 实例，与旧脚本同口径：
  提示进输出不报错，二进制级归因仍有价值。

## sudo 失败分支

- 2026-10-06 stderr 含 "password is required" / "requires root" 时给
  “在终端里运行”指引而非裸报错：无人值守环境（CI、代理会话）跑
  `just flame` 必然走到这里，指引要能当场执行。

## 文件头迁出的用法与链路（2026-10-24，注释纪律）

- 用法：`scripts/perf/flamegraph.py`（采 gloss 进程 10 秒）；`--pid 1234` 指定
  进程；`--duration 30` 采样秒数；`--rate` 采样频率（Hz）；`--out /tmp/flame.svg`
  输出路径。
- 链路实名：sudo dtrace profile 探针 → inferno-collapse-dtrace 折叠（装了
  rustfilt 则就地解 Rust mangling）→ inferno-flamegraph 渲染 SVG。
- 从启动采到退出的 launch 模式走 `just flame-startup`（cargo-flamegraph）。
- 函数级归因先 `just profile-run` 以 profiling 配置启动带符号实例再采
  （release 二进制 strip=true 无符号表，无符号帧显示为“二进制名`0x偏移”）。
