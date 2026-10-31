# gloss-app/Cargo.toml 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关依赖时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），文件内不留指向本文件的指针。

## crossbeam-channel

- 2026-09-18 std 特性即默认集全部内容，显式写出以维持“关默认集、按需开特性”的统一写法。

## 渲染栈整批（egui / egui-wgpu / egui-winit / winit / wgpu / pollster）

- 2026-09-18 egui 三件套必须同小版本；wgpu 版本由 egui-wgpu 决定，不要单独升级。
- 2026-09-18 全部 `default-features = false` + 显式列特性的原因：默认集往往带有用不到的后端（vulkan / gles / webgpu / wasm 等），既增加依赖与编译时间，也可能拖进有问题的包——实例：winit 的 wayland-csd-adwaita → ttf-parser（RUSTSEC-2026-0192，已停止维护、无可用修复版本）。新增依赖沿用此写法。

## egui-wgpu

- 2026-09-18 不用默认特性：默认里的 "wgpu/default" 会打开 wgpu 全部后端，另含 wasm 专用的 fragile-send-sync-non-atomic-wasm 与 wgpu/webgl。

## font-kit

- 2026-09-18 引入出处 M1-T5。0.14 的默认集即 source，照写 `default-features = false` 维持统一写法。

## futures

- 2026-09-18 std 是 FutureExt::catch_unwind 所需；core 已依赖 futures，不引入新子树。
- 2026-10-13（P2-1）release 上 `panic = "abort"` 后 std 特性仍不可收窄：
  `FutureExt::catch_unwind`（runtime/pipeline）在 futures-util 0.3.34 里位于
  `#[cfg(feature = "std")]` 之后（future/future/mod.rs 的 catch_unwind mod 与
  重导出），是 std 特性的唯一使用证据；`StreamExt::next`（update 的流式读取）
  无特性门、零特性可用——维持 `default-features = false, features = ["std"]`
  不动。屏障保留的缘由见 pipeline_comment.md。（同日评审修正：初版误把
  StreamExt 一并归入 std 门。）

## pollster

- 2026-09-18 macro 特性（block_on!/join!）用不到；无默认特性可关，照写保持“关默认集”写法统一。

## tokio

- 2026-09-18 不用默认特性：默认集会拉起整个子树（io/_net 等用不到），按需最小化。

## tokio-util

- 2026-09-18 sync 模块不在特性门后，关默认集即可用，避免 codec / io 等无关子特性。

## wgpu

- 2026-09-18 默认集含 gles / vulkan / webgpu 三个用不到的后端与 parking_lot，只留实际使用的 Metal。

## egui_kittest

- 2026-09-18 上游 wgpu 特性硬编码启用 wgpu 的 dx12/vulkan/gles（特性只加不减），测试构建因此含全部后端，无法从本仓库收窄。

## egui_commonmark

- 2026-09-20 T3 结果展示升级引入（版本 0.25.0）：选型理由——egui ^0.36.0 与本仓 0.36.2 对齐、MSRV 1.95 ≤ 工具链 1.96.1、纯 Rust（pulldown-cmark 后端）、MIT OR Apache-2.0 在 deny 白名单。特性只开 pulldown_cmark：load-images/fetch/svg 是联网取图路径，LLM 结果卡不需要；egui_extras 是上游非可选传递依赖，无特性开关下编译面很小（ahash/enum-map/itertools 等，无 image/http）。

## image

- 2026-09-29 页头改版引入（版本 0.25，只开 png 特性）：把内嵌的 gloss-dock-icon.png 解码成 egui 纹理。选它而非手搓 PNG 解码或走 egui_extras 装载器——该 crate 已随 arboard 在生产依赖树内，加特性只多编译 png 编解码器本身，不新增传递面；egui_extras 路线要逐 egui 上下文安装 loader，为一张图多背一套机制。

## resvg

- 2026-10-19 引入（0.45.1，default-features = false）：页头应用图标改矢量栅格化（SVG 源 → 物理像素级 ColorImage），版本对齐 egui_extras svg 特性所钉的 0.45.1。默认特性全关——text/system-fonts 等字体机器是资产无 <text> 时纯多余的传递面；直用 resvg 而非 egui_extras 的 svg loader，理由见 popup_comment.md“页头图标改矢量栅格化”。
