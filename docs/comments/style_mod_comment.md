# ui/style/mod.rs 设计取舍

## 为什么设置提示文案与浮层圆点共用 ACCENT

settings.rs 的 notice 颜色与 popup.rs 的 BRAND_DOT 是同一个值 `#D85A30`（品牌珊瑚橙的两次独立硬编码）。集中化把它们合成一个语义 token。若将来提示文案想换更刺眼的警示色，应新开 token（如 WARN）而不是直接改 ACCENT。

## 错误色 DANGER（2026-09-22 T7）

owner 拍板错误色与品牌色分离：新增 `color::DANGER`（#C62828，与 ACCENT
同饱和度带的正红），用于设置页校验下划线/就地提示/汇总行与保存失败等
运行时提示。ACCENT 收窄为品牌场景（浮层圆点、保存主按钮）。原「设置
提示文案用 ACCENT」的口径就此废止。

## 强调色并入朱砂色板（2026-10-02）

品牌从珊瑚橙换纸墨朱砂后，ACCENT 单档装不下「同一色相明暗两读」——
拆为 `ACCENT_LIGHT`/`ACCENT_DARK`（别名 `SEAL_ZHU_LIGHT`/`SEAL_ZHU_DARK`，
不另存值）。消费点不再直接引常量，走 settings/controls.rs 的 `accent(ui)` 按
`dark_mode` 取档：浅底用压暗档、深底用提亮档。设色逻辑从此与印章
（`seal_zhu`）同构：强调色＝朱砂，只是一次换装、不是新语义。上文
「ACCENT 收窄为品牌场景」的口径不变，值与取色方式随本条更新。

## 颜色语义 token 的用途注记

- `ACCENT_LIGHT`/`ACCENT_DARK` 品牌强调色（朱砂·明暗两档）——设置保存
  主按钮与开关钮的着轨，与印章、朱丝栏、品牌划线同一色板，压暗到白底/
  提亮到深底可读；`DANGER` 错误色——设置页校验失败的下划线与就地提示、
  校验汇总行，与朱砂强调色同饱和度带的正红，明暗主题下均可读；
  `SEAL_ZHU_LIGHT`/`SEAL_ZHU_DARK` 朱砂——经注疏印章的实心印与朱丝栏的
  线色源；`CODE_BG_LIGHT`/`CODE_BG_DARK` 代码面板底色——经位代码面板
  专属底（demo code-bg #f5f7f9 / #16181e），以底色本身与近隐形边从卡片上
  定界；`CODE_BORDER_LIGHT` 代码面板边色（浅色）——demo card-border 的
  10% 墨色（预乘 rgb(2,2,3)），近隐形的定界，不与卡片框线争抢；
  `CODE_BORDER_DARK`（深色）——demo card-border 的 9% 白（预乘）。

## 拆分沿革（2026-10-27 样式统一阶段 1）

原 `ui/style.rs` 拆为 `ui/style/{mod, tokens}.rs`：本文件承接颜色语义决策
（color 模块），字号、间距、圆角与线宽阶梯的决策迁往 tokens 记录；样式
统一后续阶段将把成对常量改为明暗两份解析完毕的调色板数据（Palette），
届时本文件随之更新。
