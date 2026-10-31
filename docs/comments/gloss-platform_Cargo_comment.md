# gloss-platform/Cargo.toml 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关依赖时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），文件内不留指向本文件的指针。

## arboard

- 2026-09-18 默认集含 image-data（拖进整条 image 依赖树）与各平台的后端变体，文本能力不需要。注意：egui-winit 的 clipboard 特性仍会经 image-data 把 image 子树拉进构建图，此处关闭只约束本 crate 的直接声明。

## core-foundation-sys

- 2026-09-18 唯一特性 link 是绑定 CF 符号所必需的，显式声明以维持“关默认集、按需开特性”的统一写法。

## crossbeam-channel

- 2026-09-18 选型与 gloss-app::channel 一致用 crossbeam。std 是默认集的全部内容，显式写出以维持统一写法。

## directories

- 2026-09-18 配置目录走 directories、日志目录例外，出处 06 §8。无特性可关，写法与全局“关默认集”策略保持一致。

## objc2

- 2026-09-18 三个 objc2 系绑定 0.6.4/0.3.2 系列本就在依赖树里（eframe/arboard/global-hotkey/webbrowser 带进，objc2 与 objc2-foundation 另有 wgpu-hal；winit 自身钉在 0.5.x 旧系列，双版本并存是既有事实），提为直接依赖不新增子树，版本对齐已有条目以免重复编译。

## objc2-app-kit

- 2026-09-18 NSResponder 必须与 NSApplication 同开：NSApplication 的 objc2 定义整体被该特性门控（sharedApplication 也在其门控的 impl 块里），缺了编译不过（编译器当场拦截）。

## objc2-foundation

- 2026-09-22 系统语言读取（src/locale.rs 的 `NSLocale::preferredLanguages`）要给原有关键字集加 `NSLocale`/`NSArray`/`NSString`：`preferredLanguages` 本身被 `all(feature = "NSArray", feature = "NSString")` 门控，缺任一编译不过。只加这三个，不整体开默认集。
- 2026-10-12 加 `NSURL`：`NSRunningApplication.bundleURL` 的返回类型被
  该特性门控，selection/activation.rs 按前台应用包路径查内核框架形态
  （Chromium 家族判定）。只加这一个，不整体开默认集。

## reqwest

- 2026-09-18 默认集会带 http2（多一条 h2 子树，SSE 用 HTTP/1.1 足够）、charset、native-tls 与系统代理读取，故全关。
- 2026-09-18 TLS 选 rustls（自带 ring provider），不引 aws-lc-rs 的 cmake/NASM 构建链。
- 2026-09-18 根证书用系统存储而非 webpki-roots：后者是 CDLA-Permissive-2.0，不在 deny.toml 白名单内。

## serde_json

- 2026-09-18 core 已带同一版本，这里作为直接依赖列出（本 crate 直接使用）；core 用到的特性是 std。

## toml

- 2026-09-18 serde 桥接为必选依赖，0.8 后期版本不再暴露 serde 特性，故无需特性开关。

## rdev（2026-10-13 移除）

- 2026-09-18 引入时只用于注入（模拟复制与真机测试事件），监听不走它；
  默认集只多出可选的 serde 支持，注入用不到。
- 2026-10-13 整体移除：rdev 0.5.3 → cocoa 0.22 → block 0.1.6，而 block
  0.1.6 触发 rustc future-incompat（“static of uninhabited type”，
  rust-lang/rust#74840），未来工具链升位即全仓编译失败；block 上游多年
  未修，rdev 也无新版本脱离 cocoa 0.22。注入改为手写 `ffi/event.rs`
  （CoreGraphics 合成事件），逐字复刻 rdev `simulate` 的行为
  （HIDSystemState 事件源 + kCGHIDEventTap + 同 HID 键码），依赖树同时
  拔掉 rdev/cocoa/block 三层。手势监听本就不走 rdev（eventtap 手写 FFI），
  此次只是把注入也收编；细节见 event_comment.md 与
  selection_clipboard_comment.md。

## 特性门控注释迁入（2026-10-24）

- Dock 图标装载：非 bundle 运行时经 NSApplication.setApplicationIconImage 装上品牌图标。
- 自身判定：NSRunningApplication 的 processIdentifier 被 libc 特性门控（is_self 判定）。
- 引用式删除：SecKeychainItemDelete 与 errSecItemNotFound 常量（security-framework 未封装删引用一条路）。
