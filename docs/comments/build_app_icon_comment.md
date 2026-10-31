# build_app_icon.py 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关段落时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），文件内不留指向本文件的指针。

## 工具选型

- 2026-09-18 只用系统自带的 sips（SVG→PNG、缩放）与 iconutil（iconset→.icns），零新增工具链：sips 自 macOS 13 起经 ImageIO 支持 SVG 输入，输出保留 alpha。

## 1024×1024 守卫

- 2026-09-18 光栅化尺寸取决于 SVG 声明的 width/height 属性：丢了它 sips 会按内嵌尺寸出小图，下面的档位会把小图上采样成模糊图标静默进仓库，这里当场拦下。

## Dock 图标尺寸

- 2026-09-18 256px 的原因：Retina Dock 上限 128pt@2x = 256px，再大只是增大二进制体积。

## web 站点图标（2026-10-07）

- 补 PNG favicon 的缘由：站点此前只声明 SVG favicon，Safari 不支持（站点受众是
  Mac 用户），标签页一直显示默认图标——用户观感即“站点图标没换”。新增
  `web/assets/favicon.png`（32px）与 `apple-touch-icon.png`（180px）兜底，SVG
  favicon 保留给支持的浏览器。
- web 两张 PNG 源选 `gloss-logo.svg`（满幅稿）而非 `gloss-app-icon.svg`：规范画布
  版四周留 100px 透明边距，缩到 32px 时图形占比过小、标签页里不可辨。
- 纳入本脚本而非一次性手工产出：换装 #100 时站点资产靠手工跟进，这次“图标没换”
  即由此而来；进 `just icons` 后换 Logo/调色重跑一次即全量覆盖，不再依赖记性。
- `web/assets/logo.svg` 拷贝不做程序化同步：沿用 assets/icons/README.md 的手工
  同步约定，避免脚本覆盖站点侧对主标的独立调整；本次只把新增 PNG 收编进脚本。
- 1024 守卫抽成 `raster1024` 供两处矢量源共用：web PNG 同样由 1024 主图降采样，
  小图上采样成模糊图标的风险与 .icns 链完全相同。
