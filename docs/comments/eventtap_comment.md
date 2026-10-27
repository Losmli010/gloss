# eventtap.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。

## 类型与回调签名

- `CGEventRef`：CoreGraphics 的事件引用：回调期间由系统借用，我们只读它的位置。
- `CGEventTapProxy`：tap 回调的 proxy 参数：只观察、不改写事件流的实现用不到。
- `CGPoint`：指针位置的返回结构（CGGeometry.h 的 CGPoint）。
- `TapCallback`：tap 回调的 C 签名。

## 常量

- `K_CG_HID_EVENT_TAP`：CGEventTapLocation 的 kCGHIDEventTap：事件链最早的一层。
- `K_CG_HEAD_INSERT_EVENT_TAP`：CGEventTapPlacement 的 kCGHeadInsertEventTap。
- `K_CG_EVENT_TAP_OPTION_LISTEN_ONLY`：CGEventTapOptions 的 kCGEventTapOptionListenOnly：只观察，不改写事件流。

## extern 声明契约

- `CGEventTapCreate`：建立事件 tap；未获辅助功能授权（或系统 tap 名额耗尽）时返回 NULL。
- `CGEventTapEnable`：启用/停用 tap。
- `CGEventGetLocation`：事件发生时的指针位置（全局坐标，原点在左上）。
- `CGEventGetTimestamp`：事件自身的时间戳（CGEventTimestamp = uint64_t，约自系统启动起的纳秒数；CGEvent.h 的原话）。
