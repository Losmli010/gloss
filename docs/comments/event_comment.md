# event.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），文件内不留指向本文件的指针。

## 整体方案（2026-10-13）

- **为什么手写注入 FFI**：替换 rdev——其 macOS 实现拖进 cocoa 0.22 →
  block 0.1.6（future-incompat 硬错误炸弹，见 gloss-platform_Cargo_comment.md
  的 rdev 节）。仓库的监听路径（eventtap.rs）本就是手写 CoreGraphics FFI，
  注入是同一框架同一族 API，收编后 FFI 声明仍然全部集中在 ffi/ 一处。
- **为什么逐字复刻 rdev 的参数**：模拟复制与手势注入是已验证多年行为
  的路径（读取依据 rdev 0.5.3 `src/macos/simulate.rs`）——事件源固定
  `kCGEventSourceStateHIDSystemState`、注入点固定 `kCGHIDEventTap`、
  键码映射同 HID 虚拟键码表（Cmd=55、C=8）、按钮事件取“当前指针位置”
  构造。换源或换注入点都可能改变系统对合成事件的合并行为（尤其修饰键
  状态），不值得为一个依赖替换冒行为漂移的风险。唯一结构差异：事件源在 `inject_copy_key` 内建一次、四步序列复用（rdev 每次 simulate 新建）——HIDSystemState 是共享系统状态，source 实例只是句柄，行为等价且省三次 `CGEventSourceCreate`。
- **为什么修饰键不显式 CGEventSetFlags**：与 rdev 同款——先注入 Cmd
  按下（键码 55）再注入 C，系统按事件流维护修饰键状态，后续事件自动
  带上 Cmd；显式设 flags 反而是 rdev 没有的行为差异。
- **为什么 safe 包装收在 ffi/event.rs**：`post_keyboard_event`/
  `post_mouse_event` 把“构造 + 判空 + 守卫释放 + 注入”收成一步，调用
  方不接触 unsafe 也不持有事件引用生命周期；与 ax.rs 的 safe 包装同款
  分工。返回 bool 只反映“事件构造成功”（分配失败），注入本身无返回
  状态——未授权时事件被系统静默丢弃，成败语义由剪贴板写入确认窗口
  承担（与 rdev 时代一致，见 selection_clipboard_comment.md）。
- **test 门控面**：生产路径只注入键盘（模拟复制）；鼠标构造/位置查询
  仅 L4 真机测试用，按 ffi/cf.rs 的 cf_data 先例以 #[cfg(test)] 门控，
  lib 构建零死代码。

## 测试与验证边界

- 键盘注入路径无 L1 单测（纯 FFI，无纯逻辑可拆）；行为验证靠 L4：真机
  划词走剪贴板兜底（注入 → 确认写入 → 读取 → 恢复），以及
  events/mouse.rs 的 injected_drag_yields_selection_gesture（鼠标注入
  驱动真实 tap）。替换 rdev 后两条 L4 均需真机过一遍（划词任一应用 +
  跑一次注入手势测试）。

## post_keyboard_event 修饰位语义（2026-10-15，评审补）

- **修饰键释放事件不带自身修饰位**：flagsChanged/keyUp 的修饰位表达
  “事件之后”的状态——Cmd 按下带自身位、释放不带，系统级消费者以
  “自身位是否还在”区分修饰键 down/up。注入序列的 Cmd 释放若带位，
  等于向下游宣告“Command 仍按着”，自跟踪修饰键状态的应用会把用户
  随后的真实按键误判成 chord（c→⌘C、q→⌘Q）。C 的按下/释放带位不受
  影响（按住 Cmd 按 C 的真实硬件事件本就带位）。

## 符号契约（自代码注释迁入）

- `VK_ANSI_C`：HID 虚拟键码（HIToolbox 的 Events.h）：C 键。
- `VK_COMMAND`：HID 虚拟键码：Command 修饰键。
- `K_CG_EVENT_FLAG_MASK_COMMAND`：CGEventFlags 的 kCGEventFlagMaskCommand：键事件自带 Command 修饰位。
- 测试门控常量：`CG_EVENT_LEFT_MOUSE_DOWN`/`CG_EVENT_LEFT_MOUSE_UP`/`CG_EVENT_MOUSE_MOVED` 为 CGEventType 常量（真机测试注入用）；`CG_MOUSE_BUTTON_LEFT` 为 CGMouseButton 的 kCGMouseButtonLeft；`CG_EVENT_SOURCE_HID_SYSTEM_STATE` 为 CGEventSourceStateID 的 kCGEventSourceStateHIDSystemState。
- `CGEventSourceRef`：HID 系统状态的事件源引用（+1 引用，由守卫释放）。
- `CGEventSourceCreate`：建立事件源：成功返回 +1 引用，失败返回 NULL。
- `CGEventCreateKeyboardEvent`：构造键盘事件：`key_down` 决定按下/释放；成功返回 +1 引用，失败 NULL。
- `CGEventSetFlags`：改写事件的修饰位字段；应用按事件自带的修饰位识别快捷键。
- `CGEventPost`：把合成事件注入 `tap` 层，无返回状态。
- `CGEventCreateMouseEvent`（测试门控）：构造鼠标事件：`kind` 为 CGEventType 常量；成功返回 +1 引用，失败 NULL。
- `CGEventCreate`（测试门控）：构造空事件：仅用于查询当前位置；成功返回 +1 引用，失败 NULL。
- 真机测试注入专用的鼠标事件构造：生产路径只注入键盘（模拟复制）。
- `new_hid_source`：建立 HID 系统状态的事件源（合成事件的来源标注）；失败返回 `None`。
- `post_keyboard_event`：构造一个键盘事件（`key_down` 区分按下/释放，`flags` 写入事件自带的修饰位字段）并注入 HID tap：事件构造失败（分配）返回 `false`，注入本身无返回状态。
- `post_mouse_event`：构造一个鼠标事件（`kind` 为 CGEventType 常量）并注入 HID tap：事件构造失败（分配）返回 `false`，注入本身无返回状态。
- `current_position`：当前指针位置（全局坐标，原点在左上）；临时事件构造失败返回 `None`。
