# workspace.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关符号时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），改名与合并用 git mv 保留历史；文件内不留指向本文件的指针。

## request_redraw（2026-10-16，内存优化方案）

- 为什么按浮层可见性门控重绘：隐藏窗口在 macOS 仍取得到 drawable，事件驱动的零散重绘在隐藏期是整帧空转（tessellate + surface 呈现）。备选「门控画帧（draw）」不取——预热首帧（resumed 里直调 draw）必须在隐藏态画，门控画帧会把它一并拦掉；门控 request_redraw 不影响直调路径。安全性依据：全部显示路径（reveal 的 show_overlay、失败即弹）都先置窗口可见再请求重绘，门控不吞该画的帧。

## ensure_settings_frame / drop_settings_frame（2026-10-16，内存优化方案）

- 为什么设置窗渲染帧惰性建立、随关闭释放：设置窗低频且从未打开也常驻一套 surface（高分屏约 14MB drawable）+ Renderer + 独立 egui 上下文与字体图集。窗口本体仍启动即建（NSWindow 便宜，且 ActiveEventLoop 只在 resumed 可用）；贵的 GPU 侧首次绘制前建、关闭即 drop。备选「整窗惰性」不取——WindowManager 需要持有 settings 窗口才能匹配 window_event，Option 化会传染全部事件路由。

## 字体时序

- 字体时序闭环：新上下文经 context::install 按装载进度自动取内置与完整定义；若帧建于字体装载完成前，FontsReady 的强制重施加（apply_theme_forced）覆盖现存帧，两条路径都收敛到完整字体表。

## Workspace 字段（呈现资源所有者）

- 窗口与帧建好之前各字段为 `None`（`resumed` 时 `init`）。
- `windows`：窗口管理器：浮层 + 设置窗的生存期与显隐。
- `overlay_frame`：浮层的渲染帧；随窗口栈在 `resumed` 时建好，隐藏期保留。
- `settings_frame`：设置窗口的渲染帧；惰性建立（首次绘制前），关闭即释放（见 `ensure_settings_frame`）。
- `gpu`：GPU 设备上下文：全进程一份，设置帧惰性建立时取用。
- `overlay_repaint`：浮层 egui 要求的下一帧时间点；`None` 表示等到有事件再画。
- `settings_repaint`：设置窗口 egui 要求的下一帧时间点，与浮层各自独立。
- `applied_theme`：已施加到各 egui 上下文的主题；`None` 表示还没施加过（窗口未起时会有这个状态）。

## Workspace::init

- 建窗口栈与浮层的首帧渲染状态（`resumed` 里调用；resumed 可能连续投递，调用方先查 `is_ready`）。系统字体由后台线程延迟装载（秒级，不挡首帧），装好经 `fonts_ready` 通知主线程换表重绘。

## Workspace::is_ready

- 窗口与帧是否已建立（`resumed` 是否已跑过）。

## Workspace::request_redraw 的可见性门控（行内注释迁出）

- 隐藏的浮层不请求重绘：隐藏窗口在 macOS 上仍取得到 drawable，事件驱动的零散重绘在隐藏期是整帧空转（tessellate + surface 呈现全跑一遍）。所有显示路径（显形决策、失败即弹）都在把窗口置回可见之后才请求重绘，这里的门控不吞掉任何一帧该画的画面。

## Workspace::apply_theme / apply_theme_forced

- 把主题偏好施加到两个 egui 上下文：偏好变化时才写，两个上下文各写一次。`force` 绕过变化判定（字体补装完成时必须重施加——字体表要换完整定义，即使主题没变）。
- 帧尚未建立（窗口未起）时这里是空集：只记状态，不 panic。
