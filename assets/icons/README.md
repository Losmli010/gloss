# assets/icons — 应用图标资源

方案 A「划·译」（glossline）：灰条（原文，次要）→ 白条（译文，主角）→ 橙色划线
（划词动作），与浮层的视觉层级一一对应。

## 文件

| 文件 | 角色 |
| --- | --- |
| `gloss-logo.svg` | 设计稿主标。满幅 1024 画布，四角圆角即图标底形 |
| `gloss-app-icon.svg` | macOS 规范画布版：图形收敛为 824×824 居中、圆角 185.4、四周 100px 透明边距（Big Sur 之后的应用图标规范——满幅方形在 Dock 里会比其它图标大一圈） |
| `Gloss.icns` | 打包产物。cargo-bundle 的 `icon` 配置指向它（根 `Cargo.toml`） |
| `gloss-dock-icon.png` | 开发期 Dock 图标。非 bundle 运行时由入口 `include_bytes!` 内嵌，经 `NSApplication` 装到 Dock |

**矢量源只有 `gloss-app-icon.svg` 一份**：`.icns` 与 Dock PNG 都由它生成。改图形改它，
不要直接改产物。

## 重建

```bash
just icons    # = scripts/dev/build-app-icon.sh
```

只用系统自带的 `sips`（SVG→PNG、缩放）与 `iconutil`（iconset→.icns）：`sips` 自
macOS 13 起经 ImageIO 支持 SVG 输入，输出保留 alpha，因此不需要 rsvg-convert /
Inkscape / ImageMagick 一类的额外工具链。产物进仓库，CI 不重建。
