# assets/icons — 应用图标资源

方案 A"划·译"（glossline）：宣纸底（#F5F1E8）加细墨框——墨短条占五成五宽（原文，次要）→ 浓墨长条（译文，主角）→ 朱砂划线（划词动作，#B8432F），与浮层的经、注、疏视觉层级一一对应。

## 文件

| 文件 | 角色 |
| --- | --- |
| `gloss-logo.svg` | 设计稿主标。满幅 1024 画布，四角圆角即图标底形 |
| `gloss-app-icon.svg` | 系统规范画布版：图形收敛为 824×824 居中、圆角 185.4、四周 100 像素透明边距（Big Sur 之后的应用图标规范——满幅方形在程序坞里会比其它图标大一圈） |
| `Gloss.icns` | 打包产物。打包工具（cargo-bundle）的 `icon` 配置指向它（根 `Cargo.toml`） |
| `gloss-dock-icon.png` | 开发期程序坞（Dock）图标。非打包运行时由入口 `include_bytes!` 内嵌，经 `NSApplication` 装到程序坞 |

**生成链的输入是两份矢量图（SVG）**：图标集产物与程序坞位图由 `gloss-app-icon.svg` 生成，站点的 `favicon.png` / `apple-touch-icon.png`（落在 `web/assets/`）由 `gloss-logo.svg` 生成——改图形改矢量源，不要直接改产物。`gloss-logo.svg` 是设计稿主标，内容与图标版以及站点拷贝 `web/assets/logo.svg` 靠**手工同步**（本目录不做程序化派生）——改设计时各份都要改，改其中一份不会传导到其它份。

## 重建

```bash
just icons    # = scripts/dev/build_app_icon.py
```

只用系统自带的 `sips`（矢量图到位图的转换与缩放）与 `iconutil`（图标集到打包图标的组装）：`sips` 自系统（macOS）13 起经图像框架（ImageIO）支持矢量图输入，输出保留透明通道（alpha），因此不需要 rsvg-convert、Inkscape、ImageMagick 一类的额外工具链。产物进仓库，持续集成不重建。
