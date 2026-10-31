# icon.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。

## MacAppIcon

- macOS 应用图标：调 `NSApplication.setApplicationIconImage:` 装上品牌图标。
- 图标来源分两条路：打包后的 .app 由 cargo-bundle 把 `assets/icons/Gloss.icns` 拷进 Resources、经 Info.plist 的 `CFBundleIconFile` 加载；而 `just run`（cargo 直接跑可执行文件）没有 bundle 可读，Dock 里是系统给的默认图标——这个适配器补的正是这一段。bundle 运行时它同样生效，只是用同一设计的 256px PNG 覆盖 icns 默认值，视觉无差。
- 与其余适配器不同，它不持有资源：PNG 字节由组装点（入口）注入，端口只负责“把给定的图交给平台外壳”。
- `new`：构造适配器（无状态，可自由复制）。

## install

- 主线程标记拿不到就报错退回：NSApplication 只允许主线程访问，而图标是一次性动作，没有“稍后在正确线程补做”的机会，等了也没用。2026-10-30（重构计划阶段 2 · R3）随端口对齐改返回 `Result<(), GlossError>`——失败原因（不在主线程、位图解不出）经错误值带出，调用方日志带因；降级语义不变。
- 调用点在 EventLoop::build 之后、run_app 之前，主线程此刻没有活跃的 autorelease pool（winit 的 pool 只包它自己的内部段），自建一个把 AppKit 解码沿途的临时对象收掉。
- NSImage 直接吃 PNG 字节（系统自带解码），因此不需要引入图像解码库。
