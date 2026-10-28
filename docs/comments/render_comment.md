# render.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。

## Frame（一帧渲染状态）

- 一帧渲染所需的全部状态；窗口建好之前为 `None`。
- `popup_state`：浮层的跨帧渲染状态（markdown 缓存与尺寸收敛状态）：Rc 让它经绘制闭包进入 `ui::popup::draw`。仅浮层路径使用。
- `adapter_name`：GPU 适配器名，供自检的性能数据记录运行环境。
- `reset_overlay_drag`：清除浮层的跨帧拖动状态：壳在浮层显隐边界调用，跨显示残留的拖动所有权由此失效（见 `ui::popup::RenderState::reset_drag_state`）。
- `reset_appear_animation`：清除浮层出现动画起点，让下一次显示从 0 淡入：生产显形入口在每次显示时调用，显隐自检的每轮显示也经它复现同一行为。

## build_window_stack（建窗口栈）

- 建窗口栈与浮层窗口的首帧渲染状态（生产 App 与自检 handler 共用）：浮层帧在前。设置窗口的渲染帧不在这里建——设置窗是低频入口，其 surface 与渲染器惰性创建、随关闭释放（见 `crate::present::workspace::ensure_settings_frame`）。两个窗口共享同一份 `GpuContext`（设备与队列各一份），返回它供建设置帧；egui 上下文各自独立，都经 `ui::context::new_context` 装好字体与主题（`theme` 是建立时的主题偏好）。

## build_frame

- 建单个窗口的 egui 渲染状态 + surface（浮层与设置窗口同构）。

## render_frame_with（一帧呈现管线）

- egui 出绘制数据 → wgpu 呈现，返回（egui 要求的下一帧时刻，绘制闭包的返回值）。浮层与设置窗口共用这套管线，只有绘制闭包不同。
- `run_ui` 的闭包是 FnMut 而绘制闭包是 FnOnce：装进 Option 交出所有权，绘制结果经这个槽带出来（闭包恰好执行一次）。

## OverlayFrameOutput（浮层一帧的上交产物）

- 头部动作、内容期望高与页头拖动位移。
- `action`：本帧被点击的浮层动作（失败卡与头部动作区）。
- `sizing`：浮层内容期望的窗口高（宽度恒为 `ui::popup::WIDTH`，壳按显示器钳制后应用）。
- `drag`：页头拖动热区的指针位移（窗口内相对逻辑点）；`Some`＝拖动进行中。

## render_frame（浮层特化）

- `render_frame_with` 的浮层特化，供 App 与自检 handler 用：`locale` 是界面语言（文案表选表依据，本层不做探测）；`attached` 是本会话经位附件图像（图像卡的经位，状态机经访问器随行）。返回（egui 要求的下一帧时刻，本帧的浮层上交产物）。
- 内层 Option 是「闭包有没有跑」的外壳，动作本身才是浮层的返回值。

## repaint_at

- egui 用 `Duration::MAX` 表示「不必重绘，等输入」；其余延迟换算成唤醒时刻。

## OverlayFrameOutput 的缩放通道

- 2026-10-28 帧输出随浮层新增 `resize` 通道（期望窗口尺寸，逻辑点）：浮层绘制产物的直通字段，壳侧装配点消费（见 app 与 windows 记录）。

## 取帧前的焦点校准（2026-10-29）

- `render_frame_with` 在 `take_egui_input` 前以 `window.has_focus()` 权威校准界面库输入的 `focused` 标志。背景：界面库（egui）把文本插入符的绘制门在「视口持有焦点」上，而焦点标志的既有来源是焦点事件——事件在系统（macOS）上会迟到或缺失（窗口库已知缺陷，界面库绑定层自带的补救也只修载荷不修投递），标志一旦滞留 `false`，聚焦输入框后插入符整段不渲染（诊断实例证实：`focused=false` 时插入符完全不画；强制 `true` 后显隐节拍正常）。逐帧轮询窗口实况把这一类时序缺陷整类消掉，事件投递路径保留原样（`WindowFocused` 事件仍进界面库事件流）。
- 插入符的显隐节拍本身（0.5 秒亮 / 0.5 秒灭与重绘诉求）由界面库内建，壳侧两窗的重绘期限链路照常驱动；修复后按窗口实测连拍验证逐帧交替。
