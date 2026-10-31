# benches/clone.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），文件内不留指向本文件的指针。

## 测量机制（2026-10-05）

- 计数走自定义 `GlobalAlloc` + 两个 criterion `Measurement`（分配次数/字节）而不用
  耗时：分配是 clone 的直接开销通道，对固定输入近乎确定，可作精确判罚；无第三方
  依赖。计数器挂 thread-local，天然隔离 criterion 与其他线程的分配；只计 alloc
  （毛计数），不计 dealloc——基线关心的是分配量而不是净增量。realloc 走
  GlobalAlloc 默认实现（alloc+copy+dealloc），按一次新分配计入。
- `ValueFormatter` 不做单位缩放：allocs/bytes 本身就是人读的粒度。
- 计数器用 `try_with` 而非 `with`：全局分配器在线程析构期仍可能被调，TLS 已毁时
  静默放弃该次计数（少计无害，panic 才有害）。

## 热点选取（2026-10-05）

- 五组热点对应 clone 分布的头部（见 scripts/baselines/clone-stats.json）：设置窗
  一帧（settings.rs，静态计数最高）、浮层一帧（popup.rs，逐帧可见路径）、状态机
  任务周期（machine.rs，每次划词一次）、事件去向判定（trigger_decision，每次手势
  一次）、文案表错误映射（i18n.rs，错误卡逐帧）。`machine/trigger` 预期零分配——
  基线同时是“此路径保持零分配”的契约锚。
- UI 帧的测量对象是**整帧**（含 egui 内部簿记），不是逐个 clone：clone 删减在这
  个总量里显形，且不必随实现重构改基准。帧驱动用 `run_ui`（0.36 的无头入口，
  直给根 `Ui`，无 wgpu/winit）。
- 预热推进时间、测量期时间冻结的原因：浮层淡入依赖 `input.time`，egui 的缓存
  淘汰按 `now - last_used` 判定——时间冻结后动画停在完成态、淘汰永不触发，逐帧
  分配恒定；预热帧数（40）覆盖动画时长与字体/纹理首次装入。
- `machine/lifecycle` 的输入构造**计入**测量：`iter_batched` 的 setup 分配同样落
  进测量窗口躲不掉，内联进 `b.iter` 让口径自明（基准文档注明）；其余热点输入全部
  循环外构造。输入构造里没有 `.clone()` 调用（全是 `to_owned()` 字面量），与被测
  热点的 clone 减量不混叠。
- `let-else` 处理 `begin_selection_probe` 的不可能分支（State 机刚 new 必返回
  Some）：返回即跳过该次迭代，不 panic（工作区 lints 在 benches 同样 deny panic
  家族）。

## 可见性连带（2026-10-05）

- 需要 `pub` 的最小面：`ui::i18n`（`Text`/`get`/`for_error`/`for_error_detail`）、
  `ui::settings::draw`、`ui::popup::draw`/`RenderState`/`PopupOutput`。取舍与修订
  缘由记在 i18n_comment.md / settings_comment.md / popup_comment.md。
