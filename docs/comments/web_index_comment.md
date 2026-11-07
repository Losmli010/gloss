# web/index.html 设计记录

## 2026-09-28 重设计（参照 workbuddy.cn）

### 决策

- **结构映射**（WorkBuddy → Gloss）：
  - 大 hero + 主下载按钮（带平台/架构下拉）→ 分体下载按钮：主按钮直下当前架构，右侧 caret 展开架构菜单（Apple Silicon / Intel，menuitemradio 语义）。
  - “你的工作好帮手”场景卡 → “一个划词，五种任务”：五张任务卡各带迷你浮层示意（词卡/翻译/代码解释/提取结果/图片解释），文案全部取自 `crates/gloss-app/i18n/zh.toml` 与 `gloss-core/src/domain/task.rs` 的 TaskKind，不虚构功能。
  - 生态伙伴墙 → 技术栈 chips 墙（Rust / wgpu / egui / tokio / …）。
  - 底部“开启新范式”CTA + 各平台下载 → “开始使用 Gloss”CTA 带：双架构下载卡（.dmg 主按钮 + .zip 备用 + 大小），加 ad-hoc 签名首次打开指引。
- **产品视觉无真机截图**：hero 右侧用纯 CSS/SVG 拼的“编辑器窗口 + 划词选区 + Gloss 浮层”示意（选区高亮 + 橙色下划线复刻品牌图形），带浮动动画与流式光标；不依赖图片资产。后续有真机截图可直接替换 `.hero-visual`。
- **不虚构数字**：原则条（快/清/小/稳）只用定性表述；安装包大小由 manifest 注入后显示，取不到就不显示。
- **下载契约不变**：同源 `manifest.json`（schema 1，`channels.stable[<triple>].{dmg_url,url,size}`），校验不过或取不到整体降级 GitHub Releases；版本 pill 双处显示（hero + 下载区）。这一契约与 `scripts/release/gen_manifest.py` 与 release.yml 的 deploy-web job 对齐，改动需三处同步。
- **进场动画**：IntersectionObserver 加 `.in`，`prefers-reduced-motion` 下全部禁用；导航滚动加边框。无任何外部依赖。
- **单页改三件套**：拆为 `index.html`（纯结构）+ `assets/style.css` + `assets/main.js`，脚本 `defer` 加载；Pages 部署整个 `web/` 目录，相对路径不受影响。
- **浅色主题**：应用自身是浅色浮层，站点随之改浅色底（`#F5F9FD`）；hero 的编辑器窗口保留深色作“屏幕内容”对比，浮层卡为白色以贴近应用真实观感。配色决策见 `style.css_comment.md`。
- **下载默认匹配本机架构**：WebGL 渲染器名 + UA-CH 两级启发式，见 `main.js_comment.md`。
- **页脚**：应用定位一句话（纯 Rust · 无 WebView）从页脚移除，该信息由技术区的卡片承载。
- **已修的坑**（备查）：`[hidden]` 会被 `.pill{display:inline-block}` 覆盖导致空 pill 渲染成“—”，补了全局 `[hidden]{display:none!important}`；`.arch-card .ic` 是 span（inline），width/height 不生效导致 SVG 巨大，改 flex；小屏隐藏 chip-b 避免与浮层重叠；H1 用 `<br>` 固定两行避免“现”字孤行。
- **删除了旧占位素材** `web/assets/demo-selection.svg`、`demo-stream.svg`（现设计不引用，git 历史可找回）；`assets/logo.svg` 保留。

### 待办

- 有真机截图/录屏后替换 hero 的 CSS mockup。
- 正式分发（Developer ID 公证）后，更新下载区的“首次打开指引”。
- og:image 未设：社交平台普遍不渲染 SVG 预览图，需先产出 PNG 资产；canonical/og:url 目前写死 Pages 项目页地址（`losmli010.github.io/gloss/`），换自定义域名时要同步。

## 2026-09-28 code-review 复审修复（提交前第二轮）

- **P1**：`#nav-split` 初版漏贴 `.menu` 块，导航 caret 是死控件且向读屏播报虚假展开态——已补齐菜单标记并加 `aria-controls`（hero/nav 两处 caret 均指向各自菜单 id）。
- **P2**：`.reveal` 隐藏态加 `.js` 门控（见 main.js 记录）；架构下拉补齐 ARIA menu 键盘契约（见 main.js 记录）；`--faint`/`.guide .why` 对比度加深达 WCAG AA（见 style.css 记录）。
- **P3 打磨**：WebGL 识别加 Mac 平台门禁；尺寸文案带 `zip` 前缀；两处 caret 互斥收起；补 `theme-color`/`canonical`/`og:url`；`.wall` 加 `role="list"`/`listitem`；`#tech` 内联样式改 `.section-flush`；删除 `.mi-size` 未使用的 `data-size`；brand 图标改 `alt=""`（与 footer 一致，避免与相邻文本重复播报）。

## 2026-09-28 导航下载去下拉（用户决定）

- 导航右上角改为单个直链“下载”按钮：href 恒等于当前架构（识别或 hero 菜单选择）的 .dmg 直链，无菜单、无 caret。架构选择的完整入口只保留 hero 分体按钮一处；下载区的双架构卡仍是明确的手动选择入口。

## 2026-09-28 首次打开指引补放行兜底

- **缘由**：v0.1.1 发布后用户实测，右键放行路径之外需要兜底——部分场景右键菜单无效，或用户找不到入口；实测确认 `xattr -dr com.apple.quarantine` 在本机（macOS 13 Intel）有效。
- **新增两条替代路径**（二选一）：系统设置 → 隐私与安全性 → 底部“仍要打开”；终端 `xattr -dr com.apple.quarantine /Applications/Gloss.app`。说明行强调 xattr 只移除下载隔离标记、不改应用内容。
- **上线依赖**：站点内容随 release 部署（deploy-web 只在 tag 触发），此改动要等下一次 release 才会出现在线上页面。

## 2026-09-30 任务场景改用 demo 定稿卡片截图（ux-fixes-plan 块 E）

- **资产来源与再生成**：`web/assets/task-{word,sentence,code,extract,loading}.png` 截自
  `docs/demo/popup-redesign.html`（经注疏定稿，docs/ 不进 git）的各任务完成态。再生成
  方法：把 demo 复制为按任务初始化的变体（`setTab("…")` 换尾行），headless Chrome
  `--headless=new --force-device-scale-factor=2 --window-size=1280,1100 --virtual-time-budget=9000`
  整页截图（虚拟时间快进打字机动画），再按弹窗矩形（变体页里注入的
  `data-rect` 探针，须与截图同 `--window-size`）以 PIL 裁剪。demo 改动后需整批重截。
- **带框截图而非内嵌复刻**（二选一的取舍）：站点是纯亮色主题，暗色卡片以带框
  截图呈现（`.shot-frame`：去内边距、描边圆角、深色底），亮暗场景都成立；内嵌
  CSS 复刻要把经注疏的宋楷字重与朱丝栏在站点再实现一遍，且与 demo 双源漂移。
- **翻译卡用“生成中 → 完成”双图**：加载态骨架是 demo 定稿的加载体验设计
  （触发即骨架、页脚“正在注解”），宽卡有位放下双图（`shot-pair`，≤560px 竖排）。
- **移动端 grid 塌缩**：卡内 `<img width=840>` 的固有宽度会经 grid item 的
  `min-width:auto` 把轨道撑破（视口外溢出）——`.task-card { min-width: 0 }` 与
  img `max-width:100%` 双保险。验证注意：headless Chrome 有 500px 最小窗宽，
  `--window-size=390` 的截图是“500 布局裁 390”的伪影，移动断点要用
  浏览器自动化按 viewport 断言（scrollWidth == innerWidth）或 ≥500 宽目检。

## 2026-10-07 favicon 补 PNG 兜底；更新日志区删 CHANGELOG 卡片

- **favicon 补 PNG**：原只声明 SVG favicon，Safari 不支持 SVG favicon（站点受众
  全是 Mac 用户），标签页一直显示默认图标——“站点图标没换”的观感即此。新增
  `assets/favicon.png`（32px，标签页兜底）与 `assets/apple-touch-icon.png`
  （180px，触摸/收藏），由 `just icons` 从 `gloss-logo.svg` 产出（取舍见
  build-app-icon 记录）；`<link>` 声明顺序 SVG 在前，PNG 在后兜底。
- **更新日志区只剩 GitHub Releases 卡**：CHANGELOG 卡直链仓库 `CHANGELOG.md`，
  但该文件已按“Release 页是唯一来源”决策不落仓库，链接恒 404；用户决定站点
  不留变更日志链接，删卡片。`.log-grid` 随之改单列（原双列会留空半格），只服务
  单卡的 640px 断点覆盖一并删除。逐版本发布说明的入口收敛到 GitHub Releases
  一处（取舍见 release_comment.md）。
- **上线依赖**：两处都随 release 部署（deploy-web 只在 tag 触发），下次 release
  才会出现在线上页面。
- 待办“og:image 未设”推进半步：PNG 产出链已就绪（favicon / apple-touch 已
  产出），社交预览图（1200×630）仍待产出。

## `#tasks` 任务卡（2026-10-12）

- **“提取结果”卡删除**：对应应用的 Extract 任务在 #160 已整体移除
  （`TaskKind::ImageOcr` 与 `OutcomeStructured::Extracted` 变体删除，
  `gloss_popup_seal_note` 词条下架），官网不再宣传不存在的功能；随卡
  截图 `web/assets/task-extract.png` 一并删除（git 历史可回溯）。卡片
  由五改四，节标题同步“四种任务”；剩余两张窄卡（代码解释/图片解释）
  加 `wide` 使 6 列网格保持 2×2 对称。
- **“图片解释”卡描述改为剪贴板触发**：原描述“框选一张图”对应
  `RegionGesture`/`CaptureRegion` 管线——该管线已保留但取材读取栈未
  接线（`src/main.rs` 按未接线丢弃），实际落地路径是剪贴板图片哨兵
  （⌘⌃⇧4 截图到剪贴板，或 ⌘⇧5“存储到剪贴板”）。
- **配图换经注疏完成态卡**（2026-10-12 二次改）：初版用 `shot-img`
  缩略占位，owner 要求与其它卡一致走 demo 定稿的经注疏截图。demo
  （docs/demo/popup-redesign.html）补图片解释 tab（经=附件图、注=
  解读流式、疏=条目，对齐应用图像卡的 ImageCommentary 形态），并加
  `?still=1&tab=&theme=` 静帧截取钩子（隐藏舞台装饰、放开正文高度、
  动画定格、量得卡片尺寸写 body data），headless Chrome 2x 截取为
  `web/assets/task-image.png`（808×834）；原 `.shot-img` 占位样式
  随之删除。桌面 1440 / 移动 500 目检不破版。

## 关联

- 设置页提示语（`crates/gloss-app/i18n/*.toml` 的
  `gloss_settings_watch_clipboard_images_hint`）同日补入正确用法：
  ⌘⇧4 默认存文件不写剪贴板，是“截图监听不触发”排障的第一成因。

## 全站文案围绕经注疏重写（2026-10-28，官网文案与 README 围绕经注疏重写）

- **缘由**：与 README 同一动机——经注疏体例已是产品的第一识别特征，站点
  文案要把它从任务卡的一句描述提升为全站的叙事线；同时把可见文字统一到
  仓库的中文语言规范（中文主体、专名全角括号注记、弯引号、全角标点）。
- **改动落点**（只动文字与引号，不动结构与样式）：
  - meta 与 og 描述、首屏 lede 补“经—注—疏”体例句与三段身位
    （原文为经、译文为注、例句为疏）；技术词首现处给全称括注
    （大语言模型（LLM）、纯本语言（Rust）、图形标准（WebGPU）、
    内嵌浏览器运行时（WebView）、异步运行时（tokio）、图形后端（wgpu）、
    系统图形框架（Metal）、界面库（egui）、服务器推送事件（SSE）、
    本机钥匙串（Keychain）、持续集成（CI））；分节导语的复现处保留
    括注，其余沿用短名。
  - 任务区 lede 补三印章着色（经印用墨、注印用朱、疏印用描边）；词卡补
    宋楷字体位次；翻译卡“正在注解”改为与文案表一致的“正在注疏”——
    原文是词条改版前的旧称，属漂移。
  - 评审打磨（code-review 第一轮）：技术卡“密钥进钥匙串”的正文改称
    “本机钥匙串”，与 hero 首现的称谓与括注统一，同页不再两称；og 描述
    沿用的“免费开源（MIT）”括注因词条未登记（许可证家族此前只收
    AGPL／GPL／LGPL），按 `chinese.py` 白名单注释的登记机制补收
    `MIT` 词条。
  - “流式 & 可取消”标题去半角“&”改“与”；引号统一弯引号
    （“应用程序”“仍要打开”等处）；dl-note 的破折号与“资产名”改
    “安装包名”。
- **刻意不动**：技术栈 chips 墙、hero 的 CSS mockup 文字、下载指引的
  命令行（`xattr -dr …`）——专名与命令不是中文表述，括注化反而伤可读性
  与可复制性；首现处已在 lede 与技术卡括注过，短标签不再重复展开。芯片
  架构等专名标签当时一并暂缓，后续括注化见下条（2026-11-02）。
- **上线依赖**：站点内容随 release 部署（deploy-web 只在 tag 触发），
  本轮文案要等下一次 release 才会出现在线上页面。
- README 侧的同动机改动见 `README_comment.md`。

## 芯片架构与下载文案括注化（2026-11-02，自述与官网文案语言规范收敛）

- **缘由**：与自述重写同一任务——官网描述文字整体对齐中文语言规范；上一
  轮“刻意不动”暂缓的芯片架构专名标签这次一并处理，改译名注记形态。
- **改动落点**（只动文字，结构与样式不动）：
  - 芯片架构双称统一为“苹果芯片（Apple Silicon）”“英特尔芯片
    （Intel）”：hero 下拉菜单与 `main.js` 的 `ARCHES` 标签同改（菜单项与
    动态架构标签共用一套称谓），下载区两张架构卡随改；副标题改“的
    Mac 电脑”收尾。
  - 安装包形态改中文主体：按钮“下载 .dmg”改“下载镜像（.dmg）”、
    “下载 .zip 备用”改“压缩包（.zip）备用”，指引首步“打开下载的
    .dmg”改“打开下载的安装镜像”。
  - 技术区残留：架构图节点“tokio 后台”“LLM 流式”改“异步后台
    （tokio）”“大语言模型（LLM）流式”；技术卡标题“WebGPU 渲染”改
    “图形标准（WebGPU）渲染”；浮动徽章与 chips 墙的“零 WebView”改
    “零内嵌运行时（WebView）”；主按钮“下载 for macOS”改“下载
    macOS 版”。
  - 下载数据标注“zip 5.2 MB”改“压缩包 5.2 MB”（`main.js` 注入文案，
    与“体积是压缩包实测值”的既有注释对齐）；页脚链接“MIT License”改
    “MIT 许可证”。
- **仍按专名放行**：macOS、M1 至 M4、MacBook、iMac、x86_64、GitHub、
  Releases、MIT 单用与键位符号（⌘⏎⌃⇧、Esc）——产品专名与记号的通行词
  无通行译名，保留裸用加盘古空格；站点不在机器门禁受检面，口径以语言规
  范库自查（专名视同登记裸词、快捷键按记号剥除，逐行零违规）与人工评审
  承载。
- **刻意不动的退役**：上一轮“芯片架构等专名标签不动”的取舍就此退役——
  译名注记形态验证后可读性无损，全站称谓一致的价值更高；其余“刻意不动”
  项（chips 墙、mockup 文字、命令行）维持原判。
- README 侧的同动机改动见 `README_comment.md`。

## 三平台化（2026-11-07，Windows 移植 P3/D8）

- **缘由**：Windows 移植 P3 落地（`x86_64-pc-windows-msvc` 便携 zip 与同批
  发布），站点从“macOS 专用”改为“macOS 与 Windows”双平台；与
  `gen_manifest.py` 的发布矩阵扩展（三 target）、应用内清单解析的平台键
  （`manifest.rs`）三处同批对齐，改动需三处同步（承接 2026-09-28 的
  “下载契约不变”条目的三处同步约定）。
- **标题与 hero 去 macOS 限定**：title/og:title 改“选中即译的划词翻译”，
  meta 描述与 lede 改“跨 macOS 与 Windows 的划词翻译工具”；eyebrow 补
  Windows；主下载按钮“下载 macOS 版”改“下载 Gloss”。
- **架构选择器三卡**：hero 下拉菜单与下载区卡各扩“Windows x64”（下载区
  依菜单称谓，不再括注英文名）；Windows 卡主按钮即“下载压缩包（.zip）”，
  无镜像行（Windows 无 dmg）；`.arch-cards` 网格改三列（≤900px 单列），
  第三卡图标用四窗格（Windows 记号的简化形，避免直接画商标）。
- **首次打开指引分平台**：拆 `guide-macos` 与 `guide-windows` 两块，随当前
  架构由 `main.js` 切 `hidden`（脚本不可用时两块都可见，与首行 `js` 标记
  的既有降级约定一致）；Windows 指引为解压 → 双击 `Gloss.exe` →
  SmartScreen“更多信息”→“仍要运行”，说明行口径对齐 macOS 侧
  （未签名 → 一次性确认，不改动程序内容）。macOS 指引内容不动。
- **技术卡事实修正与平台化**：
  - “全局热键”卡改名“划词即触发”：全仓无热键子系统（触发是拖选手势，
    见移植方案 §1.3），原卡宣传了不存在的功能；任务开关表述保留。
  - “图形标准（WebGPU）渲染”卡与技术栈 chips 墙：Metal 一家改
    “Metal/Vulkan/DX12”（后端随目标平台由特性面决定，无代码分支）。
  - “密钥进钥匙串”卡改“密钥只存本机”：macOS 钥匙串（Keychain）与
    Windows 凭据管理器（Credential Manager）分述；hero 的密钥说明随当前
    架构分词（`main.js` 注入）。
  - 图片解释卡补 Windows 截图键位 Win+Shift+S（与 ⌘⌃⇧4 并列）。
  - fallback-note“按芯片架构选择”改“按平台架构选择”。
- **刻意不动**：hero CSS mockup（Safari 标题栏与 ⌘C 页脚）与快捷键记号
  ——示范窗内容属示意，随真机截图待办一并处理；本次范围只收口方案
  D8.4 列出的文案面。
- **上线依赖**：随 release 部署（deploy-web 只在 tag 触发），本次改动要等
  打了 Windows 包的下一个 release 才会出现在线上页面（首发硬前置，见
  移植方案 D8.5）。
