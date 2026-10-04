# 测试行为清单（BDD）

以测试代码为唯一事实源：本清单逐条描述仓库当前测试的行为；新增、修改、删除测试时在同一 PR 内登记并刷新该条目的更新时间；描述与代码冲突时以代码为准并立即修正。

组织：人工测试 → 发版人工步骤 → 集成测试 → 性能测试 → 快照测试 → 单元测试。条目四字段：测试名称、测试目标、测试场景（给定/当/则）、更新时间。

## 总览

| 类别 | 数量 | 运行 |
| --- | --- | --- |
| 人工测试 | 11 | `cargo test -p gloss-platform -- --ignored` |
| 发版人工步骤 | 4 | 手动执行（发版链路运维步骤，无自动化测试源码，bdd 门禁豁免） |
| 集成测试 | 10 | `just test` |
| 性能测试 | 1 | `just selftest` |
| 快照测试 | 32 | `just test` |
| 单元测试 | 465 | `just test` |

## 人工测试

### reads_live_selection_when_authorized
- 测试目标：验证辅助功能授权下的真实选区读取。
- 测试场景：给定授权真机与前台选中文本，当 reader.read()，则读出非空选中文本。
- 测试步骤：
  1. 系统设置 → 隐私与安全性 → 辅助功能 → 放行运行测试的终端 App
  2. 在前台编辑器选中一段文字
  3. 运行总览中人工测试的命令
  4. 未授权时测试当场失败并打印修复指引
- 更新时间：2026-09-19

### reads_live_selection_via_simulated_copy
- 测试目标：验证辅助功能授权下经剪贴板兜底的真实选区读取。
- 测试场景：给定授权真机且前台有选中文本，当跑兜底读取，则读出非空文本。
- 测试步骤：
  1. 系统设置 → 隐私与安全性 → 辅助功能 → 放行运行测试的终端 App
  2. 前台保留可复制的文字
  3. 运行总览中人工测试的命令
  4. 测试会向前台应用注入 Cmd+C 并覆写系统剪贴板；全系列经 CLIPBOARD_LIVE_LOCK 串行，勿手动并行触发
- 更新时间：2026-09-19

### reads_live_selection_from_rich_clipboard_via_simulated_copy
- 测试目标：验证富剪贴板场景下兜底读取的恢复保真。
- 测试场景：给定预置富剪贴板与授权真机，当跑兜底读取，则读出选中文本且预置文本/自定义 flavor 原样恢复。
- 测试步骤：
  1. 系统设置 → 隐私与安全性 → 辅助功能 → 放行运行测试的终端 App
  2. 前台保留可复制的文字
  3. 运行总览中人工测试的命令
  4. 测试预置的剪贴板内容会被写入并恢复；经 CLIPBOARD_LIVE_LOCK 串行
- 更新时间：2026-09-19

### injected_drag_yields_selection_gesture
- 测试目标：验证真实 CGEventTap 下的注入拖拽手势判定。
- 测试场景：给定辅助功能授权与 rdev 注入的拖拽序列（(100,100) 按下 → 五段移动 → 释放前真实睡过最短按压时长），当经真实事件 tap，则 2s 内监听到手势且监听器未降级。
- 测试步骤：
  1. 系统设置 → 隐私与安全性 → 辅助功能 → 放行运行测试的终端 App
  2. 运行总览中人工测试的命令
  3. 测试注入真实全局鼠标事件，屏幕光标会移动
  4. 未授权时前置检查当场失败并打印修复指引
- 更新时间：2026-09-30

### keychain_round_trip_on_real_store
- 测试目标：验证真实 keychain 的写→读→覆盖→删除往返。
- 测试场景：给定真实 keychain 测试服务名，当写→读→覆盖→删除，则各步读回一致、终态 None。
- 测试步骤：
  1. 在非受限会话的终端运行总览中人工测试的命令
  2. 沙箱或 CI 会拒绝 keychain 写入，属预期环境限制
- 更新时间：2026-09-19

### live_llm_streams_a_translation
- 测试目标：验证真实 LLM 端点的流式翻译往返。
- 测试场景：给定 GLOSS_LIVE_API_KEY / GLOSS_LIVE_BASE_URL / GLOSS_LIVE_MODEL 三个变量与真实端点，当请求翻译并消费流，则累计 80+ 字符非空译文。
- 测试步骤：
  1. 导出 GLOSS_LIVE_API_KEY / GLOSS_LIVE_BASE_URL / GLOSS_LIVE_MODEL 三个环境变量（密钥不打印）
  2. 运行总览中人工测试的命令
  3. 缺变量时测试当场打印可照抄的导出命令
- 更新时间：2026-09-19

### fallback_read_restores_original_clipboard
- 测试目标：验证兜底读取后系统剪贴板恢复原内容。
- 测试场景：给定预置文本的系统剪贴板，当跑一次兜底读取（成败皆可），则原内容恢复。
- 测试步骤：
  1. 随常规测试自动运行，无需授权与 --ignored
  2. 运行中会覆写并恢复本机剪贴板；经 CLIPBOARD_LIVE_LOCK 串行
- 更新时间：2026-09-19

### fallback_read_restores_multiflavor_clipboard
- 测试目标：验证多 flavor 剪贴板的恢复保真。
- 测试场景：给定「文本 + 自定义 flavor」双 flavor 条目，当跑一次兜底读取，则两种 flavor 字节原样恢复。
- 测试步骤：
  1. 随常规测试自动运行，无需授权与 --ignored
  2. 运行中会覆写并恢复本机剪贴板；经 CLIPBOARD_LIVE_LOCK 串行
- 更新时间：2026-09-19

### fallback_read_restores_empty_clipboard
- 测试目标：验证空剪贴板场景的兜底不引入内容。
- 测试场景：给定空剪贴板，当跑一次兜底读取，则剪贴板保持为空。
- 测试步骤：
  1. 随常规测试自动运行，无需授权与 --ignored
  2. 运行中会覆写并恢复本机剪贴板；经 CLIPBOARD_LIVE_LOCK 串行
- 更新时间：2026-09-19

### fallback_read_restores_multi_item_clipboard
- 测试目标：验证多条目剪贴板的逐条目恢复。
- 测试场景：给定两条目各带文本与自定义 flavor，当跑一次兜底读取，则四个 flavor 各归回原条目。
- 测试步骤：
  1. 随常规测试自动运行，无需授权与 --ignored
  2. 运行中会覆写并恢复本机剪贴板；经 CLIPBOARD_LIVE_LOCK 串行
- 更新时间：2026-09-19

### scene_probe_reports_the_frontmost_app
- 测试目标：验证真机上 NSWorkspace 取前台应用这条路径（CI 里只能断「一致快照」，`Some` 分支跑不到）。
- 测试场景：给定有前台应用的图形会话，当读一次场景事实，则报出前台应用且其身份字段非空。
- 测试步骤：
  1. 在有窗口服务的会话里运行（无前台应用的环境会当场失败）
  2. 运行总览中人工测试的命令
  3. 前台开着任意应用即可；在密码管理器内划词的行为另见 PR 走查清单
- 更新时间：2026-09-26

## 发版人工步骤

发版链路的一次性运维与真机步骤：没有可执行的自动化测试源码，bdd 门禁对本节豁免双向核对；
条目仍按「名称、目标、场景、照抄步骤、更新时间」登记，命名用测试风格的标识符。

### pages_source_is_github_actions
- 测试目标：验证仓库 GitHub Pages 已启用且 source 为 GitHub Actions（deploy-web job 的前置，一次性）。
- 测试场景：给定仓库管理员权限，当查看 Settings → Pages，则 Build and deployment 的 Source 为 GitHub Actions。
- 测试步骤：
  1. 打开 https://github.com/Losmli010/gloss/settings/pages
  2. Build and deployment → Source 选 GitHub Actions（若尚未选择）
  3. 保存即可，无需手工建分支（deploy-web job 用 actions/deploy-pages 直接部署）
- 更新时间：2026-09-26

### pages_site_point_check
- 测试目标：验证 Pages 部署后站点的导航锚点、下载按钮、manifest 读取与降级路径。
- 测试场景：给定一次成功的 deploy-web 部署，当浏览器访问站点逐项点检，则锚点跳转正常、下载按钮指向 latest/ 对应架构直链、manifest.json 可读取且字段齐备；manifest 取不到时页面降级为 GitHub Releases 外链。
- 测试步骤：
  1. 打开 https://losmli010.github.io/gloss/
  2. 依次点导航「演示 / 功能 / 下载 / 更新日志」，确认锚点跳转
  3. 确认版本号显示；切换 Apple Silicon / Intel，确认下载链接随之指向对应架构的 latest/ 文件
  4. 直接访问 https://losmli010.github.io/gloss/manifest.json，确认 schema/version/channels 双架构字段齐备
  5. 降级路径：本地 `just site-preview`（无 manifest.json）打开页面，确认显示「无法获取最新版本信息」且下载按钮退到 GitHub Releases 外链
- 更新时间：2026-09-26

### real_update_round_trip_on_device
- 测试目标：验证真机上的真实清单拉取、整包下载校验与替换重启，双架构各一次。
- 测试场景：给定装有旧版 Gloss 的真机（有网络），当设置页手动检查更新并确认下载、确认重启替换，则应用升到清单版本并正常启动，旧 bundle 无残留。
- 测试步骤：
  1. 在 Apple Silicon 与 Intel 真机各装上一个发布版本的 Gloss
  2. 设置页点「检查更新」，确认提示新版与目标版本
  3. 确认下载，等待「更新就绪」提示
  4. 点「重启更新」，确认替换后应用以新版本启动
  5. 检查 /Applications 无 .app.old 残留；`just logs` 无 panic
- 更新时间：2026-09-26

### release_pipeline_end_to_end
- 测试目标：验证打 tag 后 Release 与 Pages 同步发布的全链路。
- 测试场景：给定与版本单点一致的 tag，当推送 tag 触发 release workflow，则 build/release 产出 Release 资产、deploy-web 部署成功，站点 manifest.json 的 version 与 tag 一致。
- 测试步骤：
  1. 本地 `just release-check vX.Y.Z` 确认版本一致后打 tag 并推送
  2. 在 Actions 观察 release workflow：build → release → deploy-web 依次成功
  3. 打开 GitHub Release，确认双架构 zip/dmg 共 4 个资产
  4. 打开站点确认 manifest.json 的 version 与 tag（去 v）一致，latest/ 下 4 个文件可下载
- 更新时间：2026-09-26

## 集成测试

文件：crates/gloss-app/tests/pipeline.rs（L1，经公共 API 与通道两端驱动状态机 + 通道③④ + tokio 桥 + mock 引擎的全时序）。

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| full_flow_classifies_then_streams_and_settles | 全链路分类+流式回流与终态 | 给定划词手势注入契约 JSON（仅 body 字段）的流式脚本，当全链路推进，则先回 TaskClassified(TranslateWord)（分类解析不出 kind 落兜底）、chunk 逐条回流、TaskDone 后定格 Show、正文取自 body 字段、词卡字段缺失按契约落空卡 | 2026-10-03 |
| classify_failure_falls_back_and_the_task_still_completes | 分类失败回退兜底且任务照常完成 | 给定分类请求注入一次性失败，当桥编排分类，则 TaskClassified 携带兜底 kind、告警无内容、重建任务照常执行完成落 Show | 2026-09-26 |
| code_language_hint_skips_the_classification_round_trip | 代码语言提示直通分类 | 给定带 CodeLanguage 提示的 Auto 任务，当桥编排，则 TaskClassified 恒为 ExplainCode、仅任务执行一次引擎调用（零分类往返） | 2026-09-26 |
| second_trigger_is_a_full_cache_hit_without_engine_calls | 二次触发全缓存命中直出 | 给定同一 input+options 的任务第二次触发，当桥按 cache_key(input, options) 查产物缓存命中，则仅回 TaskClassified+TaskDone（无 TaskChunk）、引擎调用数维持 2（首轮分类+执行）、分类 kind 随缓存产物回放、状态定格 Show | 2026-10-03 |
| hide_overlay_cancels_the_stream_and_late_events_are_dropped | 收起浮层取消流并丢弃迟到事件 | 给定慢流中已收到分类结果与首个 chunk，当 hide_overlay 取消在途令牌，则不再有任何回传事件、机器侧拒绝该任务的迟到 chunk/产物并回 Idle | 2026-09-26 |
| superseded_trigger_cancels_and_filters_late_events | 新触发取消旧任务并过滤迟到事件 | 给定 A 的分类流未完成时触发 B，当 B 触发，则 A 的令牌立即取消、代数 +1、A 代数的迟到事件被状态机拒绝，B 经分类回退后 chunk/done 正常回流至 Show | 2026-09-26 |
| failure_lands_in_error_and_retry_succeeds | 失败落错误态且重试可达 | 给定 CodeLanguage hint 固定 kind 任务首次注入 EngineRateLimited 失败，当失败回传后再次触发，则落 Error 态、第二次任务完成落 Show | 2026-10-03 |
| error_card_retry_redispatches_the_same_request | 重试动作重发同一请求 | 给定 CodeLanguage hint 固定路径的可重试失败 Retry 出口，当 retry 并重发 RunTask（input+options 原样），则同代数重发同一请求并完成落 Show | 2026-10-03 |
| frozen_options_carry_the_factory_model_by_default | 冻结选项默认带出厂模型 | 给定出厂配置的划词提交（hint 直通），当下发 RunTask，则 options.model 为 DEFAULT_TEXT_MODEL（快照冻结面） | 2026-10-03 |
| config_change_invalidates_cache_for_the_next_task | 配置变更对主缓存 key 的失效 | 给定 CodeLanguage hint 固定 kind 的同文本连续任务与运行时保存的新配置，当执行，则未改配置命中缓存（引擎 1 次）、换模型与换目标语言各触发一次重新请求（共 3 次） | 2026-10-03 |
| engine_logs_carry_the_task_span | 桥日志经 span 带上代数 | 给定带 span 的任务命令（进程级捕获订阅者），当消费桥执行到缓存命中，则命中行同时含 cache hit 与 "generation":2 | 2026-09-26 |
| legacy_fence_contract_falls_back_to_a_complete_card | 旧围栏契约落 fallback 出完整卡 | 给定旧契约（markdown + 末尾 gloss 围栏）的两段脚本与代码语言 hint（Plain 系 kind），当跑完整桥，则完成态走围栏 fallback、正文剥离围栏、title 进结构化（模型跑偏时产物不丢；word kind 缺 senses 按 core 原语义整体回退） | 2026-10-03 |

## 性能测试

文件：tests/overlay.rs（L3，harness = false 自带 main()，跑在主线程，需窗口服务与 GPU）。

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| overlay | 真实窗口栈显隐生命周期与首帧预算 | 给定经公共 API build_window_stack 预创建的生产窗口栈，当反复 show → 渲染 → hide 共 100 轮（每轮停留 80ms），则统计 show→首帧延迟并计入门禁：跑满 100 轮且有延迟统计退出 0，无帧或首帧超 100ms 预算退出 1；窗口句柄数仅进日志供人工走查（验证复用不增长）；设 GLOSS_PERF_OUT 时追加一行 JSON 性能记录（first/p50/p95/max、预算判定与运行环境），写失败只记日志不改变退出码 | 2026-09-21 |

## 快照测试

popup 快照基线：popup_word_card、popup_streaming、popup_extract、popup_failed、popup_failed_auth、popup_selfcheck、popup_code_streaming、popup_code_outcome；settings 快照基线：settings_main、settings_notice、settings_invalid、settings_default_kind_disabled、settings_update_up_to_date、settings_update_available、settings_update_downloading、settings_update_ready、settings_update_failed_install。全部基线统一英文文案 + 浅色主题渲染（2026-10-01 起）：kittest 自建上下文无 CJK 字体，英文基线可读可审。

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| word_card_exposes_entries_to_accesskit | 词卡视图的无障碍树结构 | 给定双释义的词卡 Outcome 视图，当渲染，则 AccessKit 树可按文本定位节点：gloss、音标、两条释义、例句（复制按钮已移除，划选即复制） | 2026-10-01 |
| word_card_senses_stack_vertically | 释义逐行向下排 | 给定两条释义的词卡视图，当渲染，则第二条释义的节点矩形在第一条之下（正文列显式垂直布局，不横向并排） | 2026-10-01 |
| watermark_sits_inside_the_computed_window_height | 页脚水印落在期望窗口内 | 给定流式/词卡两种视图，当按内容尺寸重设渲染窗口再量水印矩形，则水印底边不超出 draw 上交的期望窗口高（页脚被内容挤出窗外即失败） | 2026-10-03 |
| long_body_is_rendered_in_full | 长正文完整渲染不截断 | 给定超长正文（尾部带标记），当渲染，则 AccessKit 树含尾部内容——无字符截断 | 2026-09-20 |
| streaming_view_shows_the_annotating_footer | 流式视图页脚注解指示 | 给定流式视图，当渲染，则「正在注解」在页脚出现、经/注印章就位 | 2026-10-01 |
| word_card_marks_the_three_sections_per_locale | 词卡三分区印章随 locale | 给定词卡视图（zh/en 各一），当渲染，则印章字分别为 经/注/疏 与 SRC/NOTE/EXP | 2026-09-30 |
| extract_view_notes_the_measurement | 提取视图疏位小记 | 给定提取产物视图，当渲染，则提取文本在经位、疏位附「凡 N 言 · N 行」小记（字数去空白、行数按换行） | 2026-09-30 |
| streaming_view_shows_only_the_extracted_body | 流式视图只显示提取出的 body | 给定流式视图（正文为 JSON 契约原始流），当渲染，则「已流式到达的正文」「选中的原文」可见而 title 字段、JSON 残片与 ```gloss 围栏均不在树中（body 渐进提取）；头部只有图标与动作区 | 2026-10-03 |
| long_lines_never_exceed_the_window_width | 长行不超窗口可用宽 | 给定长中文段落 + 围栏代码块与长 token 代码原文两类视图，当以 380 宽渲染，则全部内容节点右缘不超窗口宽（横滚不进弹窗）；popup_long_line 基线锁定形态 | 2026-10-03 |
| failed_view_shows_retry_hint | 失败卡重试动作 | 给定 Retry 失败卡，当渲染并点击「重试」，则收集器收到 OverlayAction::Retry | 2026-09-21 |
| auth_failed_view_offers_open_settings | 鉴权失败卡设置入口 | 给定鉴权失败卡，当渲染并点击「打开设置」，则收到 OverlayAction::OpenSettings（头部齿轮标签为「设置」，与正文按钮不混淆） | 2026-09-21 |
| bare_failed_view_has_no_action_button | 无动作失败卡形态 | 给定 action=None 失败卡，当渲染，则无「重试」节点、无动作上交 | 2026-09-19 |
| close_button_submits_dismiss | 头部关闭按钮上交收起 | 给定词卡视图（头部为各视图共用路径），当点击无障碍标签「关闭浮层」的 × 钮，则 draw 产物为 OverlayAction::Dismiss | 2026-09-21 |
| gear_button_submits_open_settings | 头部齿轮上交打开设置 | 给定词卡视图，当点击无障碍标签「设置」的 ⚙ 钮，则 draw 产物为 OverlayAction::OpenSettings | 2026-09-21 |
| header_drag_strip_is_exposed_to_accesskit | 页头拖动热区进无障碍树且与动作钮零重叠 | 给定词卡视图，当渲染，则无障碍树有「拖动浮层」热区节点、矩形与页头行同高（不小于头部图标边长）且水平区间止于最左动作钮左缘（收 DRAG_STRIP_INSET，与齿轮/× 零重叠） | 2026-10-03 |
| header_drag_reports_cumulative_offset_and_ends_on_release | 页头拖动上交自按压点的累计位移 | 给定词卡视图与页头热区中心，当按下→两段移动→松开→再移动，则上交位移自按压点累计（按下为零、两段各 (20,10)/(30,15)，非逐帧增量）、松开后不再上交；按压点按物理像素记录（跨 DPI 显示器不混尺）；落点换算与屏幕钳制在壳侧 apply_overlay_drag（无状态：以窗口实际位置为基准，中途被动过会被下一帧落点吸收） | 2026-10-03 |
| selfcheck_view_exposes_texts_to_accesskit | 自检卡无障碍树 | 给定 view=None 的自检渲染，当渲染，则中英文自检文本均可定位 | 2026-09-21 |
| snapshots_match_baseline（popup） | 浮层八视图渲染基线（英文浅色） | 给定八个视图（词卡/流式/提取/失败/鉴权失败/自检卡/代码流式/代码完成态，内容夹具为英文），当 wgpu 以英文文案与浅色主题渲染并 diff，则与 popup_word_card / popup_streaming / popup_extract / popup_failed / popup_failed_auth / popup_selfcheck / popup_code_streaming / popup_code_outcome 八份基线一致，结果合并进单个 SnapshotResults（基线沿革：2026-10-01 按经注疏 demo 定稿重录：宋楷命名字体族、三印、疏区虚线、常驻页脚带水印；同日随水印槽改实测宽再录，差异仅水印字形位置；2026-10-02 随页头底缘发丝线（与行内容隔 ITEM 间距）、关闭 × 调小至 10×10 与页脚降高 26→20 再录；同日 popup_word_card 再随音标改 gloss-mono 等宽族重录，差异仅音标字形行——kittest 绑内置字形，缺字实测：内置 Hack 缺 14/18、Ubuntu-Light 缺 13/18、PingFang SC 缺 ɒ ʒ ʌ ˈ ˌ ː，链上无一命中；2026-10-02 新增 popup_code_streaming / popup_code_outcome 两份代码视图基线（T3：classified 首帧即代码排版、单层代码面板——代码底色直接覆盖经位、左上语言标签行；无高亮的纯色等宽，高亮属 T4；面板样式随用户反馈图样定稿同日再录）；2026-10-03 随热键支持移除、Acquiring 骨架视图删除，popup_loading 基线一并移除，余八份） | 2026-10-03 |
| all_sections_render_and_save_submits_the_draft | 设置窗渲染与保存提交 | 给定默认配置的设置窗口，当渲染并点保存，则各区块控件可定位且上交未改动的出厂快照 | 2026-09-19 |
| cancel_and_clear_key_actions_are_submitted | 取消与清除密钥动作 | 给定「取消」与「清除密钥」按钮，当分别点击，则取消上交 Close、清除只置标记（按钮变「撤销清除」）、保存时才上交 Clear | 2026-09-19 |
| invalid_save_is_blocked_with_field_hints | 非法草稿保存被阻断并就地提示 | 给定非法 Base URL 的设置窗，当点保存，则不上交 Save、字段就地标红并出汇总行 | 2026-09-22 |
| snapshots_match_baseline（settings） | 设置窗渲染基线（英文浅色；正常/提示/错误三态 + 更新区五相位） | 给定默认、带保存失败提示、校验错误三个状态与更新区五个相位（UpToDate/Available/Downloading/Ready/Failed(Install)），当 wgpu 以英文文案与浅色主题渲染并 diff，则与对应基线一致且关键文本进树（沿革：2026-10-03 随热键区删除、热键校验错误移除重录；同日随职责重划删除任务开关/默认任务/每类模型三区块、新增单一模型输入行重录并移除默认任务停用态，又随「模型 ID」字段标签再录） | 2026-10-03 |
| failure_card_words_each_cause | 失败卡按变体出文案 | 给定网络失败、协议异常（带诊断）、取材通道不可用、推理通道不可用四种失败起因，当渲染，则各出对应文案（协议异常保留诊断文本） | 2026-09-22 |
| failure_card_follows_the_locale | 失败卡随 locale 出表 | 给定英文 locale 的网络失败卡，当渲染，则出英文文案与英文「Retry」动作 | 2026-09-22 |
| save_failure_notice_names_the_cause | 保存失败提示出场合与诊断 | 给定带 SaveFailed(Config) 类型化提示的设置窗（中文表），当渲染，则出「保存失败：<诊断>」（前缀交代场合、诊断不重复本地化整句） | 2026-09-22 |
| a_rendered_notice_follows_the_locale | 提示行随 locale 出表 | 给定带同一类型化提示的设置窗（英文表），当渲染，则出英文前缀与诊断（提示行不是只有中文基线可查） | 2026-09-22 |
| saving_the_language_swaps_the_rendered_labels_without_a_restart | 保存语言即换渲染文案 | 给定同一进程内保存 Language::En 前后的配置句柄，当各渲染一帧设置窗，则文案由「保存」变为「Save」且中文标不再在树上（配置快照 → 落定 → 选表 → 渲染全链，不重启） | 2026-09-22 |
| english_catalog_relabels_the_settings_window | 英文文案表驱动设置窗 | 给定英文 locale 的设置窗，当渲染，则四个区块标、动作按钮、默认任务/目标语言/任务开关/清除密钥/界面语言/界面主题/缓存有效期各出自英文表（含「Enable <任务>」开关的无障碍标签模板），且中文标不在树上 | 2026-09-24 |

## 单元测试

### crates/gloss-core/tests/stubs_behavior.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| selection_reader_mock_returns_presets | 选区读取桩的两路透传 | 给定预置的成功/失败结果，当调用 SelectionReader 桩的 read，则两路都原样返回（Ok 与 AccessibilityDenied） | 2026-09-19 |
| region_capture_mock_returns_png | 区域截图桩的字节透传 | 给定预置 PNG 字节，当 capture 一个 4×4 区域，则返回同一份 Arc 缓冲（ptr_eq 断言） | 2026-09-19 |
| config_store_mock_round_trips_secrets | 密钥存取桩往返 | 给定密钥未设置时读为 None，当 set_secret 后再读，则读回写入值 | 2026-09-19 |
| config_store_mock_round_trips_document | 配置文档桩往返 | 给定带非默认 language 的配置文档，当 save 与 load，则往返无损且与出厂默认可区分 | 2026-09-22 |
| chunk_delay_paces_the_stream | chunk 延迟为流定速 | 给定 30ms chunk 间延迟的三段脚本，当消费流，则内容按序且总耗时下界为两段延迟 | 2026-09-19 |
| failures_are_injectable | 失败位置可注入 | 给定注入的各类 GlossError 与流中 Err，当 execute，则失败在注入位置原样发生、流继续按脚本 | 2026-09-19 |
| execute_failure_once_fails_exactly_once | 一次性失败只生效一次 | 给定注入一次性失败与恢复脚本的引擎，当连续两次 execute，则首次返回注入错误、第二次照常产流 | 2026-09-19 |
| execute_panic_fires_on_first_poll | panic 注入在首次 poll 触发 | 给定注入一次 panic 的引擎，当在 tokio 任务里驱动 execute，则任务以 panic 收场 | 2026-09-19 |
| call_count_tracks_execute_invocations | 调用计数如实增长 | 给定多次 execute（含克隆体），当读 call_count，则共享计数如实增长 | 2026-09-19 |
| empty_script_yields_empty_stream | 空脚本产出空流 | 给定空脚本，当 execute 并消费，则立即结束 | 2026-09-19 |

### crates/gloss-core/src/log.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| init_is_idempotent | 初始化幂等 | 给定已初始化的日志，当再次 init(None)，则无副作用不 panic | 2026-09-19 |
| file_writer_creates_missing_directory | 日志目录自动创建 | 给定不存在的目录，当建 file writer，则目录被创建且 active 路径等于它 | 2026-09-19 |
| file_writer_persists_lines_into_daily_file | 日志按日文件落盘 | 给定写出的一行日志，当 drop guard 刷盘后检查，则落入按日命名的文件且内容含该行 | 2026-09-19 |
| file_writer_degrades_when_directory_is_unusable | 目录不可用时优雅降级 | 给定路径被普通文件占用，当建 writer，则返回 None | 2026-09-19 |
| filter_falls_back_to_default_when_unset_or_empty | 过滤器缺省回退 | 给定 None 或空 RUST_LOG，当 build_filter，则得 info | 2026-09-19 |
| filter_keeps_default_level_alongside_module_directives | 模块指令与默认级别并存 | 给定模块指令，当解析，则模块级与默认 info 并存 | 2026-09-19 |
| filter_lets_global_directives_override_default | 全局指令覆盖默认级别 | 给定 off/warn/error 全局指令，当解析，则覆盖默认 info | 2026-09-19 |
| filter_drops_invalid_directives_but_keeps_valid_ones | 非法指令丢弃、合法保留 | 给定含非法指令的串，当解析，则非法项丢弃、合法项保留 | 2026-09-19 |
| json_lines_carry_the_structured_contract | 日志行是结构化 JSON | 给定带 thread/kind 字段的一条日志，当解析该行，则 level/message/thread/kind/target 均在顶层且行内无 ANSI 转义 | 2026-09-23 |
| logs_outside_a_task_carry_no_generation | 流程外的日志不带代数 | 给定任务 span 之外记的一条日志，当解析该行，则既无 generation 也无 span 对象 | 2026-09-24 |
| civil_date_matches_known_anchors | 天数换算公历 | 给定 1970-01-01 / 2000-01-01 / 2026-09-24 / 2026-12-31 对应的天数，当换算，则年月日与已知值一致 | 2026-09-24 |
| log_file_name_is_dated_jsonl | 日志文件名带日期与后缀 | 给定日期 2026-09-24，当取名，则得 gloss-2026-09-24.jsonl | 2026-09-24 |
| daily_writer_appends_into_todays_file | 按天文件追加写入 | 给定两个写入器实例写同一份今日日志，当读回，则两行都在今天的文件里 | 2026-09-24 |
| prune_keeps_the_newest_files_only | 旧日志按天数保留 | 给定 9 份日志与一个无关文件，当清理，则只留最近 7 份日志且无关文件不动 | 2026-09-24 |
| task_span_carries_generation_into_events | 任务 span 把代数带给范围内的日志 | 给定 task_span(7) 并在其中记一条日志，当格式化输出，则事件行是 JSON、顶层 generation=7 且无 span 对象 | 2026-09-24 |

### crates/gloss-core/src/classify.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| code_language_hint_short_circuits_without_the_engine | 代码语言提示直通不调引擎 | 给定带 CodeLanguage 提示的文本输入，当 classify，则恒得 ExplainCode 且引擎 0 调用 | 2026-09-26 |
| non_text_input_is_rejected | 非文本输入拒绝分类 | 给定 Audio 输入，当 classify，则 UnsupportedModality | 2026-09-26 |
| bare_json_reply_selects_the_kind | 裸 JSON 回复解析 kind | 给定 {"kind":"TranslateWord"} 回复，当 classify，则得 TranslateWord | 2026-09-26 |
| fenced_reply_is_extracted_despite_the_contract | 围栏回复容错提取 | 给定模型不守约输出的 ```json 围栏回复，当 classify，则提取围栏内 JSON 并判出 ExplainCode | 2026-09-26 |
| a_complete_json_settles_before_the_stream_ends | 完整 JSON 先于流结束定型 | 给定「半截 → 闭合 → 流内错误」的增量序列，当 classify，则返回首个完整 JSON 的判定（等流结束就会拿到那个错误） | 2026-09-30 |
| the_first_complete_json_wins_over_later_deltas | 首个完整 JSON 胜出 | 给定两段各自完整的 JSON 增量，当 classify，则返回前者（拼接后两端都解析不过，只有提前退出才拿得到） | 2026-09-30 |
| partial_json_keeps_waiting_for_the_stream | 半截 JSON 继续等流 | 给定到流结束都补不齐的半截 JSON，当 classify，则走完流并按解析失败收口（不静默回退） | 2026-09-30 |
| engine_failure_propagates | 分类请求失败原样上抛 | 给定分类请求整体失败与流中失败，当 classify，则错误原样上抛（回退由桥编排） | 2026-09-26 |
| replies_outside_the_allowed_list_are_rejected | 回复越界/不可识别一律拒绝 | 给定清单外 kind、未知标识、null、纯散文、空串与 gloss 围栏六种回复，当解析，则前五者 EngineResponse、围栏内合法 kind 放行 | 2026-09-26 |
| classify_constants_cover_the_text_kinds_with_a_concrete_fallback | 分类清单与兜底常量 | 给定 CLASSIFY_KINDS 与 CLASSIFY_FALLBACK，当检查，则清单恰为三个文本 kind、兜底是清单内的具体 kind | 2026-10-03 |

### crates/gloss-core/src/config.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| factory_defaults_match_spec | 出厂默认符合规格 | 给定出厂 Config，当逐字段抽查，则默认语言/任务/提供商/文本模型等全部符合规格 | 2026-09-19 |
| config_round_trips_through_serde | 配置 serde 往返无损 | 给定含 Lang::Other 等携数据变体的完整配置，当 serde_json 往返，则无损 | 2026-09-19 |
| partial_document_fills_factory_defaults | 部分文档补全出厂默认 | 给定只写 theme 的 JSON，当加载，则该字段保留、其余走出厂默认（含全部 kind 启用） | 2026-09-19 |
| retired_fields_are_ignored_on_load | 退役字段仍能加载 | 给定含 guard_enabled / guard_blocked_apps / hotkey_bindings 的旧配置（字段已从 Config 移除），当加载，则照常读出、已知字段取值不变、缺字段仍回出厂默认——旧版本落盘不会被当成非法配置隔离降级 | 2026-10-03 |
| language_resolves_to_locale | 界面语言落定具体 locale | 给定 System/Zh/En 三态与注入的系统语言，当解析界面语言，则 System 取系统语言、显式选择不被系统语言覆盖（同一处取值供 UI 文案表与 prompt 模板选表） | 2026-09-22 |
| lookups_prefer_later_entries | 重复条目查找后者胜出 | 给定重复 kind 的多条目，当查找，则后条胜出、未配置为 None、未知 provider 无 keychain id | 2026-09-19 |

### crates/gloss-core/src/guard.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| scene_gate_blocks_secure_input_and_listed_apps | 场景闸门的两路拦截与优先级 | 给定安全输入开启、前台应用在内建名单里、两者同时成立、名单外的应用、无事实五种场景，当判闸门，则各返回对应因由（两者同时成立时报安全输入、名单外与无事实放行） | 2026-09-26 |
| entry_matching_covers_every_identity_an_app_can_offer | 名单匹配按身份逐字比对 | 给定内建名单，当经公共入口 `trigger_block` 判闸门，则 bundle id 命中（ASCII 大小写不敏感、返回名单原文）、显示名不命中、名单外的应用与无身份的应用都放行 | 2026-09-26 |
| token_prefixes_are_detected | 各类令牌前缀识别 | 给定 sk-/ghp_/xoxb-/AKIA/Bearer 各形态、带空白与引号包裹的令牌、以及 `NAME=<令牌>` 与 `AUTH=Bearer <令牌>` 两种赋值形态（.env 行、shell 导出与 Authorization 头），当检测，则都命中 Token | 2026-09-26 |
| tokens_embedded_in_structured_text_are_detected | 嵌在结构里或汉字紧贴的令牌照样命中 | 给定 JSON 值、查询串参数、URL 路径段、.env 行、代码围栏包裹的令牌，以及紧贴汉字的令牌（`密钥是sk-…`），当检测，则都命中 Token（前缀判定看的是「前面不是 ASCII 字母数字」，不是词首） | 2026-09-26 |
| short_dummy_keys_are_detected | 手写的短假密钥照样命中 | 给定 sk-123456 / sk-abc123 / sk-XXXXXXXX / sk_live_1234abcd / ghp_1234abcd 这类十几字符内的假密钥、以及被换行截断的令牌，当检测，则都命中 Token（主体 ≥ 4 且含数字或大写即算） | 2026-09-26 |
| invisible_characters_do_not_hide_a_token | 不可见字符藏不住令牌 | 给定紧跟前缀、或嵌在卡号与高熵串里的六类不可见字符（零宽空格/连接符、词连接符、BOM、软连字符），当检测，则 Token / CardNumber / HighEntropy 各自仍命中（判定前从副本里剥掉它们，且发生在所有检测器之前） | 2026-09-26 |
| prose_about_tokens_is_not_a_hit | 谈论令牌的散文不误报 | 给定本仓库文档里描述敏感信息守则的原话、过短的 sk-abc、以及「AKIA 是前缀」这类说明，当检测，则都不命中 | 2026-09-24 |
| hyphenated_words_are_not_mistaken_for_prefixed_tokens | 英文复合词不被误判成令牌 | 给定 disk-space-2024 / risk-managed-portfolio / mask-the-answer / sk-learn-scikit 这类含 `sk-` 的普通复合词，当检测，则都不命中（前缀前面是字母即不算，普通小写主体也不算短档） | 2026-09-26 |
| private_key_blocks_are_detected_before_tokens | 私钥块先于令牌判定 | 给定 PEM 私钥块（含它同时含 sk- 形式串），当检测，则报 PrivateKey（更确定的类别胜出） | 2026-09-24 |
| card_numbers_pass_luhn_only | 卡号只有过 Luhn 才算 | 给定 4111 1111 1111 1111、差一位的变体、全零串、以及「卡号 + 有效期/CVC」与连续分隔符形态（`4111 1111 1111 1111 12/26`、`4111  1111  1111  1111`），当检测，则只有真正过 Luhn 且数字不重复的才命中 CardNumber（组分界处也判一次） | 2026-09-26 |
| high_entropy_strings_are_detected | 高熵长随机串识别 | 给定 32 位以上、含三类字符的高熵串，当检测，则命中 HighEntropy | 2026-09-24 |
| identifiers_and_prose_stay_below_the_entropy_threshold | 标识符与散文不触熵阈值 | 给定长下划线标识符、纯小写长词、中文句子，以及三类字符齐备但分布均匀的长串，当检测，则都不命中——最后那类只可能被熵阈值否掉（含一条熵 4.32 的近界样本），阈值被删或放宽到 4.0 即红 | 2026-09-26 |
| bearer_and_aws_key_ids_keep_their_own_bounds | Bearer 与 AKIA 各守自己的边界 | 给定带 base64 填充符的 AKIA 编号、过短或非大写的 AKIA 串、带连字符的伪编号、以及标准 base64 主体的 Bearer 令牌，当检测，则只有第一类命中 Token——Bearer 支仍用窄字符集，AKIA 只看连续的大写字母与数字 | 2026-09-26 |
| the_long_tier_of_prefixed_bodies_ignores_shape | 前缀令牌长档不看形状 | 给定 13 字符与 16 字符的纯小写复合词，当检测，则短档那次放行、长档那次算令牌——长档只卡长度是写明的取舍，这条用例把边界钉在明面上 | 2026-09-26 |
| detection_prefers_the_more_certain_kind | 检测按确定度排序 | 给定同时可判多类的文本，当检测，则返回更确定的那个类别（私钥 > 令牌 > 卡号 > 高熵；相邻两类各有一对同现的样例） | 2026-09-26 |
| ordinary_text_is_never_flagged | 日常文本一律不拦 | 给定常用句中英文本、中文段落、代码片段、带 `sk-` 的普通英文句与 `let key = "sk-test";` 这类代码，当检测，则全部为 None | 2026-09-26 |
| the_self_flag_is_not_consumed_by_the_sensitive_list | is_self 与敏感名单无关 | 给定 is_self 为真的名单外前台应用，当判闸门，则放行（is_self 只归触发决策的防误触分支，不是名单命中） | 2026-09-30 |

### crates/gloss-core/src/prompt.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| classify_prompt_carries_allowed_kinds_and_the_text | 分类提示词携带允许清单与原文 | 给定允许清单与原文，当 render_classify（双语），则用户消息为原文、系统指令含全部 kind 标识与 kind 契约、不含 Auto、无残留占位符 | 2026-09-30 |
| classify_schema_is_a_neutral_placeholder | 分类契约为中性占位 | 给定分类输出契约（CLASSIFY_SCHEMA），当检查，则不含任何具体 kind 标识（示例值是少样本偏置，写死哪类模型就偏向哪类） | 2026-10-03 |
| body_is_the_first_contract_field_for_every_kind | body 恒为契约首字段 | 给定三种文本 kind 的 schema，当解析 schema JSON，则首键恒为 body（流式渐进提取依赖字段序） | 2026-10-03 |
| rules_follow_the_allowed_list_and_leave_no_dangling_label | 判别规则跟随允许清单 | 给定含/不含 ExplainCode、以及全无规则的清单，当 render_classify，则命令行等边界规则只在对应 kind 在清单里时出现；清单里没有带规则的 kind 时整段（含标签）消失、无残留占位符 | 2026-09-30 |
| text_kinds_render_system_and_user_with_kind_content | 文本 kind 渲染两段消息 | 给定三个文本 kind，当渲染，则得 [System, User] 两段，系统指令含各自关键词与结构化契约围栏，用户消息为原文 | 2026-09-19 |
| structured_contract_matches_outcome_schema | 结构化契约与 schema 对齐 | 给定词卡与代码解释模板，当检查系统指令，则分别声明 senses/phonetic 与 title 字段 | 2026-09-19 |
| missing_target_lang_defaults_to_chinese | 目标语言缺省中文 | 给定未设 target_lang，当渲染句译，则系统指令含「中文」 | 2026-09-19 |
| explicit_target_lang_is_rendered | 显式目标语言渲染 | 给定 Lang::Ja，当渲染，则系统指令含「日语」 | 2026-09-19 |
| empty_target_language_name_falls_back_to_default | 空目标语言名回落缺省 | 给定 target_lang = Other("") / Other(" ") 的两个 locale，当渲染，则目标语言回落缺省中文（不吞掉整条指令行）且契约围栏仍在 | 2026-09-22 |
| hint_is_injected_and_defaults_to_nothing | hint 注入与缺省不注入 | 给定 CodeLanguage hint，当渲染，则注入系统与用户两处；无 hint 则两处均无注入行、用户消息为原文 | 2026-09-19 |
| source_lang_hint_is_injected | 源语言 hint 注入 | 给定 SourceLang(法语)，当渲染，则系统指令含「源语言：法语」 | 2026-09-19 |
| image_kinds_are_placeholders_until_m5 | 图像 kind 占位拒绝 | 给定图像 kind 的图像任务，当渲染，则报 UnsupportedModality | 2026-09-19 |
| modality_mismatch_is_rejected_before_rendering | 模态错配在渲染前拒绝 | 给定图像 kind 配文本输入，当渲染，则先被模态约束拒绝 | 2026-09-19 |
| messages_serialize_to_openai_shape | 消息序列化为 OpenAI 形态 | 给定 ChatMessage，当序列化，则得 {"role","content"} 的 OpenAI 形态 | 2026-09-19 |
| render_template_substitutes_placeholders | 占位符显式替换 | 给定含 {{占位符}} 的模板，当渲染，则按值替换、同名占位符可重复；未声明与未闭合的占位符原样保留（留待完整性测试抓） | 2026-09-22 |
| render_template_drops_lines_whose_placeholders_are_empty | 占位符全空的行整行消失 | 给定提示行与指令行模板，当某行占位符值全为空，则该行连同行内静态文字与换行一并删除，其余行不受影响 | 2026-09-22 |
| render_template_keeps_blank_lines_without_placeholders | 无占位符空行是结构 | 给定模板里的空行（不含占位符），当渲染，则原样保留 | 2026-09-22 |
| every_template_placeholder_is_declared | 模板原文占位符白名单 | 给定两个 locale 的五个模板原文，当扫占位符，则括号配对且名字都在已声明集合内（未声明者若与已声明占位符同行会被整行删掉，产物断言看不见） | 2026-09-22 |
| every_locale_renders_without_leftover_placeholders | 全 locale 无残留占位符 | 给定两个 locale × 三个文本 kind × 三种 hint 组合，当渲染，则两段消息都不含 {{ 且系统指令带结构化契约围栏 | 2026-09-22 |
| english_locale_renders_english_prompts | 英文 locale 出英文指令 | 给定 En locale 与 SourceLang(Fr) hint，当渲染，则系统指令与提示行标签均为英文、用户消息同样带英文提示行、且不含中文指令词 | 2026-09-22 |
| prompt_locale_is_independent_of_target_language | 模板语言与目标语言解耦 | 给定 En locale 未设目标语言、Zh locale 设 Lang::En，当渲染，则前者英文模板下默认目标仍是中文、后者中文模板含「英语」 | 2026-09-22 |
| prompt_locale_defaults_to_chinese | 模板语言缺省中文 | 给定未设 prompt_locale 的任务，当渲染，则结果与显式 Zh 逐字一致且含「词典助手」 | 2026-09-22 |

### crates/gloss-core/src/config_handle.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| load_takes_first_snapshot_from_store | load 的首份快照来源 | 给定存储已有配置，当 ConfigHandle::load，则首份快照即存储内容 | 2026-09-19 |
| save_writes_through_and_swaps_snapshot | save 穿透落盘并换快照 | 给定一次 save，当执行，则磁盘文档与运行时快照同时为新版 | 2026-09-19 |
| failed_save_keeps_previous_snapshot | 失败保存保持旧快照 | 给定落盘必失败的存储，当 save，则错误上抛且快照保持旧版 | 2026-09-19 |
| load_or_default_uses_store_snapshot_or_falls_back | load_or_default 的两路 | 给定健康与损坏两种存储，当 load_or_default，则分别取存储快照与出厂默认 | 2026-09-19 |
| load_propagates_store_failure | 加载失败原样上抛 | 给定加载失败，当 ConfigHandle::load，则 Config 错误原样上抛 | 2026-09-19 |
| saved_version_is_visible_from_another_thread | 保存结果跨线程可见 | 给定另一线程的读者，当 save 完成、信号放行后读取，则必见新版本 | 2026-09-19 |
| concurrent_readers_never_see_a_mixed_version | 并发读者不见混合版本 | 给定 4 读者与 200 轮 A/B 交替保存，当读者全程校验，则每份快照都是完整版本且末次保存胜出 | 2026-09-19 |
| concurrent_saves_keep_disk_and_snapshot_in_step | 并发保存盘与快照同步 | 给定 4 写者并发各存 50 次，当全部结束，则磁盘与内存快照同版 | 2026-09-19 |
| base_url_validation_rejects_structural_problems | Base URL 结构性拒绝 | 给定空串/非 https/缺 scheme/缺 host/内嵌凭据/带 query 或 fragment/中部空白的地址，当 validate_base_url，则按类别返回 Err | 2026-09-22 |
| base_url_validation_accepts_legal_addresses | Base URL 合法放行 | 给定 https 地址（含首尾空白、无路径、带端口），当 validate_base_url，则 Ok | 2026-09-22 |
| missing_fields_fall_back_to_defaults_on_deserialize | 缺字段回落出厂默认 | 给定缺 language 字段的配置 JSON，当反序列化，则 language 按出厂跟随系统补齐 | 2026-09-22 |

### crates/gloss-core/src/task.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| task_input_carries_text_and_hint | 文本输入携带 hint | 给定文本与 hint，当构造 TaskInput::Text，则两者正确携带 | 2026-09-19 |
| task_binds_kind_input_and_options | Task 三元正确绑定 | 给定构造参数，当建 Task，则 kind/input/options 正确绑定 | 2026-09-19 |
| task_options_default_carries_the_factory_model | 选项缺省携带出厂模型 | 给定 TaskOptions::default()，当检查，则 model 为 DEFAULT_TEXT_MODEL、target_lang/prompt_locale 为 None（缺省只服务测试直构） | 2026-10-03 |
| image_input_shares_png_bytes_via_arc | 图像输入 Arc 共享 | 给定 PNG 字节，当构造 Image 输入，则经 Arc 共享（ptr_eq）并携带区域 | 2026-09-19 |
| outcome_carries_structured_variants | 词卡结构化字段携带 | 给定 WordCard 变体，当构造 TaskOutcome，则 word 字段完整携带 | 2026-09-19 |
| modality_matrix_is_enforced_cell_by_cell | 模态矩阵逐格校验 | 给定 6 kind（含 Auto）× 3 输入全矩阵，当逐格 validate，则文本列（含 Auto）与 Image 列合法、Audio 全列非法 | 2026-09-26 |
| task_round_trips_through_serde | Task serde 往返无损 | 给定整条 Task（含 hint 与目标语言），当 serde 往返，则无损 | 2026-09-19 |
| image_input_round_trips_through_serde | 图像输入 serde 往返 | 给定 Image 输入，当 serde 往返，则字节与区域不变且反序列化得到新 Arc（不与原值共享） | 2026-09-19 |

### crates/gloss-core/src/model.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| screen_rect_compares_by_value | ScreenRect 按值比较 | 给定同值不同实例的 ScreenRect，当比较，则按值相等、异值不等 | 2026-09-19 |
| error_display_is_diagnostic_text | 错误文案为诊断文本 | 给定各 GlossError 变体，当 to_string，则输出精确的诊断文案 | 2026-09-19 |
| error_is_std_error | 错误可作 std Error | 给定 GlossError 装箱为 std Error，当 to_string，则输出正确文案 | 2026-09-19 |

### crates/gloss-core/src/engine.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| fence_fallback_pairs_with_the_raw_stream | 围栏 fallback 与原始流的契约配对 | 给定含围栏的原始回复，当 complete 的 fallback 层（finalize_outcome）解析，则产出 kind 正确、围栏从正文剥离、词卡结构化字段完整回填 | 2026-10-03 |
| json_main_path_tolerates_null_and_missing_fields | JSON 主路径容忍 null 与缺字段 | 给定 "phonetic":null 或 senses 缺失的契约 JSON，当 complete，则 phonetic 为 None、senses 落空表（字段级按契约回退） | 2026-10-03 |
| json_main_path_skips_bad_sense_entries | 坏词条跳过不致命 | 给定含缺字段坏条目的 senses，当 JSON 主路径解析，则两条好条目保留、坏条目跳过 | 2026-10-03 |
| fence_fallback_keeps_the_rfind_semantics | 围栏 fallback 保留 rfind 语义 | 给定围栏后尾随文字/多围栏/坏围栏/缺 senses 的四种输入，当 parse_structured，则取最后一个围栏、尾随文字不进正文、残片无损保留、kind 兜底接住 | 2026-10-03 |
| non_json_reply_falls_through_to_the_fence_fallback | 非 JSON 回复逐层退让 | 给定非 JSON 的原始回复，当 complete，则 JSON 主路径失败、围栏 fallback 接住（无围栏时正文原样、结构化为无标题 Plain） | 2026-10-03 |
| engine_failures_propagate_and_earlier_chunks_are_kept | 引擎失败原样上抛 | 给定 execute 整体失败与流中 Err，当 run，则错误原样上抛（分类段失败与任务段失败同规）且失败前的增量已转发 | 2026-10-03 |
| hint_passthrough_classifies_without_a_round_trip | 提示直通零往返 | 给定 CodeLanguage hint 输入，当 run，则 ExplainCode 直接定型、on_classified 恰发一次、引擎仅任务执行 1 次调用 | 2026-10-03 |
| classified_kind_arrives_before_any_chunk | 分类先于任何增量 | 给定无 hint 输入与两段任务流，当 run，则 on_classified 恰在首个 on_chunk 之前触发一次 | 2026-10-03 |
| classify_failure_falls_back_and_the_task_still_runs | 分类失败兜底后任务照跑（engine） | 给定分类调用失败（一次性错误）与任务脚本，当 service.run，则落 CLASSIFY_FALLBACK、引擎共 2 次调用、fallback warn 不含选区原文 | 2026-10-03 |
| raw_text_is_returned_verbatim | 原始文本原样返回 | 给定两段 JSON 流式脚本，当 run，则 RunOutput.body 为未解析的原始拼接文本 | 2026-10-03 |
| blank_model_is_a_config_failure_before_the_engine | 空白模型先于引擎拒绝 | 给定 model 为空白的选项，当 run，则 Config 错误且引擎 0 调用 | 2026-10-03 |
| non_text_input_is_rejected_before_anything | 非文本输入在一切之前拒绝 | 给定 Audio/图像输入，当 run，则 UnsupportedModality 且引擎 0 调用 | 2026-10-03 |

### crates/gloss-app/src/cache.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| different_inputs_get_different_keys | 不同输入必不同 key | 给定不同文本/带 hint 文本/音频模态的输入，当 cache_key(input, options)，则 key 互不相同 | 2026-10-03 |
| different_options_get_different_keys | 不同选项必不同 key | 给定同一输入，当换 target_lang/prompt_locale/model（非出厂值），则 key 均不同 | 2026-10-03 |
| key_derivation_is_stable_and_serialization_failure_falls_back | key 派生稳定、序列化失败兜底 | 给定同输入重复派生与含 NaN 的 Audio 输入（序列化失败），当 cache_key，则同值同 key、回退路径确定性且与正常值可分辨 | 2026-10-03 |
| entries_store_and_isolate_outcomes | 条目存取与 key 隔离 | 给定 miss→set→hit 序列，当按不同 key 查询，则命中且无关 key 互不可见 | 2026-10-03 |
| ttl_expiry_takes_effect | TTL 过期生效 | 给定 TTL 60ms 的条目，当过 120ms 并 run_pending_tasks 后读，则 miss | 2026-10-03 |
| factory_ttl_matches_the_config_default | 默认 TTL 单点一致 | 给定出厂 TTL（Config::cache_ttl_secs），当与 gloss_app::cache 的 DEFAULT_TTL 比对，则相等 | 2026-10-03 |

### crates/gloss-app/src/finalize.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| json_main_path_builds_the_word_card | JSON 主路径出词卡 | 给定契约 JSON（body + word/phonetic/senses），当 complete，则 body 原样、词卡结构化字段完整回填 | 2026-10-03 |
| json_main_path_tolerates_null_and_missing_fields | JSON 主路径容忍 null 与缺字段 | 给定 "phonetic":null 或 senses 缺失的契约 JSON，当 complete，则 phonetic 为 None、senses 落空表（字段级按契约回退） | 2026-10-03 |
| json_main_path_skips_bad_sense_entries | 坏词条跳过不致命 | 给定含缺字段坏条目的 senses，当 JSON 主路径解析，则两条好条目保留、坏条目跳过 | 2026-10-03 |
| json_main_path_covers_plain_and_extracted_kinds | JSON 主路径覆盖 Plain 与提取 | 给定句译/代码（title）与 OCR（text）契约 JSON，当 complete，则 title 与 text 各按 kind 落结构化、body 保持 markdown | 2026-10-03 |
| missing_body_field_hands_over_to_the_fence_fallback | 缺 body 交围栏 fallback | 给定 body 缺失但带旧围栏的回复，当 complete，则 JSON 主路径整路失败、围栏 fallback 接住旧契约输出 | 2026-10-03 |
| non_json_reply_falls_through_to_the_fence_fallback | 非 JSON 回复逐层退让 | 给定非 JSON 的原始回复，当 complete，则 JSON 主路径失败、围栏 fallback 接住（无围栏时正文原样、结构化为无标题 Plain） | 2026-10-03 |
| fence_fallback_pairs_with_the_raw_stream | 围栏 fallback 与原始流的契约配对 | 给定含围栏的原始回复，当 complete 的 fallback 层（finalize_outcome）解析，则产出 kind 正确、围栏从正文剥离、词卡结构化字段完整回填 | 2026-10-03 |
| fence_fallback_keeps_the_rfind_semantics | 围栏 fallback 保留 rfind 语义 | 给定围栏后尾随文字/多围栏/坏围栏/缺 senses 的四种输入，当 parse_structured，则取最后一个围栏、尾随文字不进正文、残片无损保留、kind 兜底接住 | 2026-10-03 |
| both_layers_agree_on_the_two_layer_handoff | 两层衔接各就各位 | 给定坏 JSON + 坏围栏的 OCR 回复，当 complete，则一路退到 kind 兜底、全文无损保留 | 2026-10-03 |

### crates/gloss-app/src/channel.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| platform_events_round_trip_through_crossbeam | 通道①事件往返 | 给定五种 PlatformEvent，当经通道①往返，则按序原样到达 | 2026-09-19 |
| acquire_commands_carry_app_assigned_gen | 取材命令携带代数 | 给定带代数的取材命令，当下发，则接收侧读到同一代数与 kind/区域 | 2026-09-19 |
| run_task_command_delivers_cancellable_task | 任务命令携带取消令牌 | 给定 RunTask 命令，当下发，则代数、任务、取消令牌完整到达且令牌联动 | 2026-09-19 |
| event_channel_supports_dual_senders | 事件通道双发送端 | 给定克隆出的第二 Sender，当两端各发一条，则主线程按到达顺序都收到 | 2026-09-19 |
| event_channel_carries_stream_chunks_and_failures | 通道④携带流块与失败 | 给定 TaskChunk 与 TaskFailed，当经通道④，则按序携带 | 2026-09-19 |
| try_recv_on_empty_queue_returns_empty | 空队列 try_recv 返回 Empty | 给定空队列，当 try_recv，则返回 Empty | 2026-09-19 |
| try_recv_after_senders_dropped_reports_disconnected | 发送端全关报 Disconnected | 给定全部 Sender drop，当先收完余量再 try_recv，则报 Disconnected | 2026-09-19 |
| channels_bundles_all_four | 四通道捆绑互不串扰 | 给定 Channels::new，当四条通道各发一条，则各自到达、互不串扰 | 2026-09-19 |
| event_sender_clone_is_independent | 克隆 Sender 独立存活 | 给定克隆 Sender 且原型 drop，当用它发送，则消息照常到达 | 2026-09-19 |

### crates/gloss-app/src/app/mod.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| ui_locale_follows_the_saved_language_without_a_restart | 界面语言随保存即时切换 | 给定出厂配置（跟随系统），当保存 Language::En 再保存 Language::System，则逐帧解析出的 locale 依次为 Zh→En→Zh（不重启即换文案表） | 2026-09-22 |

### crates/gloss-app/src/app/render.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| repaint_delay_max_means_no_wakeup | MAX 延迟不唤醒 | 给定 Duration::MAX 延迟，当换算唤醒时刻，则 None | 2026-09-19 |
| repaint_delay_becomes_a_deadline | 延迟换算为截止时刻 | 给定 250ms/0 延迟，当换算，则 now+延迟/now | 2026-09-19 |

### crates/gloss-app/src/app/handler.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| escape_press_is_the_dismiss_key | 浮层收起键判定 | 给定逻辑键与按下状态，当判定收起键，则 Escape 按下为真、释放与其它字符键为假 | 2026-09-21 |
| sooner_picks_the_earliest_deadline | 取更早的截止时刻 | 给定两个时刻（可含 None），当取更早，则 None 让位、双 None 不唤醒 | 2026-09-19 |

### crates/gloss-app/src/app/channels.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| late_events_of_superseded_trigger_do_not_bleed | 旧会话的迟到事件不渗漏 | 给定划词提交 A 在推理中、再给一次新划词探测，当探测期间 A 的流照常推进，随后提交 B，则提交才取消 A、A 的迟到 chunk/TaskDone 被陈旧过滤、B 的产物照常 Show | 2026-10-01 |
| probe_empty_selection_is_silently_dropped | 壳层时序：误滑静默丢弃 | 给定划词探测（状态机不动），当收空选区失败，则壳不请求浮层、状态机与视图原样、探测消费 | 2026-10-03 |
| failed_task_lands_in_error_and_retry_works | 失败落错误态且可再划词 | 给定推理中任务，当匹配代数的失败到达，则落 Error；陈旧失败丢弃；再次划词探测不动失败卡（探测编号在场） | 2026-10-01 |
| stale_input_ready_is_dropped_entirely | 陈旧 InputReady 整体丢弃 | 给定陈旧编号 InputReady，当采纳，则整体丢弃、不下发通道③ | 2026-09-19 |
| saved_config_applies_to_the_next_trigger | 新配置对下次触发生效 | 给定保存新配置（目标语言与模型），当下一次划词提交，则新值随任务选项冻结下发（模型与语言均出自探测时快照；kind 由 LLM 层分类） | 2026-10-03 |
| saved_config_does_not_leak_into_the_inflight_task | 在途任务用触发时快照 | 给定探测后、产物到达前保存新配置，当产物提交下发，则仍用探测时快照 | 2026-09-19 |
| dispatched_acquire_carries_the_task_span | 取材命令带着任务 span 下发 | 给定划词触发，当取出通道②载荷并进入它的 span，则探针日志行是 JSON 且带 "generation":1 | 2026-09-23 |
| a_disabled_default_kind_does_not_stop_the_selection_gesture | 任务开关不拦划词手势 | 给定默认任务被停用的配置，当划词触发，则仍下发 Auto 取材命令（手势不带显式意图，开关只拦显式 kind） | 2026-09-26 |
| a_sensitive_scene_makes_the_selection_gesture_a_no_op | 敏感场景下划词彻底无声 | 给定安全输入开启、再给定前台应用在拦截名单内（两侧各自设置），当划词触发，则取材命令都不下发、探测编号不领、状态留 Idle；场景恢复后同一手势照常下发 | 2026-09-24 |
| suspicious_input_is_suppressed_and_shows_nothing | 可疑内容被拦下且什么都不出 | 给定嵌在 JSON 里的短令牌取材产物，当采纳，则通道③一条都没有、可见会话不被触碰（没有卡片、没有可点的出口）；下一次普通取材照常下发 | 2026-10-01 |
| a_mis_slide_over_a_visible_session_preserves_it_entirely | 已显示会话对误滑零感知 | 给定推理中的可见会话，当新划词探测以空选区失败收场，则令牌未取消、状态与视图原样、无显形挂起、流式正文照常追加 | 2026-10-01 |
| committing_the_probe_flags_the_reveal_for_the_same_frame | 提交即挂起显形 | 给定在途划词探测，当产物提交，则置位显形挂起（drain_events 同帧消费）、进入 Translating | 2026-10-01 |

### crates/gloss-app/src/i18n.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| both_locale_files_declare_the_same_keys | 两份文案表键集合一致 | 给定 zh.toml 与 en.toml，当递归收集叶子键路径，则两份逐条一致且整表条数为 76（条数钉住，防遍历退化） | 2026-09-24 |
| every_entry_is_translated_in_the_english_catalog | 英文表逐条真译不照抄 | 给定两份文案表的全部词条，当逐条比对取值，则除语言自身名（gloss_ui_language_en）外无一与中文表逐字相同 | 2026-09-23 |
| entries_are_written_fully_qualified | 词条键写成下划线全限定名 | 给定两份文案表的每一行非注释行，当解析键名，则键一律以 gloss_ 开头且不含点号（前缀落在每一行、无节头；退回节头或点号连接即红） | 2026-09-23 |
| placeholders_match_across_locales | 占位符名两语言一一对应 | 给定两份文案表，当逐条比对词条里的 {{占位符}} 名集合，则两语言一致（拼错名不会单边漏改），且带占位符的词条恰为 9 条（逐条列名，新增模板漏登记即红） | 2026-09-24 |
| catalogs_parse_into_typed_fields | 文案表解析进类型化字段 | 给定编译期嵌入的两份文件，当取用，则解析成功且两语言取值可区分 | 2026-09-22 |
| fill_replaces_every_named_placeholder | 占位符按名填充 | 给定含同名多处的模板与无参数/无对应参数的模板，当填充，则同名全替换、无占位符原样、无参数占位符原样保留 | 2026-09-22 |
| error_text_maps_every_variant_per_locale | 错误文案按变体覆盖两语言 | 给定 GlossError 的十个变体（含两个带诊断文本的），当取失败卡文案，则各自的两种语言都非空且互不相同 | 2026-09-22 |
| each_error_variant_maps_to_its_own_entry | 错误变体各取自己的词条 | 给定八个无诊断文本的变体，当取失败卡文案与错因细节，则各等于本变体对应的词条（两臂对调会被抓住）；带诊断的两个变体按模板填诊断 | 2026-09-22 |
| error_detail_prefers_the_variant_diagnostic | 复合提示取诊断细节 | 给定带诊断的变体与不带诊断的变体，当取错因细节，则前者只出诊断原文、后者回落本地化整句 | 2026-09-22 |

### crates/gloss-app/src/windows.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| position_inside_the_monitor_is_untouched | 屏内位置原样保留 | 给定显示器范围内的位置，当钳制，则原样返回 | 2026-09-20 |
| position_past_the_right_or_bottom_edge_pulls_back | 右下越界向内收 | 给定越出右/下边缘的位置，当钳制，则收到屏宽/高减浮层尺寸处 | 2026-09-20 |
| position_before_the_origin_clamps_to_it | 负坐标钳到屏原点 | 给定负全局坐标（主屏左侧显示器），当钳制，则收到该显示器原点而非主屏 | 2026-09-20 |
| clamping_respects_the_monitor_origin_on_secondary_displays | 副屏钳制按全局原点 | 给定副屏（全局原点非零）右缘位置，当钳制，则按副屏全局区间收口而非主屏 | 2026-09-20 |
| monitor_smaller_than_the_overlay_pins_to_the_origin | 显示器小于浮层贴原点 | 给定比浮层还小的显示器与屏内位置，当钳制，则贴显示器原点（上限取 0） | 2026-09-20 |
| height_caps_at_half_the_screen_minus_the_margin | 高度上限为半屏减余量 | 给定逻辑高 1080 的显示器与超限请求，当 capped_height，则上限取一半屏高减工作区余量（444） | 2026-10-02 |
| smaller_requests_pass_through_untouched | 未超限的高度原样保留 | 给定远小于上限的请求，当 capped_height，则原样返回 | 2026-10-02 |
| short_screens_bottom_out_at_the_default_height | 矮屏回落默认高度 | 给定半屏扣余量后仍低于默认高度的显示器，当 capped_height，则下限取默认高度 200 | 2026-10-02 |
| width_is_locked_to_the_current_tier | 流式锁宽 | 给定当前档与更高宽度档的请求，当流式防抖，则宽度保持当前档、高度量化到步长倍数 | 2026-09-26 |
| height_steps_up_in_quantized_increments | 步进增高 | 给定略高于当前档的高度请求，当流式防抖，则上升一个完整步长 | 2026-09-26 |
| height_never_shrinks_during_streaming | 流式不回缩 | 给定明显小于当前档的请求，当流式防抖，则保持当前高度（回缩留给完成态精确重排） | 2026-09-26 |
| current_size_quantizes_up_to_the_step_boundary | 首帧落在步长边界 | 给定与当前相等的请求，当流式防抖，则高度量化到当前之上的步长边界、宽度不变 | 2026-09-26 |

### crates/gloss-app/src/ui/popup.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| width_hysteresis_does_not_oscillate_between_frames | 宽度滞回不振荡 | 给定上一帧宽度与内容高，当决策宽度，则长内容加宽、带内保持原档、明显变矮才收回 | 2026-09-20 |
| width_hysteresis_band_bounds_are_symmetric | 滞回阈值边界对称 | 给定阈值附近的内容高，当按当前档决策，则过加宽阈值才加宽、过收回阈值才收回 | 2026-09-20 |
| stream_body_extracts_the_json_body_progressively | 流式正文按 JSON body 渐进提取 | 给定完整 JSON/空串/非 JSON/部分键/未闭合值/外键在前/转义（引号反斜杠 unicode）/残缺转义/旧围栏契约九类原始流，当 stream_body，则反转义前缀渐进可见、残缺序列留待下帧、无 body 键恒空（进度态） | 2026-10-03 |
| decode_app_icon_rejects_bad_bytes | 图标解码失败隔离降级 | 给定非 PNG 字节，当解码应用图标，则返回 None（页头退化为无图标行，不 panic） | 2026-09-30 |
| decode_app_icon_crops_to_the_content_square | 图标按画布比例裁本体 | 给定内嵌的 Dock 图标 PNG，当解码裁剪，则得 206×206 的图形本体（256 按 100/824/1024 画布比例裁去透明边距） | 2026-09-30 |
| example_lines_split_at_the_first_cjk_glyph | 例句在首个 CJK 字形处拆两行 | 给定「英译+中译」/纯英文/开头即 CJK/开头即 CJK 的例句四种输入，当 example_lines，则英汉混合的拆出原文与译文两行、其余原样单行 | 2026-10-01 |
| watermark_is_the_untranslated_brand_name | 页脚水印是品牌名 | 给定水印取值入口，当取值，则恒为 "Gloss"（品牌名不翻译） | 2026-10-01 |
| code_views_expose_the_language_badge_and_prose_untouched | 代码视图语言标签入树、非代码无标签 | 给定 ExplainCode 流式视图，当渲染，则无障碍树可检索语言标签 rust（面板首行左上、原样小写）；给定翻译流式视图，当渲染，则树上无标签（未知/非代码不显示） | 2026-10-02 |
| snapshots_match_baseline（popup 代码视图随高亮再录） | 两份代码视图基线随语法着色重录 | 给定 popup_code_streaming / popup_code_outcome 的英文夹具，当 wgpu 渲染并 diff，则与基线一致（2026-10-02 随 T4 单趟正则六类着色再录：关键字/函数形/字符串着色；其余七份 popup 与 settings 基线零变化——着色只落代码分支） | 2026-10-02 |

### crates/gloss-app/src/app/overlay.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| retry_action_redispatches_the_failed_task | Retry 动作重发失败任务 | 给定失败卡 Retry 动作，当执行，则同代数同任务新令牌重发通道③，重试产物照常采纳 | 2026-09-21 |
| open_settings_action_keeps_the_error_card | 打开设置保留错误卡 | 给定鉴权失败卡，当执行 OpenSettings 动作，则停在 Error、通道③无流量、编辑会话就位 | 2026-09-21 |
| dismiss_abandons_the_inflight_task_and_returns_to_idle | 收起浮层放弃在途任务 | 给定推理中的浮层，当执行收起出口，则回 Idle、在途令牌取消、视图清空、迟到产物被丢弃 | 2026-09-21 |
| auto_show_policy_decides_when_the_overlay_pops | 自动弹出按事件类别 | 给定事件类别×采纳组合，当逐事件判定，则失败即弹；取材成功不再露面（显形随划词提交置位）、完成与 chunk 不弹（浮层已可见）；未采纳一律不弹 | 2026-09-26 |
| auto_show_survives_a_mixed_batch | 混合批次自动弹出取或 | 给定一批混合回传，当按批取或，则任一被采纳的失败即弹、整批陈旧不弹、空批不弹 | 2026-09-26 |
| pending_reveal_shows_only_over_a_live_view | 挂起显形的守卫只认活视图 | 给定挂起的显形请求与机器视图有无两种状态，当判显形，则视图在场即显形、视图为空不显形（弹出渲染自检卡）、无挂起且批次无失败即弹也不显形 | 2026-10-03 |
| reveal_decision_combines_the_pending_request_with_the_batch | 显形决策对挂起请求与批次取或 | 给定挂起显形请求配整批陈旧回传、无挂起配被采纳的失败、无挂起配取材成功与流式增量，当判显形，则挂起显形不依赖批次（陈旧批也拦不下）、失败即弹独立成立、取材成功与流式增量不负责露面 | 2026-09-27 |
| show_position_follows_selection_only_for_the_current_generation | 显示位置跟随当代划词 | 给定划词锚点与代数，当决策显示位置，则当代跟随选区（右下偏移）、代数不符或无锚点回落居中 | 2026-09-20 |
| selection_trigger_records_its_anchor_per_generation | 划词触发按代数记锚点 | 给定两次划词触发，当消费，则锚点随代数刷新为各自释放坐标 | 2026-09-20 |

### crates/gloss-app/src/app/settings_session.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| open_settings_request_starts_an_edit_session | 打开设置即编辑会话 | 给定 OpenSettingsRequested，当消费，则编辑会话打开、草稿=当前快照、不占代数 | 2026-09-19 |
| settings_save_writes_keychain_and_swaps_config | 保存写密钥串并换配置 | 给定含密钥替换的保存，当成功，则密钥进 keychain、快照换新、会话关闭，新配置随后续触发生效 | 2026-09-26 |
| clearing_the_key_deletes_the_secret_on_save | 清除密钥保存即删除 | 给定 KeyUpdate::Clear，当保存，则 keychain 条目删除、会话关闭 | 2026-09-19 |
| failed_save_keeps_the_session_open_with_a_notice | 失败保存会话不关 | 给定落盘必失败存储，当保存，则会话保持打开、错误进提示、快照不变 | 2026-09-19 |

### crates/gloss-app/src/app/theme.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| apply_theme_elides_writes_until_the_preference_changes | 主题未变不重写 | 给定未变的偏好，当重复 apply_theme，则不写；偏好变了则跟上 | 2026-09-23 |

### crates/gloss-app/src/ui/code_hl.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| normalize_language_collapses_aliases_to_canonical_names | 语言名归一化收拢别名 | 给定 rs/py/TS/golang/c++ 与规范名、清单外名，当归一化，则别名折叠到规范名（rust/python/typescript/go/cpp）、规范名原样、未知名保留小写原形 | 2026-10-02 |
| normalize_language_rejects_blank_names | 空白语言名无语言 | 给定空串与全空白名，当归一化，则恒为 None | 2026-10-02 |
| detect_language_reads_shebang_interpreters | shebang 解释器映射语言 | 给定 python3/bash/ruby/node 的 shebang 脚本与未知解释器，当内容探测，则各映射到 python/bash/ruby/javascript、未知解释器无语言 | 2026-10-02 |
| detect_language_reads_markup_declarations | 标记语言显式声明 | 给定 DOCTYPE html、<?xml、<?php 开头的文本，当内容探测，则各判为 html/xml/php | 2026-10-02 |
| detect_language_reads_language_signatures | 语言签名形探测 | 给定 fn main、package main+func main、func main、def 、import 开头的代码，当内容探测，则各判为 rust/go/python（go 的 package 子句胜 func 签名、裸 import 兜底 python） | 2026-10-02 |
| detect_language_matches_signatures_only_at_line_starts | 签名形只在行首命中 | 给定行中段含 "def" 的普通句子，当内容探测，则无语言（弱签名不做子串匹配） | 2026-10-02 |
| detect_language_yields_none_for_plain_text | 普通文本无语言 | 给定空串、英文句子与中文句子，当内容探测，则恒为 None | 2026-10-02 |
| detect_language_reads_sql_shapes | SQL 形状探测 | 给定 SELECT...FROM 跨行对（大小写各一）、 lone SELECT、读起来像该对的单行散文，当内容探测，则跨行对判为 sql（大小写不敏感）、孤 SELECT 与单行散文不判（防散文误报） | 2026-10-02 |
| known_languages_color_keywords_and_shapes | 已知语言的关键字与形状着色 | 给定 rust 与 python 代码片段，当 tokenize，则 fn/def/return 落 Keyword、函数调用形落 Function 且区间从标识符起（定界符裁掉） | 2026-10-02 |
| unknown_languages_still_color_strings_and_comments | 清单外语言走通用启发集 | 给定清单外语言（fortran）的字符串与行注释代码，当 tokenize，则字符串与注释仍着色（空输入无 token） | 2026-10-02 |
| missing_language_falls_to_the_generic_set | 无语言提示落通用集 | 给定无 hint 的含字符串与注释代码，当 tokenize，则字符串与注释照常着色 | 2026-10-02 |
| json_keys_specialize_ahead_of_strings | JSON 键特化先于字符串 | 给定 {"key": 1}，当按 json 与无语言各 tokenize，则 json 下冒号前字符串落 Type（键）、通用集下同段落 String | 2026-10-02 |
| markup_tags_and_attributes_specialize | 标记语言标签与属性特化 | 给定 HTML 片段，当 tokenize，则标签名落 Keyword、属性名（等号前）落 Function | 2026-10-02 |
| sql_keywords_are_case_insensitive | SQL 关键字大小写不敏感 | 给定全小写 select/from/where 查询，当 tokenize，则三个关键字全部着色 | 2026-10-02 |
| block_comments_span_lines_and_triples_span_lines | 块注释与三引号跨行 | 给定跨行块注释的 rust 代码与三引号字符串的 python 代码，当 tokenize，则各自为单个跨行 token | 2026-10-02 |
| hex_numbers_color_whole | 十六进制字面量整体着色 | 给定含 0xFF_00 的 rust 代码，当 tokenize，则十六进制字面量为单个 Number token | 2026-10-02 |
| every_class_has_its_own_color_per_theme | 六类色明暗两套互异 | 给定六类别与明暗两主题，当取色，则同主题内六色两两不同、注释恒斜体 | 2026-10-02 |
| js_template_strings_color_with_the_backtick_ruleset | JS 别名落到反引号规则集 | 给定含反引号模板串的 js 代码，当按 "js" tokenize，则 const 为关键字、模板串为字符串（别名表查到的是带反引号的条目） | 2026-10-02 |
| keywords_with_non_word_edges_still_color | 非词边缘关键字着色 | 给定 objc @interface、ruby defined?、clojure set! 的代码，当 tokenize，则各自关键字着色（\b 不存在于 @ 前、?/! 后，裸匹配） | 2026-10-02 |
| toml_section_headers_color_on_every_line | TOML 节头任意行着色 | 给定首行与第四行各一个节头的 TOML，当 tokenize，则两个节头都落 Function（多行锚定） | 2026-10-02 |
| sql_detection_ignores_non_statement_lines | SQL 探测忽略非语句行 | 给定注释里含 select...from 对的 rust 代码、selected/fromage 子串散文，当内容探测，则前者 rust、后者无语言（行锚定 + 词边界） | 2026-10-02 |
| css_hex_colors_color_as_numbers_not_selectors | CSS 十六进制色值归数字 | 给定含 #fff 短色值与 #wrap id 选择器的 CSS，当 tokenize，则色值落 Number（色值组先于选择器组） | 2026-10-02 |

### crates/gloss-app/src/ui/context.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| theme_preference_covers_every_variant | 主题三档全映射 | 给定三档主题，当映射 egui 偏好，则一一对应且出厂跟随系统 | 2026-09-23 |
| new_context_carries_fonts_and_theme | 新上下文两样都装好 | 给定主题，当新建上下文，则主题偏好落上且字体表含 CJK 后备（依赖宿主机字体） | 2026-09-23 |
| reapply_writes_every_context | 重施加写满每个上下文 | 给定两个已装好的上下文，当施加各档主题，则每个都被写；空集写 0 个不 panic | 2026-09-23 |

### crates/gloss-app/src/gpu.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| alpha_mode_prefers_premultiplied | alpha 模式优先 PreMultiplied | 给定含 PreMultiplied 的候选，当挑选，则选它 | 2026-09-19 |
| alpha_mode_falls_back_to_postmultiplied | alpha 模式回退 PostMultiplied | 给定缺 PreMultiplied 的候选，当挑选，则回退 PostMultiplied | 2026-09-19 |
| alpha_mode_degrades_to_auto_without_transparency | 无透明支持降级 Auto | 给定仅 Opaque/Auto 的候选或空列表，当挑选，则降级 Auto | 2026-09-19 |
| errors_describe_their_cause | GPU 错误文案含根因 | 给定各 GpuError 变体，当 to_string，则文案包含根因 | 2026-09-19 |

### crates/gloss-app/src/pipeline.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| classified_chunks_and_done_flow_back_in_order | 分类、增量与完成按序回流 | 给定脚本化引擎流（脚本解析不出分类 kind，落兜底），当跑完整桥，则 TaskClassified → TaskChunk（按序携带代数）→ TaskDone（完成态解析后正文）依次回传 | 2026-10-03 |
| cancel_takes_effect_mid_stream | 流中取消即时生效 | 给定慢流中紧随首 chunk 的取消，当取消，则不再有任何后续事件 | 2026-09-19 |
| engine_failure_becomes_task_failed | 引擎失败映射 TaskFailed | 给定 execute 整体失败，当运行，则映射为带代数的 TaskFailed | 2026-09-19 |
| background_panic_becomes_task_failed_and_the_loop_survives | 后台 panic 转失败且循环存活 | 给定注入 panic 的引擎，当任务炸掉，则转 EngineResponse 失败且循环存活、第二个任务照常完成 | 2026-09-19 |
| closing_commands_stops_the_consumer | 关闭命令通道停消费循环 | 给定通道③关闭，当 drop 运行时，则超时内干净关停 | 2026-09-19 |

### crates/gloss-app/src/machine.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| probe_mapping_covers_wired_events_only | 探测映射只覆盖已接线事件 | 给定划词手势与未接线事件（框选、设置、退出），当 begin_selection_probe，则前者发 AcquireText（无 kind——类型归 LLM 层）且只领探测编号不动代数、后者 None 且不占代数不顶掉在途探测 | 2026-10-03 |
| trigger_decision_separates_blocked_and_unwired_events | 触发去向分出被拦/未接线 | 给定 trigger_decision，则划词恒为 Acquire（无载荷——分类是 LLM 层的事）、框选与退出报 Unwired、设置与退出不因场景被拦；出厂配置下划词为 Acquire，拦截名单内的前台应用则报 Blocked | 2026-10-03 |
| scene_gate_stops_the_probe_before_acquisition | 场景闸门在取材前停住探测 | 给定安全输入开启（前台应用不在名单内）、再给定「前台应用在名单内且安全输入关闭」，当 begin_selection_probe，则两次都 None、无探测编号、状态留 Idle；场景恢复后同一手势照常探测（仍不动代数） | 2026-10-01 |
| selection_options_pair_with_one_snapshot_including_the_model | 选项（含模型）出自同一快照 | 给定自定义配置快照，当划词探测并提交产物，则目标语言与模型 id 均出自快照冻结（类型不在其中——kind 由 LLM 层分类决定） | 2026-10-03 |
| classified_kind_updates_the_streaming_chip_only_once_current | 分类结果只更新当前代的流式标签 | 给定推理中的流式视图，当 accept_classified，则当前代写入判定 kind、陈旧代与已定格产物卡拒绝、无流式视图不采纳 | 2026-09-26 |
| code_language_prefers_the_hint_and_rides_into_the_outcome | 语言 hint 优先并随行进产物卡 | 给定 hint CodeLanguage("py") 的划词提交，当检查流式视图，则 code_lang 为归一化后的 "python"（hint 胜内容探测）；accept_done 后当检查产物卡，则 code_lang 原样随行 | 2026-10-02 |
| options_freeze_at_probe_time | 选项在探测时刻冻结 | 给定探测后更换配置（目标语言与界面语言同时变），当提交产物，则任务仍带探测时快照的选项（含 prompt_locale）；第二次探测才用新值 | 2026-10-01 |
| prompt_locale_follows_config_language_and_the_system | 任务选项落定模板语言 | 给定显式 Language::En 与出厂 System 两份配置，当探测并提交产物，则任务携带的 prompt_locale 分别为 En 与注入的系统语言 | 2026-09-22 |
| commit_selection_yields_run_request_and_supersedes_the_previous_session | 提交探测接管会话并守卫状态 | 给定命中在途探测的合法产物，当 commit_selection，则返回下发请求、探测编号提升为代数、旧在途任务被取消、视图换流式卡；同编号重复提交被拒 | 2026-10-01 |
| a_probe_leaves_a_visible_session_completely_untouched | 探测与误滑不扰动可见会话 | 给定推理中的可见会话，当新划词探测、再当探测以空选区失败收场，则状态、视图、取消令牌全程原样，流式正文照常追加 | 2026-10-01 |
| image_input_for_text_kind_is_rejected | 文本 kind 拒绝图像输入 | 给定文本 kind 配图像输入，当提交，则 Ignored | 2026-09-19 |
| hide_abandons_inflight_and_drops_late_events | 隐藏放弃在途并拒迟到事件 | 给定 Translating 态隐藏，当收起，则令牌取消、视图清空回 Idle，迟到同代数产物/失败被拒 | 2026-09-19 |
| hide_overlay_drops_the_outstanding_probe | 隐藏作废在途探测 | 给定在途探测，当收起浮层，则迟到的探测产物与失败均被拒（不得把浮层弹回） | 2026-10-01 |
| the_streaming_view_waits_unclassified_until_the_llm_layer_reports | 流式视图等 LLM 层分类精化 | 给定划词提交（hint 为空），当检查流式视图，则 classified 与 code_lang 创建均为 None（等 LLM 层回传） | 2026-10-03 |
| failed_guard_matches_translating_only | 失败守卫只认推理在途态 | 给定推理中任务的失败、以及隐藏后的迟到失败，当采纳，则前者落 Error（Shown）、后者被拒（Ignored） | 2026-10-03 |
| probe_no_selection_failures_are_silently_dropped | 探测空选区静默丢弃 | 给定在途划词探测，当收 SelectionUnavailable / SelectionEmpty 失败，则静默丢弃不弹卡，状态机与当前显示一律不动、探测编号消费 | 2026-10-01 |
| probe_permission_failures_still_raise_the_card | 探测权限失败仍弹卡 | 给定在途划词探测，当收 AccessibilityDenied 失败，则接管会话落 Error 弹失败卡（真实故障需要显式反馈） | 2026-10-01 |
| selection_failures_outside_the_probe_still_raise_the_card | 推理期取材类失败仍弹卡 | 给定划词提交已进推理态，当收 SelectionUnavailable，则落 Error 弹卡（静默只覆盖探测一腿） | 2026-10-01 |
| stale_probe_results_are_dropped | 陈旧探测产物整体丢弃 | 给定新探测替换旧探测，当旧编号的产物/失败到达，则一律 Ignored | 2026-10-01 |
| probe_in_the_self_frontmost_scene_is_suppressed_without_an_id | 自身前台的划词被拦且不占编号 | 给定前台应用 is_self 为真，当手势 trigger_decision，则 SelfSuppressed、探测为 None、无浮层 | 2026-10-03 |
| modality_mismatch_preserves_pending_options | 模态错配保留待定选项 | 给定模态错配被拒后，当同编号合法产物到达，则仍可按冻结选项提交 | 2026-10-03 |
| transport_failure_lands_in_error | 传输失败落错误态 | 给定推理通道不可用，当 fail_transport，则落 Error、失败视图无动作按钮、retry 为 None | 2026-09-19 |
| retryable_failure_keeps_request_and_retry_redispatches_it | 可重试失败保留请求 | 给定网络类失败，当落 Error，则失败原因按变体记录（FailureCause::Task(EngineNetwork)）、retry 同代数按原 input+options 新令牌重发且回流式视图（不回读配置） | 2026-10-03 |
| error_actions_follow_the_mapping_table | 错误动作按映射表 | 给定限流/鉴权/模态/配置类失败，当映射，则限流可重试，其余引导打开设置且不可重试 | 2026-09-19 |
| new_commit_and_hide_supersede_the_retry_request | 新提交与隐藏取代重试 | 给定失败卡在场时新探测，则 retry 仍在（误滑不得杀掉重试出口）；当提交产物或隐藏，则 retry 返回 None | 2026-10-03 |
| suspicious_input_is_dropped_while_the_visible_session_survives | 可疑内容丢弃且当前显示保留 | 给定可见会话（产物卡在场）时到达带令牌的探测产物，当提交，则结果是 Blocked{Token}、探测消费、可见会话的状态与视图原样保留、已完结会话的令牌不被取消 | 2026-10-01 |
| blocked_input_is_not_redispatched_by_any_later_path | 被拦下的取材没有旁路 | 给定已拦下的探测产物（卡号命中），当 retry、同编号重复提交、同编号 chunk/done/failed、以及同编号通道故障（fail_acquire / fail_transport）陆续到达，则全部被拒且状态机不被触碰；新探测后同一份可疑文本仍被拦下 | 2026-10-01 |
| ordinary_input_still_passes_the_content_gate | 日常文本照常通过内容闸门 | 给定一段普通中文，当提交，则照常 Dispatch 并进 Translating（闸门只认高置信度模式） | 2026-09-24 |

### crates/gloss-app/src/update/mod.rs

模块接线与集成时序（L1）：经公共 API（消息通道 + watch 广播）驱动，桩 hooks 用本地 oneshot 触发回包，同步点全走 watch，不碰网络。

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| initial_broadcast_is_the_idle_snapshot | 启动即广播初始快照 | 给定刚拉起的模块，当订阅 watch，则先收到 Idle 快照 | 2026-09-26 |
| state_transition_wakes_the_ui_without_input | 相位迁移唤醒壳层 | 给定已安装的唤醒桩，当检查被受理与在途回包被采纳，则两次迁移各触发一次唤醒、全程无输入 | 2026-09-27 |
| confirmed_flow_runs_check_download_install_to_completion | 两道确认全流程到替换成功 | 给定新版清单与下载、替换成功桩，当检查→确认下载→确认重启，则相位依次推进且替换收到 zip 路径、任务以「已安装」收尾 | 2026-09-26 |
| not_newer_manifest_lands_in_up_to_date_without_a_target | 无新版落 UpToDate | 给定与本地等版本的清单回包，当检查完成，则落 UpToDate 且无目标版本 | 2026-09-26 |
| manifest_failure_lands_in_failed_and_retry_rechecks | 清单步失败与重试 | 给定清单拉取失败回包，当检查完成再点重试，则先落 Failed(Manifest) 再回 Checking 并重跑检查桩 | 2026-09-26 |
| download_failure_lands_in_failed_and_retry_resumes | 下载步失败与续传重试 | 给定下载失败回包，当失败后再点重试，则落 Failed(Download)、重跑下载桩且 resume 为真 | 2026-09-26 |
| install_failure_lands_in_failed_install_and_retry_installs | 替换步失败与重试 | 给定替换失败回包，当失败后再点重试，则落 Failed(Install)、重跑替换桩并以成功收尾 | 2026-09-26 |
| cancel_during_download_returns_to_update_available | 取消下载与迟到回包丢弃 | 给定下载中任务，当取消，则回 UpdateAvailable；其后的迟到下载回包被丢弃（确认下载仍可用即相位未被动过） | 2026-09-26 |
| fetch_manifest_parses_a_valid_body_from_the_wire | 线上清单解析 | 给定本地服务器回的合法清单（带 Content-Length），当 fetch，则解析出版本 | 2026-09-27 |
| fetch_manifest_rejects_oversize_body_without_content_length | 无长度声明的超限 body 拒绝 | 给定不声明 Content-Length、逐块送出 1.5 MiB 的服务器，当 fetch，则中途判超限拒绝（无界内存禁入） | 2026-09-27 |

### crates/gloss-app/src/update/manifest.rs

清单校验矩阵与版本比较（L1）。

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| valid_manifest_parses_with_all_fields | 合法清单全字段解析 | 给定 schema=1 双架构齐备的清单，当 parse，则版本、发布时间、说明页与本机架构条目全部就位 | 2026-09-26 |
| unknown_fields_are_ignored_for_forward_compatibility | 未知字段前向兼容 | 给定含 dmg_url 等未知字段的清单，当 parse，则照常通过 | 2026-09-26 |
| optional_fields_may_be_absent | 可选字段缺省 | 给定无 published_at/notes_url 的清单，当 parse，则通过且对应 getter 为 None | 2026-09-26 |
| unknown_schema_is_fail_closed | 未知 schema 拒绝 | 给定 schema 为 0/2/99 的清单，当 parse，则 UnknownSchema 拒绝且不继续解析 | 2026-09-26 |
| unparsable_body_is_rejected | 非法 body 拒绝 | 给定空串、非 JSON、数组、schema 类型不符的 body，当 parse，则 Unparsable | 2026-09-26 |
| version_must_be_a_semver_triple | 版本必须可解析 | 给定空串/两段/带 v/非数字的 version，当 parse，则 Invalid | 2026-09-26 |
| both_architectures_are_required_by_the_matrix | 双架构条目必须齐备 | 给定缺 channels、缺 stable、单架构的清单，当 parse，则 Invalid | 2026-09-26 |
| artifact_fields_follow_the_matrix | 条目字段矩阵 | 给定缺 url/size/sha256 或类型不符的条目，当 parse，则 Invalid（类型不符 Unparsable） | 2026-09-26 |
| artifact_url_must_be_https | 条目 url 仅接受 https | 给定 http:// 的 url，当 parse，则 Invalid | 2026-09-26 |
| sha256_must_be_64_lowercase_hex | sha256 形态校验 | 给定非 64 位/大写/非 hex 的 sha256，当 parse，则 Invalid | 2026-09-26 |
| is_newer_follows_semver_precedence | semver 全序比较 | 给定更高/相等/更低/预发布/构建元数据版本，当比较，则按 semver 全序判定是否更新 | 2026-09-26 |
| arch_keys_cover_the_published_pair | 架构键覆盖双 target | 给定编译期架构键，当取键对，则恰为 aarch64/x86_64 两个发布 target | 2026-09-26 |
| current_version_matches_the_cargo_package_version | 本地版本回落分支锁定 | 给定 CARGO_PKG_VERSION，当解析，则 current_version 与之相等（回落分支不可达） | 2026-09-26 |

### crates/gloss-app/src/update/state.rs

更新子状态机迁移表逐行（L1）。

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| starts_from_idle_into_checking_with_a_command | 发起检查进 Checking | 给定 Idle 状态机，当 check，则落 Checking 并交出清单拉取命令与令牌 | 2026-09-26 |
| checking_manifest_newer_lands_in_update_available | 新版落 UpdateAvailable | 给定更高版本的清单，当采纳，则锁目标版本与本机架构产物 | 2026-09-26 |
| checking_manifest_not_newer_lands_in_up_to_date | 无新版落 UpToDate | 给定不高于本地的清单，当采纳，则落 UpToDate 且清掉旧目标 | 2026-09-26 |
| checking_manifest_unavailable_lands_in_failed_manifest_step | 清单失败落清单步 | 给定清单不可用，当采纳，则落 Failed(Manifest) 且令牌清空 | 2026-09-26 |
| recheck_from_any_active_phase_cancels_and_returns_to_checking | 重复检查先取消 | 给定下载中的任务，当再次检查，则回 Checking 且下载令牌被取消 | 2026-09-26 |
| ready_to_restart_refuses_a_new_check | 替换执行中拒绝新检查 | 给定 ReadyToRestart，当 check，则拒绝且相位不动 | 2026-09-26 |
| confirm_download_only_from_update_available | 确认下载仅限待确认态 | 给定 Idle/Checking/UpdateAvailable，当 confirm_download，则仅最后者交出全量下载命令（resume 为假） | 2026-09-26 |
| download_verified_lands_in_update_ready_then_install_confirms | 下载完成与确认重启 | 给定下载完成回包，当采纳并确认重启，则经 UpdateReady 落 ReadyToRestart 并交出替换命令 | 2026-09-26 |
| download_failed_lands_in_failed_download_step | 下载失败落下载步 | 给定下载失败回包，当采纳，则落 Failed(Download) 且目标保留 | 2026-09-26 |
| retry_returns_to_the_recorded_step | 重试回到记录步骤 | 给定三类失败态，当 retry，则分别回 Checking/Downloading(resume)/ReadyToRestart 并交出对应命令 | 2026-09-26 |
| install_failed_lands_in_failed_install_step | 替换失败落替换步 | 给定 ReadyToRestart，当采纳替换失败，则落 Failed(Install) | 2026-09-26 |
| cancel_only_interrupts_a_download | 取消仅对下载有效 | 给定 Idle/Checking/Downloading，当 cancel，则仅下载中取消令牌并回 UpdateAvailable，其余拒绝 | 2026-09-26 |
| late_outcomes_are_rejected_by_phase_guards | 迟到回包被相位守卫拒绝 | 给定已离开中间相位的状态机，当重放各回包，则一律拒绝且状态不动 | 2026-09-26 |
| update_available_clears_when_a_new_check_finds_nothing_newer | 新检查收回旧提示 | 给定 UpdateAvailable，当重新检查发现无新版，则落 UpToDate 且目标撤下 | 2026-09-26 |

### crates/gloss-app/src/update/download.rs

整包下载与校验（L1，mock HTTP 服务器驱动）。

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| full_download_verifies_and_lands_at_dest | 全量下载落位 | 给定 200 全量响应，当下载，则 size/sha256 校验通过、原子改名落位且 .partial 消失 | 2026-09-26 |
| truncated_stream_keeps_partial_for_resume | 断连保留残料 | 给定提前断连的响应，当下载失败，则报长度/传输错误且 .partial 保留在途字节 | 2026-09-26 |
| resume_from_partial_completes_and_verifies | 续传完成并全量校验 | 给定遗留 .partial 与支持 Range 的服务器（206），当 resume 下载，则拼接完整、校验通过、落位 | 2026-09-26 |
| server_without_range_support_restarts_from_scratch | 不支持 Range 整体重下 | 给定忽略 Range 的服务器（200），当 resume 下载，则从头重下且结果完整不重复 | 2026-09-26 |
| sha_mismatch_discards_the_partial | 校验不符拒绝并丢弃 | 给定 sha256 与清单不符的响应，当下载完成，则报 ShaMismatch 且 .partial 已丢弃 | 2026-09-26 |
| oversize_response_is_rejected_and_discarded | 超长响应拒绝 | 给定超过清单 size 的响应，当下载，则报 TooLarge 且 .partial 已丢弃 | 2026-09-26 |
| cancellation_returns_cancelled_without_dest | 取消即取消 | 给定已取消的令牌，当下载，则报 Cancelled 且无落位文件 | 2026-09-26 |
| bad_artifact_url_is_rejected_before_any_request | 非法产物名前置拒绝 | 给定取不出文件名的 url，当下载，则在发起请求前报 Network | 2026-09-26 |

### crates/gloss-app/src/update/install.rs

bundle 原位替换（L1，临时目录夹具 + ditto 构造 zip）。

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| replace_swaps_bundle_and_leaves_no_litter | 替换换装无残留 | 给定旧 bundle 与合法 zip，当 replace，则新 bundle 就位原路径、.app.old 清除、解压现场清空 | 2026-09-26 |
| replace_over_a_stale_app_old_still_succeeds | 残留 .app.old 先清场 | 给定上次替换遗留的 .app.old，当替换，则先清场并成功换装 | 2026-09-26 |
| zip_without_a_structured_app_leaves_the_bundle_intact | 坏 zip 不动原 bundle | 给定非 zip 文件，当 replace，则报 Unzip 且已安装 bundle 原样 | 2026-09-26 |
| zip_with_an_app_missing_macos_dir_is_invalid_structure | 包结构检查 | 给定缺 Contents/MacOS 的 .app zip，当 replace，则报 InvalidStructure 且原 bundle 原样 | 2026-09-26 |
| unwritable_dir_is_reported_before_anything_is_touched | 只读目录前置拒绝 | 给定只读的 bundle 目录，当安装，则报 Unwritable 且 bundle 未被触碰 | 2026-09-26 |

### crates/gloss-app/src/ui/fonts.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| cjk_fallback_appends_after_builtin_fonts | CJK 后备排在内置字体后 | 给定字体字节，当接入字体定义，则接入成功且 CJK 后备排在比例与等宽两族内置字体之后 | 2026-10-01 |
| named_family_without_bytes_binds_the_builtin_glyphs | 命名字体族无字节时绑内置字形 | 给定缺失的字体字节，当注册宋体命名字体族，则不发明字体数据、族仍绑定到内置字形（epaint 对未绑定族直接 panic，族必须恒存在） | 2026-10-01 |
| cjk_fallback_shares_bytes_across_contexts | 后备字体字节按借用登记、零拷贝共享 | 给定同一段字体字节接入两份字体定义，当检查，则两份定义都按借用登记且指向同一地址 | 2026-10-01 |
| named_family_registers_bytes_verbatim | 命名字体族按字节注册 | 给定字体字节，当注册宋体命名字体族，则族名指向自身且字节按借用原样登记 | 2026-10-01 |
| definitions_always_bind_the_named_typography_families | 宋楷与等宽命名族恒绑定 | 给定真实系统的字体定义，当取 definitions，则宋楷与等宽三命名族要么有自身字体数据、要么绑定到内置字形（epaint 对未绑定族直接 panic，族恒存在） | 2026-10-02 |
| zhu_family_is_always_named | 注区字体族恒为命名族 | 给定 zhu_family 入口，当取值，则恒为楷体命名字体族 | 2026-10-01 |
| mono_family_is_always_named | 代码/音标字体族恒为命名族 | 给定 mono_family 入口，当取值，则恒为 gloss-mono 命名字体族 | 2026-10-02 |
| mono_family_chain_appends_the_cjk_fallback | 等宽族绑定含 CJK 后备 | 给定等宽与 CJK 两段字体字节（CJK 已按后备登记），当注册等宽命名字族，则族链为系统等宽在前、CJK 后备在后（代码内中文可读） | 2026-10-02 |
| mono_family_without_a_system_mono_binds_the_builtin_glyphs | 等宽族不向比例 CJK 降级 | 给定缺失的系统等宽字节，当注册等宽命名字族，则族绑内置字形、不落 CJK 比例字体（等宽对齐优先于字形覆盖） | 2026-10-02 |
| system_cjk_font_is_discoverable | 系统 CJK 字体可发现 | 给定真实系统，当执行 CJK 字体发现，则必须找到且字节非空（依赖宿主机） | 2026-10-01 |
| system_monospace_font_is_discoverable | 系统等宽字体可发现 | 给定真实系统，当按候选顺序（SF Mono → Menlo → Monaco → DejaVu Sans Mono → Consolas）执行等宽字体发现，则必须找到（依赖宿主机） | 2026-10-02 |
| system_cjk_font_is_loaded_once | 系统 CJK 字体只加载一次且两份定义共享 | 给定真实系统，当连续两次取字体字节并各自接入字体定义，则两次返回同一段内存、两份定义登记到同一地址（依赖宿主机） | 2026-10-01 |

### crates/gloss-app/src/ui/settings.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| save_trims_endpoint_and_treats_blank_key_as_unchanged | 保存端点 trim、空白密钥视为未改 | 给定带空白的端点与空白密钥草稿，当 build_save，则端点被 trim、密钥按 Keep 上交 | 2026-09-22 |
| blank_model_saves_as_the_factory_default | 空白模型折叠出厂默认 | 给定模型输入框为空白的草稿，当 build_save，则落盘 config.model 为 DEFAULT_TEXT_MODEL（下游对空串明确失败，不接住会锁死任务） | 2026-10-03 |
| save_carries_the_key_outside_the_config | 密钥走带外通道不上配置 | 给定非空密钥草稿，当 build_save，则密钥走 KeyUpdate::Replace、不进配置（连 Debug 表示也不含） | 2026-09-22 |
| clear_key_is_deferred_to_save_and_revocable | 清除密钥延迟到保存且可撤销 | 给定「清除密钥」标记，当交互与保存，则删除延迟到保存生效、重新输入可撤销标记 | 2026-09-22 |
| invalid_draft_blocks_save_and_enters_the_error_state | 非法草稿阻断保存进入错误态 | 给定非法 Base URL 草稿，当 build_save，则返回 Idle、置校验态、该字段提示含 https 规则 | 2026-09-22 |
| fixing_the_field_restores_save | 改对字段恢复保存 | 给定被阻断的校验态，当修正 Base URL，则错误清空、再次 build_save 上交 Save | 2026-09-22 |
| open_copies_the_snapshot_into_the_draft | 打开设置拷贝快照进草稿 | 给定打开时的快照，当建草稿并随后改原配置，则草稿不跟随、可携带提示 | 2026-09-19 |
| field_errors_are_worded_per_locale | 字段错误按 locale 出措辞 | 给定全部九类字段错误（含行号与触发键回显两种模板），当按中英文表取文案，则各出对应措辞（换臂或漏译会被抓住） | 2026-09-23 |
| every_notice_renders_its_localized_prefix_and_detail | 三类提示的中英措辞 | 给定三类壳回写提示（各带同一诊断），当按中英表取文案，则前缀与诊断都按表落地、且无残留的 {{占位符}}（两条从未渲染过的模板由此覆上） | 2026-09-22 |
| base_url_errors_map_to_their_own_field_error | Base URL 错因映射到字段错误 | 给定五类 BaseUrlError，当映射，则空/语法与 https/内嵌凭据/查询参数各落到对应 FieldError（内嵌凭据与查询参数两臂易错） | 2026-09-22 |
| update_section_buttons_follow_the_phase | 更新区动作按钮随相位 | 给定七个带动作的更新相位，当点对应按钮，则上交 Check/ConfirmDownload/Cancel/ConfirmRestart/Retry 之一 | 2026-09-27 |
| update_busy_phases_offer_no_action_and_show_the_target_version | 忙碌相位无动作、显示目标版本 | 给定 Checking/ReadyToRestart，当渲染，则无任何更新动作上交；UpdateAvailable 的行标签含目标版本号 | 2026-09-27 |
| failed_install_shows_the_install_hint | 替换失败附安装指引 | 给定 Failed(Install)，当渲染，则指引行可见（DMG 装入 / 权限出口）且无副作用动作 | 2026-09-27 |

### crates/gloss-app/src/ui/style.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| ladders_are_strictly_descending | 字号与间距阶梯严格递减 | 给定字号与间距两条阶梯，当逐档比较，则每档严格大于下一档（档位语义不塌缩） | 2026-09-21 |

### crates/gloss-platform/src/appearance/icon.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| install_degrades_to_false_off_the_main_thread | 非主线程安装图标优雅降级 | 给定非主线程调用与坏 PNG 字节，当 install，则不 panic 且如实返回 false | 2026-09-19 |

### crates/gloss-platform/src/ffi/cf.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| cf_string_round_trips_back_to_utf8 | CFString 往返回环 | 给定 C 字符串，当构造 CFString 再转回 Rust 字符串，则内容逐字一致（含非 ASCII） | 2026-09-23 |
| string_from_rejects_non_string_objects | 非 CFString 拒绝转换 | 给定 CFData，当按字符串转换，则返回 None（先验类型） | 2026-09-23 |
| string_from_respects_the_byte_ceiling | 字节上界起作用 | 给定远低于所需的字节上界，当转换，则返回 None | 2026-09-23 |
| empty_cf_string_converts_to_empty_rust_string | 空串转换 | 给定空 CFString，当转换，则得到空 Rust 字符串而非 None | 2026-09-23 |

### crates/gloss-platform/src/ffi/carbon.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| secure_input_query_links_and_answers_consistently | 安全输入查询可链接且自洽 | 给定真实系统，当连续查询两次安全输入，则两次一致——不断言系统的当前取值（那是环境事实），只要 Carbon 框架没链上或符号对不上，链接期就红 | 2026-09-26 |

### crates/gloss-platform/src/locale.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| locale_maps_preferred_languages | 首选语言映射界面语言 | 给定 zh-Hans-CN / zh_CN / ZH-TW / en-US / ja-JP 与空值，当映射，则中文标签归中文、其余（含拿不到偏好语言）归英文 | 2026-09-22 |

### crates/gloss-platform/src/scene.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| scene_probe_answers_with_a_coherent_snapshot | 场景探针给出一致快照 | 给定真实系统，当读一次场景事实，则若报出前台应用则其身份字段非空（不断言安全输入取值——那是环境事实，任何进程持有它都会变） | 2026-09-26 |

### crates/gloss-platform/src/storage/mod.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| save_then_load_round_trips_every_field | 全字段配置存取往返 | 给定全字段差异样例配置，当 save→load，则逐字段相等 | 2026-09-19 |
| load_generates_default_when_file_missing | 缺文件生成默认配置 | 给定缺文件目录，当 load，则返回出厂默认且落盘文件可再解析 | 2026-09-19 |
| load_creates_missing_directories | 缺目录自动创建 | 给定连父级都缺的目录，当 load 落盘，则目录被创建 | 2026-09-19 |
| composite_delegates_document_methods | 组合存储委托文档半边 | 给定 CompositeConfigStore，当 save→load，则完整委托文档半边、逐字段一致 | 2026-09-19 |
| composite_forwards_secret_methods_to_half | 组合存储转发密钥半边 | 给定内存桩密钥半边，当经组合体 set/get/delete，则读回写入值、删除后 None | 2026-09-19 |
| persisted_document_stores_no_secret_material | 落盘文档不含密钥本体 | 给定 save 写出的 TOML，当检查内容，则含 keychain_id 但不含密钥本体 | 2026-09-19 |
| default_store_targets_standard_config_dir | 默认存储落在标准目录 | 给定 Default 构造与真实 HOME，当定位，则路径为 gloss/config.toml（BaseDirs 缺失时跳过） | 2026-09-19 |
| load_rejects_corrupt_file_and_quarantines_it | 损坏配置隔离降级 | 给定损坏配置，当 load，则报 Config 错、错误文本不转述内容、原文件字节原样挪入唯一 .bak | 2026-09-19 |
| legacy_document_gets_factory_endpoint_and_provider | 老配置回填出厂端点 | 给定 M4-T3 老配置（空 provider 数组），当 load，则回填出厂端点与出厂 provider/模型 | 2026-09-19 |
| load_recovers_after_quarantine | 隔离后自愈 | 给定损坏文件已被隔离，当再次 load，则自愈落一份出厂默认 | 2026-09-19 |
| concurrent_saves_keep_the_document_parseable | 并发保存文件可解析 | 给定 4 线程各并发 save 20 次，当结束后 load，则文件完整可解析（主题为某一完整版本） | 2026-09-19 |
| error_line_reports_the_offending_line | 错误行号指向真实出错行 | 给定解析错误与 span，当报告，则行号落在真实出错行；span 缺失返回 None | 2026-09-19 |
| default_cache_ttl_matches_core_cache_semantics | 默认 TTL 与 core 语义一致 | 给定默认 TTL 字段，当与 cache 语义对照，则恰为 1 小时 | 2026-09-19 |

### crates/gloss-platform/src/storage/keychain.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| missing_entry_reads_as_none | 缺条目读为 None | 给定先删除预清的条目，当 get，则 None 而非错误 | 2026-09-19 |
| delete_missing_entry_is_ok | 删除缺条目幂等成功 | 给定不存在的条目，当 delete，则幂等成功 | 2026-09-19 |
| cached_reads_stay_consistent_with_writes | 缓存读写一致 | 给定写后读与删除后读，当经克隆共享缓存，则写后读新值、删除后克隆读 None（不走真实 keychain 写入） | 2026-09-19 |

### crates/gloss-platform/src/selection/accessibility.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| untrusted_maps_to_accessibility_denied | 未授权一律映射拒绝 | 给定未授权进程的取值结果，当 interpret 映射，则一律 AccessibilityDenied | 2026-09-19 |
| selected_text_passes_through_verbatim | 选中文本原样透传 | 给定读到的选中文本（含纯空白），当映射，则原样透传不裁剪 | 2026-09-19 |
| an_empty_selection_is_empty_but_a_missing_value_stays_unavailable | 空选区与读不到分开建档 | 给定空串与 None（超上限/非字符串），当映射，则前者 SelectionEmpty（没选东西）、后者 SelectionUnavailable（选了但读不动，兜底仍值得一试）；NoValue 归前者 | 2026-09-30 |
| api_disabled_after_trusted_check_maps_to_denied | 授权后 API 禁用仍映射拒绝 | 给定授权后 AX 报 APIDisabled，当映射，则仍是 AccessibilityDenied | 2026-09-19 |
| adjacent_error_codes_are_told_apart | 相邻错误码区分 | 给定相邻码 -25211（APIDisabled）与 -25212（NoValue），当映射，则前者 Denied、后者 SelectionEmpty（错一位语义就反转） | 2026-09-30 |
| other_ax_errors_map_to_unavailable | 其余 AX 错误映射不可用 | 给定 -25206/-25213/-25200，当映射，则归 SelectionUnavailable | 2026-09-19 |

### crates/gloss-platform/src/permissions.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| accessibility_denied_is_recognized | 权限错误语义识别 | 给定 AccessibilityDenied 与其它取材错误，当识别，则前者命中、其余不误伤 | 2026-09-29 |

### crates/gloss-platform/src/selection/composite.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| fallback_is_lazy_on_ax_success | AX 成功时兜底惰性求值 | 给定 AX 成功，当 combine，则采纳 AX 结果且剪贴板兜底闭包不被求值 | 2026-10-01 |
| permission_denied_skips_fallback | 权限拒绝跳过兜底 | 给定 AccessibilityDenied，当 combine，则原样上抛且不兜底 | 2026-10-01 |
| empty_selection_settles_then_falls_back_to_the_clipboard | 空选区让渡补读后再落兜底 | 给定持续 SelectionEmpty 的 AX 读与零让渡策略，当 combine，则补读至预算耗尽（恰 4 读）才落一次剪贴板兜底并采纳其结果（确认写入把关，陈旧剪贴板不得冒充本次选区） | 2026-10-01 |
| empty_then_ready_ax_read_adopts_the_late_selection | 补读等到迟到的选区 | 给定首次空、二次有值的 AX 读，当 combine，则采纳第二次读到的选区且不兜底 | 2026-10-01 |
| settle_budget_is_bounded_and_positive | 让渡预算有界且为正 | 给定预算内、末次与超界的尝试序号，当 settle_delay，则预算内返回正的让渡时长、预算用尽（含 usize::MAX）返回 None | 2026-10-01 |
| unavailable_ax_falls_back_to_clipboard | AX 不可用落兜底 | 给定 SelectionUnavailable，当 combine，则落到兜底结果；兜底也失败则以兜底错误收口 | 2026-10-01 |

### crates/gloss-platform/src/selection/clipboard.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| change_is_detected_when_generation_bumps | 写入确认按代数探测 | 给定基线代数 0 与第 3 轮才变化的 probe，当 wait_for_write，则确认成功且恰好轮询 3 次 | 2026-09-19 |
| timeout_returns_false_without_hanging | 超时返回不悬挂 | 给定 probe 恒不变化，当到 deadline，则返回 false 且 2s 内返回 | 2026-09-19 |
| unavailable_probe_keeps_polling_until_deadline | probe 恒 None 轮询到期限 | 给定 probe 恒 None，当到 deadline，则返回 false、不提前放弃也不永久等 | 2026-09-19 |

### crates/gloss-platform/src/events/mod.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| commands_are_consumed_in_order_then_thread_exits | 命令按序消费线程干净退出 | 给定 16 条命令，当事件线程消费，则按序产出、Sender drop 后线程退出且无多余事件 | 2026-09-19 |
| late_commands_are_still_consumed | 晚到命令仍被消费 | 给定跨定时器周期（60ms）晚到的命令，当消费，则仍被处理 | 2026-09-19 |
| sources_are_polled_and_forwarded | 事件源轮询转发 | 给定事件源每轮抽干，当 tick，则产出经 sink 送平台通道、命令照常消费 | 2026-09-19 |
| panicking_handler_does_not_kill_thread | 处理器 panic 不杀线程 | 给定会 panic 的命令处理器，当处理，则 panic 被拦截、后续命令照常消费 | 2026-09-19 |
| panicking_source_does_not_kill_thread | 事件源 panic 不杀线程 | 给定首轮 panic 的事件源，当轮次推进，则线程存活、后续轮次照常抽干 | 2026-09-19 |
| sink_send_tolerates_closed_receivers | sink 容忍关闭的接收端 | 给定接收端已关闭的 sink，当发送，则返回 false 不 panic | 2026-09-19 |
| successful_send_wakes_main_thread | 发送成功唤醒主线程 | 给定主线程在事件循环等待，当发送成功，则唤醒回调被触发；发送失败不唤醒 | 2026-09-19 |

### crates/gloss-platform/src/events/mouse.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| drag_release_emits_selection | 拖拽释放判定划词手势 | 给定按下→时长与位移双达标→释放序列，当驱动 GestureDetector，则判定一次划词手势 | 2026-09-30 |
| flick_shorter_than_the_minimum_press_is_filtered | 快甩被最短按压时长滤除 | 给定按压仅 10ms 但位移充足的事件对，当驱动，则不产出手势 | 2026-09-30 |
| press_duration_is_observed_at_the_boundary | 按压时长在边界被遵守 | 给定比下限短 1ns 与恰好等于下限（150ms）的两对事件，当驱动，则前者滤除、后者判定 | 2026-09-30 |
| plain_click_and_jitter_do_not_trigger | 原地点击与抖动不触发 | 给定原地点击与阈值内抖动，当驱动，则不产出手势 | 2026-09-19 |
| displacement_comes_from_the_events_themselves | 位移取自事件自身坐标 | 给定长距离拖拽，当算位移，则取按下/释放事件自身坐标（释放位置即位移来源） | 2026-09-19 |
| state_resets_after_each_gesture | 每轮手势后状态重置 | 给定连续两轮完整手势，当驱动，则每轮后状态机重置、第二轮照常判定 | 2026-09-19 |
| stray_events_are_ignored | 杂散事件静默忽略 | 给定无按下的释放、双按下等杂散序列，当驱动，则静默忽略、后续手势照常 | 2026-09-19 |
| poll_drains_channel_through_state_machine | poll 抽干通道过状态机 | 给定事件通道，当 poll，则抽干并经状态机计数（第二次 poll 为 0） | 2026-09-19 |
| keyboard_events_are_never_subscribed | 键盘事件永不订阅 | 给定生产订阅掩码与 classify，当断言，则只有左键按下/抬起两位、键盘/滚轮/flags 各位不置（回归护栏） | 2026-09-19 |

### crates/gloss-platform/src/engine/llm.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| resolves_endpoint_from_config | 端点从配置解析 | 给定出厂配置，当解析端点，则得 {DEFAULT_BASE_URL}/chat/completions | 2026-09-19 |
| endpoint_trimming_avoids_double_slash | 端点尾斜杠去重 | 给定尾斜杠 base_url，当解析，则无双斜杠 | 2026-09-19 |
| cleartext_endpoints_are_rejected | 明文端点拒绝 | 给定 http:// 明文端点（含 localhost/127.0.0.1），当解析，则报 Config 错 | 2026-09-19 |
| endpoints_with_credentials_query_or_fragment_are_rejected | 带凭据/查询/片段的端点拒绝 | 给定带 userinfo/query/fragment 的端点，当解析，则拒绝 | 2026-09-19 |
| unusable_endpoints_report_config_error | 不可用端点报配置错误 | 给定空串/空白/file:///无 scheme 地址，当解析，则报 Config 错 | 2026-09-19 |
| resolves_key_from_keychain | 密钥从 keychain 解析 | 给定出厂配置+内存密钥，当解析，则取出密钥原文 | 2026-09-19 |
| missing_key_reports_auth_error | 缺密钥报鉴权错误 | 给定未配置密钥，当解析，则 EngineAuth | 2026-09-19 |
| blank_key_counts_as_missing | 空白密钥视为缺失 | 给定空白密钥，当解析，则按未配置报 EngineAuth | 2026-09-19 |
| empty_provider_list_falls_back_to_the_factory_entry | 空 provider 回退出厂条目 | 给定显式空 provider_keys 与出厂 keychain 条目密钥，当解析，则回退出厂条目 gloss/deepseek | 2026-09-19 |
| adapter_streams_deltas_across_chunk_boundaries | SSE 适配器跨块流式 | 给定同一段 SSE 响应按 1/2/5/整包字节分块，当经 SseStream 适配，则增量恒为「光泽」、[DONE] 终止、余字节不解析 | 2026-09-19 |
| adapter_delivers_deltas_then_terminates_on_protocol_error | 协议错误先交增量再终结 | 给定增量后跟坏 JSON，当流推进，则先交付增量、随后以 EngineResponse 错误终结 | 2026-09-19 |
| adapter_ends_cleanly_without_the_done_marker | 无 [DONE] 关流干净收尾 | 给定无 [DONE] 直接关流的响应，当解析，则带增量流按正常结束收尾 | 2026-09-19 |
| adapter_reports_empty_completion_as_failure | 零增量完成报失败 | 给定零增量响应（仅 [DONE] 或 HTML 劫持页），当解析，则报 EngineResponse 错且流即止 | 2026-09-19 |
| request_body_has_the_openai_envelope | 请求体 OpenAI 信封 | 给定未设限的 EngineRequest，当构造请求体，则 model/messages/stream 三键 wire 形态精确匹配且无 max_tokens 键 | 2026-09-26 |
| max_tokens_is_carried_only_when_set | max_tokens 仅设限时上线 | 给定 max_tokens=None 与 Some(64) 两个请求，当构造请求体，则 None 无该键、Some 携带数值 64 | 2026-09-26 |
| maps_http_status_to_error_variants | HTTP 状态映射错误变体 | 给定 401/403/429/500/400+JSON/400+HTML，当映射，则分别得 EngineAuth/EngineRateLimited/EngineNetwork/带诊断的 EngineResponse | 2026-09-19 |

### crates/gloss-platform/src/engine/sse.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| decodes_a_complete_stream | 完整流解码 | 给定完整 OpenAI 兼容流，当一次喂完，则两段增量+Done（role 块不产出） | 2026-09-19 |
| decodes_identically_for_every_chunk_split | 任意切分解码一致 | 给定同一段流按 1..n 每种字节切法，当逐块喂入，则产出完全一致 | 2026-09-19 |
| byte_by_byte_input_keeps_multibyte_characters_intact | 逐字节输入多字节字符完好 | 给定逐字节喂入，当解析，则多字节字符「光泽」完好 | 2026-09-19 |
| ignores_crlf_heartbeats_and_other_fields | 忽略 CRLF/心跳/其它字段 | 给定 CRLF/心跳注释/其它 SSE 字段，当解析，则不产出、data 行照常出增量 | 2026-09-19 |
| maps_server_errors_by_code | 服务端错误按码分类 | 给定流中错误对象（rate_limit/invalid_api_key/其它），当解析，则分类为 RateLimited/Auth/带诊断 EngineResponse | 2026-09-19 |
| unparsable_payload_reports_bounded_diagnostic | 解析失败诊断有上限 | 给定解析不出的载荷与超长错误消息，当解析，则报 EngineResponse 且诊断文本有上限 | 2026-09-19 |
| ignores_a_data_line_without_payload | 空载荷 data 行忽略 | 给定空载荷 data: 行，当解析，则按心跳忽略 | 2026-09-19 |
| ignores_bytes_after_done | [DONE] 后字节忽略 | 给定 [DONE] 后的字节，当解析，则一律忽略 | 2026-09-19 |
| finish_drops_a_truncated_tail | 残缺尾行丢弃 | 给定残缺半行，当 finish，则丢弃且此后字节不再解析 | 2026-09-19 |
| finish_keeps_a_complete_unterminated_line | 完整无换行末行保留 | 给定完整但无换行的最后一行，当 finish，则作为增量交出 | 2026-09-19 |
| rejects_a_line_beyond_the_limit | 超长行拒绝终结流 | 给定超 MAX_LINE_BYTES 的无换行洪泛，当解析，则协议错误终结流、后续不再解析 | 2026-09-19 |
| error_without_code_reports_the_message_only | 无码错误只报消息 | 给定只有 message 的错误对象，当解析，则文案不带前导冒号 | 2026-09-19 |
| error_classification_checks_code_and_type | 错误分类查码与类型 | 给定 code 通用而 type 细分类（或反之），当解析，则按细分类识别 | 2026-09-19 |

### src/main.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| channels_bundle_is_created | 组装点创建的通道捆绑可用 | 给定 create_channels() 创建的四通道捆绑，当向通道②发送 AcquireText 命令，则发送成功 | 2026-09-19 |

### crates/gloss-eval/src/dataset.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| classify_dataset_loads_with_unique_ids | 分类数据集加载与唯一性 | 给定内嵌 classify.jsonl，当加载，则 ≥60 条、全部为文本任务 kind、id 无重复 | 2026-09-26 |
| task_datasets_load_and_match_required_fields | 任务数据集加载与 kind 对位 | 给定三个内嵌任务数据集，当加载，则非空且每行 kind 与其文件一致、reference 满足必需键 | 2026-09-26 |
| fixtures_align_with_dataset_ids | 夹具与数据集 id 对齐 | 给定分类/任务夹具文件与数据集，当加载，则 id 无重复、deltas 非空、且每条夹具 id 都存在于对应数据集（陈旧 id 即失败） | 2026-09-26 |
| duplicate_ids_are_rejected | 重复 id 硬错误 | 给定含重复 id 的 jsonl，当加载，则报错而非静默跳过 | 2026-09-26 |
| bad_reference_is_rejected | reference 缺必需键硬错误 | 给定词卡条目缺 senses 的 jsonl，当加载，则报错 | 2026-09-26 |
| required_fields_follow_the_contract | 必需键随结构化契约 | 给定各任务 kind，当查必需键，则词卡 word+senses、句译/代码 title、OCR text | 2026-09-26 |

### crates/gloss-eval/src/metrics.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| classify_verdicts_cover_the_matrix | 分类判定四分类 | 给定正确/混淆/非 JSON/未知 kind/清单外 kind 五种回复，当经生产校验器判定，则分别落 Correct/Wrong/InvalidJson/Rejected | 2026-09-26 |
| legacy_fence_reply_falls_back_like_production | 旧围栏回复与生产同轨（eval） | 给定旧围栏与纯正文两类回复，当 TaskVerdict::for_reply，则判定为降级（未按现行契约）而 outcome 与生产 fallback 同形（词卡/Plain 兜底） | 2026-10-03 |
| task_verdict_reads_the_four_levels | 任务契约四级判定 | 给定完整契约/无围栏/坏 JSON 三种词卡回复，当 TaskVerdict.for_reply，则四级标志与生产降级产物（Plain 兜底）符合预期 | 2026-09-26 |
| field_completeness_requires_the_contract_keys | 字段完整性按契约键 | 给定缺 senses 的词卡围栏，当判定，则 fields_complete=false 且视为降级 | 2026-09-26 |
| latency_percentiles_interpolate | 延迟百分位线性插值 | 给定四个样本，当取 p0/p50/p95/p100，则插值结果精确；空样本返回 None | 2026-09-26 |

### crates/gloss-eval/src/judge.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| judge_prompt_carries_all_three_sections | judge rubric 三段齐备 | 给定输入/产出/参考，当渲染 judge messages，则系统指令含三段且无残留占位符 | 2026-09-26 |
| judge_reply_parsing_accepts_json_and_score_line | judge 回复分数解析 | 给定 JSON 与 SCORE: 行两种回复，当解析，则得 1–5 分；越界与缺失为 None | 2026-09-26 |

### crates/gloss-eval/src/report.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| report_renders_the_table_shape | 报告 Markdown 表格形态 | 给定含混淆矩阵与任务指标的报告，当渲染，则指标表、accuracy 百分比、混淆矩阵行、任务四率齐全且跳过数正确 | 2026-09-26 |
| latency_only_shows_when_sampled | 延迟只在有样本时出现 | 给定无延迟样本的报告，当渲染，则不含延迟行 | 2026-09-26 |

### crates/gloss-eval/src/runner.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| replay_runs_the_full_deterministic_track_over_embedded_assets | 重放轨全链路离线跑通 | 给定内嵌数据集与夹具，当 run_replay，则有夹具条目计入（覆盖率随夹具维护增长，只设下限不设上限）、正确/无效 JSON/被拒路径均有命中、各任务集有覆盖、无延迟样本 | 2026-09-26 |
