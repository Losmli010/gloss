# 测试行为清单（`gloss-app`）

本文件登记主线程应用层（`gloss-app`）的测试行为，含集成、快照与单元三类；总纲、人工测试与发版人工步骤见 `bdd.md`。

## 集成测试

文件：`crates/gloss-app/tests/pipeline.rs`（`L1`，经公共 `API` 与通道两端驱动状态机加通道三与通道四加 `tokio` 桥加 `mock` 引擎的全时序），另含进程级日志捕获夹具的窗口隔离。

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| full_flow_classifies_then_streams_and_settles | 全链路分类加流式回流与终态 | 给定划词手势注入契约（JSON）（仅 `note` 字段）的流式脚本，当全链路推进，则先回 `TaskClassified(General)`（分类解析不出 `kind`、内部修复一轮后落兜底）、`chunk` 逐条回流、`TaskDone` 后定格 `Show`、正文取自 `note` 字段、`structured` 缺失按软约束落空 `Plain` 且不降级 | 2026-10-12 |
| classify_failure_falls_back_and_the_task_still_completes | 分类请求失败内部消化回退兜底 | 给定分类请求注入一次性失败、任务脚本为契约（JSON）、命令携带本测试的任务 `span`，当桥编排分类，则分类不重试、内部消化后 `TaskClassified` 携带兜底 `General`、告警无选区内容且带本任务代数、任务照常执行完成落 `Show`（引擎共 2 次调用） | 2026-11-11 |
| second_trigger_is_a_full_cache_hit_without_engine_calls | 二次触发全缓存命中直出 | 给定同一 `input+options` 的任务第二次触发，当桥按 `cache_key(input, options)` 查产物缓存命中，则仅回 `TaskClassified+TaskDone`（无 `TaskChunk`）、引擎调用数维持 3（首轮分类含 1 轮修复加执行）、分类 `kind` 随缓存产物回放、状态定格 `Show` | 2026-10-12 |
| hide_overlay_cancels_the_stream_and_late_events_are_dropped | 收起浮层取消流并丢弃迟到事件 | 给定慢流中已收到分类结果与首个 `chunk`，当 `hide_overlay` 取消在途令牌，则不再有任何回传事件、机器侧拒绝该任务的迟到 `chunk`、产物并回 `Idle` | 2026-09-26 |
| superseded_trigger_cancels_and_filters_late_events | 新触发取消旧任务并过滤迟到事件 | 给定 `A` 的分类流未完成时触发 `B`，当 `B` 触发，则 `A` 的令牌立即取消、代数加 1、`A` 代数的迟到事件被状态机拒绝；`B` 的分类回退与任务 `chunk` 照常回流，重放的违约回复经修复轮标记（同代采纳）后降级落 `Show` | 2026-10-12 |
| failure_lands_in_error_and_retry_succeeds | 失败落错误态且重试可达 | 给定首次注入两次 `EngineRateLimited` 一次性失败（分类与任务各吃一条），当失败回传后再次触发，则落 `Error` 态、第二次任务完成落 `Show` | 2026-10-03 |
| error_card_retry_redispatches_the_same_request | 重试动作重发同一请求 | 给定注入两条一次性 `EngineNetwork` 失败（分类与任务各吃一条）的可重试 `Retry` 出口，当 `retry` 并重发 `RunTask`（`input+options` 原样），则同代数重发同一请求并完成落 `Show` | 2026-10-03 |
| frozen_options_carry_the_factory_model_by_default | 冻结选项默认带出厂模型 | 给定出厂配置的划词提交，当跑一轮完整任务，则引擎恰被调用 3 次（分类含 1 轮修复加执行）且任务照常完成 | 2026-10-12 |
| config_change_invalidates_cache_for_the_next_task | 配置变更对主缓存 `key` 的失效 | 给定同文本连续任务与运行时保存的新配置，当执行，则未改配置命中缓存（引擎维持 3 次：分类含修复加执行）、换模型再触发一轮（加 3）、换目标语言再触发一轮（加 4：语言收敛后目标语言为英文，中文样张进一次语言修复轮），共 10 次 | 2026-10-30 |
| engine_logs_carry_the_task_span | 桥日志经 `span` 带上代数 | 给定带 `span` 的任务命令（进程级捕获订阅者），当消费桥执行到缓存命中，则命中行同时含 `cache` `hit` 与 `"generation":2` | 2026-09-26 |
| capture_windows_read_only_their_own_events | 捕获窗口彼此隔离 | 给定先开先关的第一个捕获窗口与后开的第二个窗口，当第二个窗口取走缓冲文本，则文本含本窗口的探针行、不含已关窗口的探针行 | 2026-11-11 |
| violating_replies_repair_then_degrade_without_cache | 违约回复修复三轮后降级且不进缓存 | 给定分类判 `ExplainCode` 的裸（JSON）脚本与恒违约的任务回复，当跑完整桥，则事件序为 `Classified` → `Chunk` → `[Repairing → Chunk]{3}` → `Done(degraded)`：每个 `Repairing` 清空流式累积并置修复态、耗尽后完成态按 `validate_task` 落降级产物（`note` 空串、重试出口在）、同输入再触发引擎再次执行（降级产物不投毒缓存） | 2026-10-12 |
| an_image_task_repairs_through_the_same_stream | 图像任务共用同一修复轮流 | 给定图像取材注入“违约回复 → 契约（JSON）”的脚本队列，当跑完整桥，则恒定映射 `ImageExplain`（跳过分类）、违约轮 `Chunk` 后回 `TaskRepairing`、修复轮契约回复直接 `TaskDone`（正常产物）、引擎恰被调用 2 次 | 2026-10-12 |
| image_flow_skips_classification_and_settles_the_plain_examples | 图像任务跳过分类并落 `Plain` 疏证 | 给定粘贴板观察注入嵌套（JSON）契约（`note` 与 `structured.examples`）的流式脚本，当全链路推进，则先回 `TaskClassified(ImageExplain)`（图像跳过 `LLM` 分类、恒定映射、先于任何 `chunk` 恰一次）、`chunk` 逐条回流、`TaskDone` 后定格 `Show`：`note` 落注位、`examples` 落 `Plain` 疏位、引擎恰被调用 1 次、`attached_image` 保持 `Arc` 同一性 | 2026-10-11 |
| repeated_image_copy_is_a_full_cache_hit | 同图重复复制全缓存命中直出 | 给定同一图像字节（`cache_key` 摘要一致）的第二次复制，当桥查产物缓存命中，则仅回 `TaskClassified(ImageExplain)+TaskDone`（无 `TaskChunk`）、引擎调用数维持 1（首轮仅执行）、状态定格 `Show` | 2026-10-11 |
| hide_overlay_cancels_the_image_stream_and_drops_late_events | 收起浮层取消图像流并丢弃迟到事件 | 给定慢流中的图像任务已收到分类与首个 `chunk`，当 `hide_overlay` 取消在途令牌，则不再有任何回传事件、`attached_image` 清空、机器侧拒绝该任务的迟到 `chunk` 并回 `Idle` | 2026-10-10 |
| classify_reads_the_kind_from_a_bare_kind_script | 分类从裸 JSON `kind` 脚本判型 | 给定 `{"kind":"ExplainCode"}` 裸 JSON 脚本与契约任务脚本（脚本队列两份），当 `classify`，则解析出 `kind`（无提示直通——分类完全交给 `LLM`），全链路引擎恰 2 次调用 | 2026-10-12 |

## 快照测试

`popup` 快照基线：`popup_word_card`、`popup_streaming`、`popup_failed`、`popup_failed_auth`、`popup_selfcheck`、`popup_code_streaming`、`popup_code_outcome`、`popup_image_outcome`、`popup_image_streaming`、`popup_image_failed`、`popup_degraded_outcome`、`popup_long_line`、`popup_unverified_outcome`（2026-10-30 随降级批注方案重录：图标自复制钮前移到复制钮后、形制自警告三角改批注气泡、页脚下留批注卡展开余量）、`popup_violation_hint_hover`（2026-10-30 起：降级批注悬停态基线，真实窗口几何下悬停批注图标录得，行文本由结构化树断言另行锁定）；`popup` 待确认卡基线中英双份：`popup_pending_text_zh/en`、`popup_pending_image_zh/en`（发送前确认是用户手按的关键界面，中文形态一并锁定）；`settings` 快照基线（2026-11-10 起随提示文案分平台分套，文件名带 `.macos`/`.windows` 后缀，`snapshot_name` 按构建目标取套）：`settings_main`、`settings_notice`、`settings_invalid`、`settings_update_up_to_date`、`settings_update_available`、`settings_update_downloading`、`settings_update_ready`、`settings_update_failed_install` 各平台一套（`windows` 套随本拉取请求落库，`macos` 套由系统（macOS）腿产出候选、经差异审查后采纳）；`settings_default_kind_disabled` 单套留存（未分平台前录得，改到跨字段规则版式时同步重录）。其余基线统一英文文案加浅色主题渲染（2026-10-01 起）：`kittest` 自建上下文无（CJK）字体，英文基线可读可审；待确认卡基线（2026-10-27 起）经测试内安装（CJK）字体后中英各录一份，浅色主题同规。

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| word_card_exposes_entries_to_accesskit | 词卡视图的无障碍树结构 | 给定双释义的词卡 `Outcome` 视图，当渲染，则 `AccessKit` 树可按文本定位节点：`gloss`、音标（精确标签），注与疏条目（`markdown` 段落为 `TextRun`，按 `contains` 定位——精确标签查询会被标签提供者过滤规则吞掉） | 2026-10-21 |
| word_card_sections_stack_vertically | 词卡经→疏纵向排布 | 给定词卡视图，当渲染，则经位（字头加音）与疏位（例句）矩形纵向递增（分区显式垂直布局） | 2026-10-04 |
| watermark_sits_inside_the_computed_window_height | 页脚水印落在期望窗口内 | 给定流式、词卡两种视图，当按内容尺寸重设渲染窗口再量水印矩形，则水印底边不超出 `draw` 上交的期望窗口高（页脚被内容挤出窗外即失败） | 2026-10-03 |
| a_long_selection_keeps_the_footer_inside_the_window | 长选区下页脚仍在窗内 | 给定原文很长的流式视图（普通原文与代码面板两态），当先确认 `draw` 上交的完整内容高远超钳制值，再把窗口压到 420（代表壳侧钳高后的高度）渲染，则“正在注疏”与水印底边不超窗高，且下方向键把经位上移而页脚不动（经位与注疏同区滚动，生成指示不被长原文顶出窗外、长原文尾部可达） | 2026-10-27 |
| long_body_is_rendered_in_full | 长正文完整渲染不截断 | 给定超长正文（尾部带标记），当渲染，则 `AccessKit` 树含尾部内容——无字符截断 | 2026-09-20 |
| arrow_keys_scroll_the_overflowing_body | 正文溢出时方向键滚动 | 给定超长正文的 `Outcome` 视图，当按下方向键再按上方向键，则内容视口先下移后回位（无修饰键参与，滚动区到界即停） | 2026-10-12 |
| streaming_view_shows_the_annotating_footer | 流式视图页脚注疏指示 | 给定流式视图，当渲染，则“正在注疏”在页脚出现、经、注印章就位 | 2026-10-27 |
| a_repairing_streaming_view_switches_the_footer_wording | 修复中流式卡页脚切重新注疏 | 给定 `repairing` 的流式视图（正文已清稿、骨架态），当渲染，则页脚为“重新注疏”，英文语境为“`Re-annotating`”且“正在注疏”不在树中（无横幅无计数） | 2026-10-27 |
| word_card_marks_the_three_sections_per_locale | 词卡三分区印章随 `locale` | 给定词卡视图（`zh/en` 各一），当渲染，则印章字分别为 经、注、疏 与 `SRC/NOTE/EXP` | 2026-09-30 |
| streaming_view_shows_only_the_extracted_note | 流式视图只显示提取出的义 | 给定流式视图（正文为 `JSON` 契约原始流），当渲染，则“已流式到达的正文”“选中的原文”可见而（JSON）残片与 `gloss` 围栏均不在树中；头部只有图标与动作区 | 2026-10-03 |
| long_lines_never_exceed_the_window_width | 长行不超窗口可用宽 | 给定长中文段落加围栏代码块与长 `token` 代码原文两类视图，当以 480 宽渲染，则全部内容节点右缘不超窗口宽（横滚不进弹窗）；`popup_long_line` 基线锁定形态 | 2026-10-19 |
| image_card_exposes_the_commentary_to_accesskit | 图像完成态卡经注疏入树且纵排 | 给定附件图像加 `ImageCommentary` 产物（注加两条解读）的完成态视图，当渲染，则注与两条解读皆可按文本定位，且疏位条目矩形在注位之下（经为图、注→疏纵向排布） | 2026-10-10 |
| image_streaming_card_shows_the_interpretation_items_as_they_arrive | 图像流式卡双字段渐进上屏 | 给定 `note` 截断加 `structured.examples` 两条（第二条中段截断）的图像流式视图，当渲染，则注前缀与两条疏条目（含在写条目的已到达前缀）可见而（JSON）残片不在树中（扫描器下钻 `structured` 渐进提取） | 2026-10-11 |
| image_streaming_card_without_a_note_keeps_the_skeleton | 图像流式卡注未至落骨架 | 给定原始流为空的图像流式视图，当渲染，则“正在注疏”页脚在（骨架态，经位图像照常回显） | 2026-10-27 |
| image_card_degrades_to_a_placeholder_when_the_bytes_do_not_decode | 图像字节解码失败占位降级 | 给定不可解码字节的图像完成态卡，当渲染，则经位出“`The image could not be displayed.`”占位、注位照常（不 `panic`） | 2026-10-10 |
| attached_image_texture_is_reused_across_frames | 附件图像纹理跨帧复用 | 给定同一份附件图像字节连续渲染七帧，当比对第 2 帧与第 7 帧的界面库（egui）纹理分配数，则计数不增（同字节恒同纹理，逐帧解码即失败） | 2026-10-10 |
| attached_image_slot_is_dropped_once_an_imageless_frame_renders | 会话离开图像卡即清纹理槽 | 给定图像帧（纹理就位）后切到无附件帧再切回同图，当逐段比对界面库（egui）纹理分配数，则无附件帧分配数下降（源字节与纹理同帧释放，敏感数据不驻留会话外）、同图再上卡重新解码回到原驻留数 | 2026-10-10 |
| degraded_outcome_card_shows_only_the_abnormal_notice_and_retry | 降级产物卡只出异常提示与重试 | 给定 `degraded=true` 且 `note` 为空的完成态卡（格式降级的真实形态），当渲染，则出“`Abnormal output`”与“`Retry`”按钮、点击上交 `OverlayAction::Retry`，而注与疏一概不在树上（原文垃圾不展示不留存） | 2026-10-28 |
| lang_degraded_outcome_card_keeps_content_with_the_footer_badge | 语言降级卡保内容出页脚徽章 | 给定 `degraded=true` 且内容完整的完成态卡（语言降级路径），当渲染，则页脚复制钮右侧出“`For reference only`”批注图标、注与角标照常入树、页脚为复制钮而非重试钮（混用语言的译文对用户仍有价值，批注不占正文首行） | 2026-10-30 |
| degraded_icon_follows_the_copy_button_in_the_footer | 降级批注图标位次随复制钮 | 给定带违约清单的语言降级完成态卡，当渲染，则批注图标紧随复制钮右侧（窄缝间隔，附属信息不占主动作前位）、与复制钮同带居中（图标位次与形制的像素形态由 `popup_unverified_outcome` 基线锁定） | 2026-10-30 |
| hovering_the_degraded_icon_expands_the_violation_rows_or_the_generic_fallback | 悬停批注清单与泛化回落分派 | 给定带违约清单的降级完成态卡，当悬停批注图标，则悬停卡出“`Some language issues remain:`”标题与人读行清单（位置词、规则名、动作）；给定无清单的降级卡（旧缓存条目形态），当悬停，则回落现泛化文案“`Not fully proofread`”且标题不出现（判据在视图层，键与值都不动） | 2026-10-30 |
| the_violation_hint_expands_below_the_footer_inside_the_window | 悬停批注卡向下展开且不超窗 | 给定聚合后五行批注加截断注的最坏行清单（长动作行文），当把窗口压到 `draw` 上交的期望高并悬停批注图标，则悬停卡标题在图标下缘之下展开（不遮经注疏正文）且每一行底边不超窗高（页脚下留批注卡展开余量） | 2026-10-30 |
| degraded_outcome_card_never_takes_the_source_jing_section | 降级产物卡不保留原文经位 | 给定带原文的降级完成态卡，当渲染，则树上无原文节点（降级卡与失败卡同形：只有提示与重试） | 2026-10-11 |
| general_outcome_card_renders_the_plain_layout | 通用产物卡与句译同形 | 给定 `General` 产物（原文经位加 `markdown` 注加两条疏条目）的完成态卡，当渲染，则原文、注文与疏条目逐条入树（条目经 `markdown` 渲染按 `contains` 定位；通用落 `Plain` 变体、按句译同形排布） | 2026-10-21 |
| failed_view_shows_retry_hint | 失败卡重试动作 | 给定 `Retry` 失败卡，当渲染并点击“重试”，则收集器收到 `OverlayAction::Retry` | 2026-09-21 |
| auth_failed_view_offers_open_settings | 鉴权失败卡设置入口 | 给定鉴权失败卡，当渲染并点击“打开设置”，则收到 `OverlayAction::OpenSettings`（头部齿轮标签为“设置”，与正文按钮不混淆） | 2026-09-21 |
| bare_failed_view_has_no_action_button | 无动作失败卡形态 | 给定 `action=None` 失败卡，当渲染，则无“重试”节点、无动作上交 | 2026-09-19 |
| close_button_submits_dismiss | 头部关闭按钮上交收起 | 给定词卡视图（头部为各视图共用路径），当点击无障碍标签“关闭浮层”的关闭钮，则 `draw` 产物为 `OverlayAction::Dismiss` | 2026-09-21 |
| gear_button_submits_open_settings | 头部齿轮上交打开设置 | 给定词卡视图，当点击无障碍标签“设置”的齿轮钮，则 `draw` 产物为 `OverlayAction::OpenSettings` | 2026-09-21 |
| header_drag_strip_is_exposed_to_accesskit | 页头拖动热区进无障碍树且与动作钮零重叠 | 给定词卡视图，当渲染，则无障碍树有“拖动浮层”热区节点、矩形与页头行同高（不小于头部图标边长）且水平区间止于最左动作钮左缘（收 `DRAG_STRIP_INSET`，与齿轮、关闭钮零重叠） | 2026-10-03 |
| header_drag_reports_cumulative_offset_and_ends_on_release | 页头拖动上交自按压点的累计位移 | 给定词卡视图与页头热区中心，当按下→两段移动→松开→再移动，则上交位移自按压点累计（按下为零、两段各 `(20,10)/(30,15)`，非逐帧增量）、松开后不再上交；按压点按物理像素记录（跨 `DPI` 显示器不混尺）；落点换算与屏幕钳制在壳侧 `apply_overlay_drag`（无状态：以窗口实际位置为基准，中途被动过会被下一帧落点吸收） | 2026-10-03 |
| resize_zones_are_exposed_to_accesskit | 缩放热区进无障碍树 | 给定词卡视图，当渲染，则无障碍树有“调整浮层大小”热区节点八段（四角与上下左右边界） | 2026-10-29 |
| corner_drag_anchors_the_press_and_reports_pointer_offsets | 右下角拖拽上交缩放意图 | 给定词卡视图与右下角热区中心，当按下→两段移动→松开→再移动，则按下帧上交 `Press`（南东边界组合、按下时刻窗口尺寸与窗口内指针位置），拖动帧上交 `Drag`（同边界组合、随指针的窗口内位置），松开后粘性上交 `Settle`（壳侧接管宽高，内容自适应退位）；全局换算、锚定与钳制落窗在壳侧 | 2026-10-29 |
| west_edge_drag_reports_the_west_edge_only | 西缘拖拽只携带西边界 | 给定词卡视图与西缘热区中心，当按下→左移，则拖动帧上交的边界组合仅西位真、指针为窗口内坐标 | 2026-10-29 |
| selfcheck_view_exposes_texts_to_accesskit | 自检卡无障碍树 | 给定 `view=None` 的自检渲染，当渲染，则中英文自检文本均可定位 | 2026-09-21 |
| snapshots_match_baseline（popup） | 浮层十七视图渲染基线（英文浅色为主） | 给定十七个视图夹具（词卡、流式、失败、鉴权失败、自检卡、代码流式、代码完成态、图像完成态、图像流式、图像失败、降级完成态、长行，内容夹具为英文；另文本与图像待确认卡中英双份），当 `wgpu` 以对应文案与浅色主题渲染并 `diff`，则与 `popup_word_card`、`popup_streaming`、`popup_failed`、`popup_failed_auth`、`popup_selfcheck`、`popup_code_streaming`、`popup_code_outcome`、`popup_image_outcome`、`popup_image_streaming`、`popup_image_failed`、`popup_degraded_outcome`、`popup_long_line`、`popup_unverified_outcome` 十三份英文基线与 `popup_pending_text_zh/en`、`popup_pending_image_zh/en` 四份待确认卡基线一致，结果合并进单个 `SnapshotResults`。基线沿革：2026-10-01 按经注疏 `demo` 定稿重录：宋楷命名字体族、三印、疏区虚线、常驻页脚带水印；同日随水印槽改实测宽再录，差异仅水印字形位置；2026-10-02 随页头底缘发丝线（与行内容隔 `ITEM` 间距）、关闭钮调小至 10 像素见方与页脚降高 26→20 再录；同日 `popup_word_card` 再随音标改 `gloss-mono` 等宽族重录，差异仅音标字形行——`kittest` 绑内置字形，缺字实测：内置 `Hack` 十八个音标字形缺十四个、`Ubuntu-Light` 缺十三个、`PingFang SC` 缺 `ɒ ʒ ʌ ˈ ˌ ː`，链上无一命中；2026-10-02 新增 `popup_code_streaming`、`popup_code_outcome` 两份代码视图基线（`T3`：`classified` 首帧即代码排版、单层代码面板——代码底色直接覆盖经位、左上语言标签行；无高亮的纯色等宽，高亮属 `T4`；面板样式随用户反馈图样定稿同日再录）；2026-10-03 随热键支持移除、`Acquiring` 骨架视图删除，`popup_loading` 基线一并移除，余八份；2026-10-10 随剪贴板图像卡新增 `popup_image_outcome`、`popup_image_streaming`、`popup_image_failed` 三份基线（图像经位加注疏，夹具为测试内确定性编码的宽 120 高 80 像素渐变位图，英文浅色同规），文本卡八份零变化（图像分支独立、文本流式经位仍在滚动区外）；同日删除 `ImageOcr` 任务类型与 `Extracted` 结构化变体，`popup_extract` 基线随其视图一并移除，余七份；2026-10-11 随契约收敛新增 `popup_degraded_outcome` 降级产物卡基线（异常提示加重试，无经注疏内容），`popup_code_streaming` 随流式代码面板移除语言标签行重录（差异仅该行移除与其下既段位移），余十一份；2026-10-18 随文本流式经位并入滚动区重录 `popup_streaming`、`popup_code_streaming`，差异仅经位整体的亚像素抗锯齿（行高与列位置不变）；2026-10-19 随代码着色后端换 `egui_extras` 与 `syntect`（`two-face` 语法补集加六类色板自定义主题）重录 `popup_code_outcome`；`popup_code_streaming` 因流式期语言未定仍走通用兜底而零变化；`popup_long_line` 是连带变化——`egui_commonmark` 的围栏代码块经 `feature` 统一改用 `syntect` 默认主题与默认语法集，不再是六类色板，表外语言如（TOML）由简易高亮退化为纯色；同日再随疏字号 13→14 重录 `popup_word_card`、`popup_image_outcome`、`popup_image_streaming`（快照画布固定宽 800 高 600，浮层宽度常量不参与基线）；同日再随页头图标改矢量栅格化重录 `popup_*` 全部十二份基线（图标纹理由 `resvg` 按 `pixels_per_point` 从 `SVG` 源直栅到物理像素，不再解码 256 像素 `Dock` 位图降采样，`Retina` 下糊化消除；逐份核对像素差异仅落在 `(23,23)` 至 `(43,43)` 的图标槽位，`settings` 基线不涉及）；2026-10-21 随疏位改 `markdown` 渲染（去弱色改正文色、去列表符与英汉拆行，条目走注文同一条 `markdown` 管线）重录 `popup_word_card`、`popup_image_outcome`、`popup_image_streaming` 三份（差异仅疏位条目行，无疏内容的九份零变化）；2026-10-27 随发送前确认改动：页脚文案“正在注解”改名“正在注疏”、“重新注解”改名“重新注疏”，完成态卡页脚新增“复制注疏”钮（点击后短时翻“已复制”），三印章宽取最宽标签统一，重录七份含页脚或印章的基线（`popup_word_card`、`popup_streaming`、`popup_code_streaming`、`popup_code_outcome`、`popup_image_outcome`、`popup_image_streaming`、`popup_long_line`），其余五份英文基线零变化；同日新增 `popup_pending_text_zh/en`、`popup_pending_image_zh/en` 四份待确认卡基线——页脚为“注疏”主按钮加“取消”，中文形态首次入基线，`kittest` 上下文经测试内安装（CJK）字体后可渲染中文；同日页脚订正后重录八份——待确认四份随确认与取消按钮收回带内垂直居中，按钮原被界面库默认内边距顶出窄带；完成态四份（词卡、代码、图像、长行）随复制图标重画重录，印章圆角恰为方块边长一半、图标被吃成双圆，改专用小圆角档、粗描边与前片遮底；同日按钮组与复制钮相对分割线下移一档再录同八份，随后按钮去窄形态改常规内边距、下移档定为 4.5 像素又再录同八份；2026-10-28 随缩放握把（右下角三道斜纹示意）重录全部十六份基线，逐份核对像素差异仅落在右下角握把区；另三份流式基线（`popup_streaming`、`popup_code_streaming`、`popup_image_streaming`）连带页脚呼吸点的亚像素抗锯齿漂移（每份 36 像素块，幅度低于判容差、肉眼不可辨；成因是调色板结构体增字段的连锁渲染微移，重录实验确认逐字节确定性复现）；2026-10-28 随语言体例门新增 `popup_unverified_outcome` 一份语言降级产物卡基线（泛化“结果未通过校对”徽标加完整注疏内容与复制钮），`popup_degraded_outcome` 随降级夹具改为空注真实形态核对后重录（像素零变化）；同日合入 `main` 随缩放握把重录 `popup_unverified_outcome`（本分支基线录制早于握把改动，`diff` 逐份核对像素差异仅右下角握把区 96 像素）；同日语言降级标记改页脚徽章再录 `popup_unverified_outcome`（正文顶部警示行撤出、页脚左端新增琥珀三角徽章，像素差异落在正文首行与页脚左端）；同日警示提示行文字色收编调色板 `warn` 档再录 `popup_failed`、`popup_failed_auth`、`popup_degraded_outcome`、`popup_image_failed` 四份（像素差异仅提示行文字色）；2026-10-29 随缩放握把撤销重录全部十七份基线（逐份核对像素差异仅右下角握把区 96 像素） | 2026-10-29 |
| all_sections_render_and_save_submits_the_draft | 设置窗渲染与保存提交 | 给定默认配置的设置窗口，当渲染并点保存，则各区块控件可定位且上交未改动的出厂快照 | 2026-09-19 |
| cancel_and_clear_key_actions_are_submitted | 取消与清除密钥动作 | 给定“取消”与“清除密钥”按钮，当分别点击，则取消上交 `Close`、清除只置标记（按钮变“撤销清除”）、保存时才上交 `Clear` | 2026-09-19 |
| invalid_save_is_blocked_with_field_hints | 非法草稿保存被阻断并就地提示 | 给定非法 `Base URL` 的设置窗，当点保存，则不上交 `Save`、字段就地标红并出汇总行 | 2026-09-22 |
| snapshots_match_baseline（settings） | 设置窗渲染基线（英文浅色；正常、提示、错误三态加更新区五相位） | 给定默认、带保存失败提示、校验错误三个状态与更新区五个相位 `UpToDate`、`Available`、`Downloading`、`Ready`、`Failed(Install)`，当 `wgpu` 以英文文案与浅色主题渲染并 `diff`，则与对应基线一致且关键文本进树。沿革：2026-10-03 随热键区删除、热键校验错误移除重录；同日随职责重划删除任务开关、默认任务、每类模型三区块，新增单一模型输入行重录并移除默认任务停用态，又随“模型 `ID`”字段标签再录；2026-10-10 通用区新增“监听剪贴板图片”开关行与说明随录，八份基线 `diff` 均仅为该行插入与其下内容的既段位移，其余像素零变化；同日评审修复把初版勾选框改为自绘开关滑块（`egui 0.36` 无内建 `Switch`，按官方 `demo` 范式，轨道宽 36 高 20 较勾选框高约 2 像素）再录，`diff` 仍仅限该行控件本体与其下既段位移；2026-10-28 随词条语言规范门禁把监听剪贴板提示的英文弯引号改直引号重录八份，像素差异仅该提示行文字；2026-11-03 随密钥行改右到左等宽排布（清除按钮贴右缘，输入框加按钮总宽与服务地址框对齐）重录八份，像素差异仅密钥行 | 2026-11-03 |
| failure_card_words_each_cause | 失败卡按变体出文案 | 给定网络失败、JSON 模式不支持、协议异常（带诊断）、推理通道不可用四种失败起因，当渲染，则各出对应文案（协议异常保留诊断文本） | 2026-10-11 |
| failure_card_follows_the_locale | 失败卡随 `locale` 出表 | 给定英文 `locale` 的网络失败卡，当渲染，则出英文文案与英文“`Retry`”动作 | 2026-09-22 |
| save_failure_notice_names_the_cause | 保存失败提示出场合与诊断 | 给定带 `SaveFailed(Config)` 类型化提示的设置窗（中文表），当渲染，则出“保存失败：诊断文本”（前缀交代场合、诊断不重复本地化整句） | 2026-09-22 |
| a_rendered_notice_follows_the_locale | 提示行随 `locale` 出表 | 给定带同一类型化提示的设置窗（英文表），当渲染，则出英文前缀与诊断（提示行不是只有中文基线可查） | 2026-09-22 |
| saving_the_language_swaps_the_rendered_labels_without_a_restart | 保存语言即换渲染文案 | 给定同一进程内保存 `Language::En` 前后的配置句柄，当各渲染一帧设置窗，则文案由“保存”变为“`Save`”且中文标不再在树上（配置快照 → 落定 → 选表 → 渲染全链，不重启） | 2026-09-22 |
| english_catalog_relabels_the_settings_window | 英文文案表驱动设置窗 | 给定英文 `locale` 的设置窗，当渲染，则区块标、动作按钮、目标语言、清除密钥、界面语言、界面主题、缓存有效期、监听剪贴板图片等行标各出自英文表，且中文标不在树上 | 2026-10-10 |

## 单元测试

### `crates/gloss-app/src/app/handler.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| escape_press_is_the_dismiss_key | 浮层收起键判定 | 给定逻辑键与按下状态，当判定收起键，则 `Escape` 按下为真、释放与其它字符键为假 | 2026-09-21 |
| sooner_picks_the_earliest_deadline | 取更早的截止时刻 | 给定两个时刻（可含 `None`），当取更早，则 `None` 让位、双 `None` 不唤醒 | 2026-09-19 |
| due_deadline_with_visible_overlay_requests_the_frame | 到点截止在浮层可见时照常请求重绘 | 给定已到期的截止与可见浮层，当处置到点截止，则返回请求重绘且截止保留待新帧刷新 | 2026-10-16 |
| due_deadline_with_hidden_overlay_is_cleared_without_a_frame_request | 到点截止在浮层隐藏时随丢弃清除 | 给定已到期的截止与隐藏浮层，当处置到点截止，则不请求重绘且截止被清为 `None` | 2026-10-16 |
| pending_deadline_is_left_untouched | 未到期截止不处置 | 给定未到期的截止与可见、隐藏两种浮层，当处置到点截止，则均不请求重绘且截止原样保留 | 2026-10-16 |
| missing_deadline_is_never_due | 无截止不触发处置 | 给定 `None` 截止，当处置到点截止，则可见与隐藏均不请求重绘且仍为 `None` | 2026-10-16 |

### `crates/gloss-app/src/app/mod.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| ui_locale_follows_the_saved_language_without_a_restart | 界面语言随保存即时切换 | 给定出厂配置（跟随系统），当保存 `Language::En` 再保存 `Language::System`，则逐帧解析出的 `locale` 依次为 `Zh`→`En`→`Zh`（不重启即换文案表） | 2026-09-22 |

### `crates/gloss-app/src/channel.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| platform_events_round_trip_through_crossbeam | 通道一事件往返 | 给定四种 `PlatformEvent`（含粘贴板图片观察与授权落定），当经通道一往返，则按序原样到达 | 2026-10-10 |
| acquire_commands_carry_app_assigned_gen | 取材命令携带代数 | 给定带代数的两种取材命令（读文本、读剪贴板图），当下发，则接收侧读到同一代数与各自的载荷 | 2026-10-10 |
| run_task_command_delivers_cancellable_task | 任务命令携带取消令牌 | 给定 `RunTask` 命令，当下发，则代数、任务、取消令牌完整到达且令牌联动 | 2026-09-19 |
| prewarm_secret_command_carries_only_the_entry_id | 预热命令只携带条目定位符 | 给定 `PrewarmSecret` 命令，当下发，则到达的命令只含 `keychain_id`（密钥值不进通道载荷） | 2026-10-06 |
| secret_prewarmed_receipt_round_trips_without_a_value | 预热回执成功失败两路往返 | 给定 `Ok` 与 `Err` 两条预热回执，当经通道四，则按序携带（`Err` 只含失败原因，不含密钥值） | 2026-10-06 |
| event_channel_supports_dual_senders | 事件通道双发送端 | 给定克隆出的第二 `Sender`，当两端各发一条，则主线程按到达顺序都收到 | 2026-09-19 |
| event_channel_carries_stream_chunks_and_failures | 通道四携带流块、修复标记与失败 | 给定 `TaskChunk`、`TaskRepairing` 与 `TaskFailed`，当经通道四，则按序携带（修复标记不带轮次） | 2026-10-12 |
| try_recv_on_empty_queue_returns_empty | 空队列 `try_recv` 返回 `Empty` | 给定空队列，当 `try_recv`，则返回 `Empty` | 2026-09-19 |
| try_recv_after_senders_dropped_reports_disconnected | 发送端全关报 `Disconnected` | 给定全部 `Sender` `drop`，当先收完余量再 `try_recv`，则报 `Disconnected` | 2026-09-19 |
| channels_bundles_all_four | 四通道捆绑互不串扰 | 给定 `Channels::new`，当四条通道各发一条，则各自到达、互不串扰 | 2026-09-19 |
| event_sender_clone_is_independent | 克隆 `Sender` 独立存活 | 给定克隆 `Sender` 且原型 `drop`，当用它发送，则消息照常到达 | 2026-09-19 |

### `crates/gloss-app/src/flow/actions.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| retry_action_redispatches_the_failed_task | `Retry` 动作重发失败任务 | 给定失败卡 `Retry` 动作，当执行，则同代数同任务新令牌重发通道三，重试产物照常采纳 | 2026-09-21 |
| open_settings_action_keeps_the_error_card | 打开设置保留错误卡 | 给定鉴权失败卡，当执行 `OpenSettings` 动作，则停在 `Error`、通道三无流量、编辑会话就位 | 2026-09-21 |
| confirm_action_dispatches_the_pending_card | 确认动作下发待确认卡 | 给定落定待确认卡的会话，当执行 `Confirm` 动作，则进 `Translating`、通道三收到当代数的 `RunTask`（确认点击前通道三零流量） | 2026-10-27 |
| stale_confirm_clicks_dispatch_nothing | 陈旧确认点击不下发 | 给定无卡时的 `Confirm` 点击与已消费卡上的第二次确认，则前者无卡不下发、后者被拒（通道三只收到第一次的 `RunTask`） | 2026-10-27 |

### `crates/gloss-app/src/flow/probe.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| dispatched_acquire_carries_the_task_span | 取材命令带着任务 `span` 下发 | 给定划词触发，当取出通道二载荷并进入它的 `span`，则探针日志行是 JSON 且带 `"generation":1` | 2026-09-23 |
| a_disabled_default_kind_does_not_stop_the_selection_gesture | 任务开关不拦划词手势 | 给定默认任务被停用的配置，当划词触发，则仍下发 `Auto` 取材命令（手势不带显式意图，开关只拦显式 `kind`） | 2026-09-26 |
| a_sensitive_scene_makes_the_selection_gesture_a_no_op | 敏感场景下划词彻底无声 | 给定安全输入开启、再给定前台应用在拦截名单内（两侧各自设置），当划词触发，则取材命令都不下发、探测编号不领、状态留 `Idle`；场景恢复后同一手势照常下发 | 2026-09-24 |
| a_pasteboard_observation_dispatches_the_clipboard_acquire_command | 粘贴板观察下发剪贴板读图命令 | 给定粘贴板图片观察事件，当消费通道一，则状态机留 `Idle`、只领探测编号（`id=1`、代数 0）、通道二收到 `AcquireClipboardImage{1}`、不记录划词锚点（露面走居中） | 2026-10-10 |
| a_sensitive_scene_silences_the_pasteboard_observation | 敏感场景下粘贴板观察无声 | 给定安全输入开启、再给定前台应用在拦截名单内（两侧各自设置），当粘贴板观察触发，则读图命令不下发、探测编号不领、状态留 `Idle`；场景恢复后同一观察照常下发 | 2026-10-10 |

### `crates/gloss-app/src/flow/reveal.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| dismiss_abandons_the_inflight_task_and_returns_to_idle | 收起浮层放弃在途任务 | 给定推理中的浮层，当执行收起出口，则回 `Idle`、在途令牌取消、视图清空、迟到产物被丢弃 | 2026-09-21 |
| show_position_follows_selection_only_for_the_current_generation | 显示位置跟随当代划词 | 给定划词锚点与代数，当决策显示位置，则当代跟随选区（右下偏移）、代数不符或无锚点回落居中 | 2026-09-20 |
| selection_trigger_records_its_anchor_per_generation | 划词触发按代数记锚点 | 给定两次划词触发，当消费，则锚点随代数刷新为各自释放坐标 | 2026-09-20 |

### `crates/gloss-app/src/flow/settings_session.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| open_settings_request_starts_an_edit_session | 打开设置即编辑会话 | 给定 `OpenSettingsRequested`，当消费，则编辑会话打开、草稿取当前快照、不占代数 | 2026-09-19 |
| settings_save_writes_keychain_and_swaps_config | 保存写密钥串并换配置 | 给定含密钥替换的保存，当成功，则密钥进 `keychain`、快照换新、会话关闭，新配置随后续触发生效 | 2026-09-26 |
| settings_save_hot_toggles_the_clipboard_watch_switch | 保存热切换剪贴板哨兵开关 | 给定出厂配置（哨兵开）的两次保存，当先关再开“监听剪贴板图片”，则运行时快照与共享开关位同步复位、置位（不重启即生效） | 2026-10-12 |
| clearing_the_key_deletes_the_secret_on_save | 清除密钥保存即删除 | 给定 `KeyUpdate::Clear`，当保存，则 `keychain` 条目删除、会话关闭 | 2026-09-19 |
| failed_save_keeps_the_session_open_with_a_notice | 失败保存会话不关 | 给定落盘必失败存储，当保存，则会话保持打开、错误进提示、快照不变、哨兵开关位不动 | 2026-10-10 |

### `crates/gloss-app/src/flow/task.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| late_events_of_superseded_trigger_do_not_bleed | 旧会话的迟到事件不渗漏 | 给定划词提交 `A` 在推理中、再给一次新划词探测，当探测期间 `A` 的流照常推进，随后提交 `B`，则提交才取消 `A`、`A` 的迟到 `chunk/TaskDone` 被陈旧过滤、`B` 的产物照常 `Show` | 2026-10-01 |
| probe_empty_selection_is_silently_dropped | 壳层时序：误滑静默丢弃 | 给定划词探测（状态机不动），当收空选区失败，则壳不请求浮层、状态机与视图原样、探测消费 | 2026-10-03 |
| failed_task_lands_in_error_and_retry_works | 失败落错误态且可再划词 | 给定推理中任务，当匹配代数的失败到达，则落 `Error`；陈旧失败丢弃；再次划词探测不动失败卡（探测编号在场） | 2026-10-01 |
| stale_input_ready_is_dropped_entirely | 陈旧 `InputReady` 整体丢弃 | 给定陈旧编号 `InputReady`，当采纳，则整体丢弃、不下发通道三 | 2026-09-19 |
| saved_config_applies_to_the_next_trigger | 新配置对下次触发生效 | 给定保存新配置（目标语言与模型），当下一次划词提交，则新值随任务选项冻结下发（模型与语言均出自探测时快照；`kind` 由 `LLM` 层分类） | 2026-10-03 |
| saved_config_does_not_leak_into_the_inflight_task | 在途任务用触发时快照 | 给定探测后、产物到达前保存新配置，当产物提交下发，则仍用探测时快照 | 2026-09-19 |
| suspicious_input_is_suppressed_and_shows_nothing | 可疑内容被拦下且什么都不出 | 给定嵌在（JSON）里的短令牌取材产物，当采纳，则通道三一条都没有、可见会话不被触碰（没有卡片、没有可点的出口）；下一次普通取材照常下发 | 2026-10-01 |
| a_mis_slide_over_a_visible_session_preserves_it_entirely | 已显示会话对误滑零感知 | 给定推理中的可见会话，当新划词探测以空选区失败收场，则令牌未取消、状态与视图原样、无显形挂起、流式正文照常追加 | 2026-10-01 |
| committing_the_probe_flags_the_reveal_for_the_same_frame | 提交即挂起显形 | 给定在途划词探测，当产物提交，则置位显形挂起（`drain_events` 同帧消费）、落 `PendingConfirm` 待确认卡 | 2026-10-27 |
| pasteboard_commit_retains_the_image_and_flags_the_reveal | 粘贴板提交留图落卡并挂起显形 | 给定在途粘贴板探测，当图像产物提交，则落 `PendingConfirm`、图像以 `Arc` 留存进 `attached_image`、置位显形挂起（无匹配代数的锚点——露面走居中）、通道三零流量；当确认点击，则通道三收到携带 `TaskInput::Image` 的 `RunTask`、视图换流式卡 | 2026-10-27 |
| the_pending_card_cancel_path_sends_no_channel3_message | 待确认卡取消路径零通道三消息 | 给定落定待确认卡的会话，当经取消按钮路径收起浮层，则回 `Idle`、通道三一条命令都没有（收起不惊动推理侧）、迟到的同编号产物无人采纳 | 2026-10-27 |
| a_pasteboard_product_without_a_matching_probe_is_dropped | 无匹配探测的图像产物整体丢弃 | 给定陈旧编号的图像 `InputReady`（无在途探测），当采纳，则整体丢弃、不下发通道三、不留随行图像 | 2026-10-10 |
| pasteboard_oversized_failure_lands_in_the_error_card | 图片超上限失败落错误卡 | 给定在途粘贴板探测，当收 `ImageTooLarge` 失败，则落 `Error` 弹失败卡（显式可观察）、探测消费 | 2026-10-10 |
| a_repair_marker_resets_the_stream_through_the_shell | 壳层采纳修复轮开始标记 | 给定推理中的流式会话（已有违约累积），当壳 `accept_repairing`，则当前代清空累积并置修复态、陈旧代拒绝且不扰动在途会话 | 2026-10-12 |
| a_cached_selection_skips_confirmation_and_lands_the_outcome | 缓存命中的选区免确认直落产物卡 | 给定按探测同款选项预热壳侧任务缓存的会话，当划词产物提交，则直落产物卡（`Show`、通道三零流量、同帧置显形挂起）；紧随一次未缓存划词照旧落待确认卡并经确认下发（跳过确认的前提是亲眼命中，未命中把关原样在岗） | 2026-11-04 |
| a_cached_pasteboard_image_skips_confirmation_too | 缓存命中的剪贴板图片同样免确认 | 给定按图像输入预热的任务缓存与在途粘贴板探测，当图像产物提交，则直落产物卡（`Show`）、通道三零流量（免确认是两模态同权） | 2026-11-04 |
| a_selection_after_a_settled_image_task_still_flags_the_reveal | 图片任务落定后划词照常出弹窗 | 给定已落定产物卡（`Show`）的图片任务，当一次新划词提交，则落待确认卡且同帧置显形挂起（图片任务不清空后续划词的露面）、确认后按文本任务下发通道三 | 2026-11-10 |

### `crates/gloss-app/src/flow/task.rs`（预热发送端）

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| prewarm_dispatches_the_configured_keychain_entry_exactly_once | 预热命令携带当前配置条目且逐次下发 | 给定默认配置的驱动器，当连续两次 `send_secret_prewarm`，则通道三各收到一条 `PrewarmSecret` 且 `keychain_id` 取自当前快照（共两条） | 2026-10-06 |
| prewarm_without_endpoints_is_a_silent_no_op | 无端点时预热静默跳过 | 给定 `endpoints` 已清空的驱动器，当 `send_secret_prewarm`，则不发任何命令、不 `panic` | 2026-10-06 |
| prewarm_is_deferred_when_startup_permissions_are_not_ready | 启动权限未就绪时预热推迟 | 给定启动权限预检结论为未就绪的驱动器，当 `send_secret_prewarm`，则不发任何命令（`keychain` 授权框不得抢在系统权限引导弹窗之前，读密推迟到首次任务） | 2026-10-08 |
| accessibility_grant_event_unblocks_and_dispatches_deferred_prewarm | 授权落定解门控补发预热 | 给定启动权限未就绪的驱动器，当 `drain` 收到 `AccessibilityGranted`，则预热门控置位且通道三补发一条 `PrewarmSecret`；重复事件不再补发 | 2026-10-09 |
| accessibility_grant_event_is_a_noop_when_already_ready | 已就绪时授权事件无操作 | 给定启动权限已就绪的驱动器，当 `drain` 收到 `AccessibilityGranted`，则不发任何命令 | 2026-10-09 |

### `crates/gloss-app/src/machine.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| selection_probe_takes_an_id_without_touching_the_generation | 划词探测只领编号不动代数 | 给定划词来源，当 `begin_probe`，则发 `AcquireText`（无 `kind`——类型归 `LLM` 层）且只领探测编号不动代数、在途探测停驻等产物。沿革：原名“探测映射只覆盖已接线事件”，2026-11-12 两探测入口收为 `begin_probe` 后事件接线映射上移编排层，未接线断言随之由 `trigger_decision` 的 `Unwired` 分支测试承载 | 2026-11-12 |
| trigger_decision_separates_blocked_and_unwired_events | 触发去向分出被拦、未接线 | 给定 `trigger_decision`，则划词恒为 `Acquire`（无载荷——分类是 `LLM` 层的事）、剪贴板观察为 `Acquire`（取材动作即显式意图）且只受场景闸门（名单内报 `Blocked`、自身前台不设防）、设置与授权落定报 `Unwired`；出厂配置下划词为 `Acquire` | 2026-10-10 |
| scene_gate_stops_the_probe_before_acquisition | 场景闸门在取材前停住探测 | 给定安全输入开启（前台应用不在名单内）、再给定“前台应用在名单内且安全输入关闭”，当 `begin_probe`，则两次都 `None`、无探测编号、状态留 `Idle`；场景恢复后同一手势照常探测（仍不动代数） | 2026-11-12 |
| selection_options_pair_with_one_snapshot_including_the_model | 选项（含模型）出自同一快照 | 给定自定义配置快照，当划词探测并提交产物，则目标语言与模型 `id` 均出自快照冻结（类型不在其中——`kind` 由 `LLM` 层分类决定） | 2026-10-03 |
| classified_kind_updates_the_streaming_chip_only_once_current | 分类结果只更新当前代的流式标签 | 给定推理中的流式视图，当 `accept_classified`，则当前代写入判定 `kind`、陈旧代与已定格产物卡拒绝、无流式视图不采纳 | 2026-09-26 |
| a_degraded_done_keeps_the_request_and_retry_redispatches_it | 降级产物保留请求副本且可重试 | 给定推理中任务收 `degraded` 产物，当 `accept_done`，则落 `Show` 定格降级卡、清空取消令牌且不清 `active_request`；当 `retry`，则同代数按原 `input+options` 重发、换新令牌并回流式视图；正常产物落 `Show` 即清请求（`retry` 返回 `None`） | 2026-10-18 |
| the_repair_marker_clears_the_stream_and_refuses_stale_rounds | 修复轮标记清稿并拒陈旧轮次 | 给定推理中的流式会话，当 `accept_repairing`，则当前代清空上一轮累积并置 `repairing`（修复轮内 `chunk` 照常追加、页脚保持修复态）；陈旧代拒绝且原稿保留、已定格产物卡拒绝、新会话接受自己的标记 | 2026-10-12 |
| options_freeze_at_probe_time | 选项在探测时刻冻结 | 给定探测后更换配置（目标语言与界面语言同时变），当提交产物，则任务仍带探测时快照的选项（含 `prompt_locale`）；第二次探测才用新值 | 2026-10-01 |
| prompt_locale_follows_config_language_and_the_system | 任务选项落定模板语言 | 给定显式 `Language::En` 与出厂 `System` 两份配置，当探测并提交产物，则任务携带的 `prompt_locale` 分别为 `En` 与注入的系统语言 | 2026-09-22 |
| commit_probe_promotes_the_generation_and_supersedes_the_previous_session | 提交探测接管会话并守卫状态 | 给定命中在途探测的合法产物，当确认后 `commit_probe`，则探测编号提升为代数、进 `Translating`、领到取消令牌、探测消费；再给新探测并提交，则落 `PendingConfirm` 待确认卡、旧在途任务令牌被取消（让位语义不变）、视图换持冻结输入的待确认卡；同编号重复提交被拒 | 2026-10-27 |
| commit_lands_the_pending_card_and_confirm_dispatches_the_task | 提交落待确认卡且确认下发 | 给定命中在途探测的合法产物，当 `commit_probe`，则落 `PendingConfirm` 待确认卡、编号提升为代数、无取消令牌（待确认阶段无在途推理、通道三零流量）、探测消费、视图持冻结输入；当 `confirm_pending`，则返回下发请求进 `Translating`、领到新令牌、视图换流式卡、代数不因确认而动；已消费的待确认卡拒收重复确认 | 2026-10-27 |
| confirm_pending_carries_the_frozen_options_snapshot | 确认下发携带探测时冻结选项 | 给定自定义配置的探测产物提交，当 `confirm_pending`，则下发请求的模型与目标语言均为探测时刻快照值（确认不回读配置；下一次探测才用新值） | 2026-10-27 |
| confirm_pending_is_refused_outside_the_pending_card | 确认守卫只认待确认卡 | 给定空白机器、流式中的会话、已定格产物卡三种非待确认状态，当 `confirm_pending`，则一律 `None`（无卡无确认出口；已定格代数的迟到失败一并被拒） | 2026-10-27 |
| hide_drops_the_pending_card_without_a_token_to_cancel | 隐藏丢弃待确认卡且无需取消 | 给定待确认卡在场，当 `hide_overlay`，则回 `Idle`、视图清空、确认出口一并失效（收起后的确认不能复活成下发） | 2026-10-27 |
| a_new_commit_supersedes_the_pending_card_and_confirm_acts_on_the_current_card | 新提交顶掉待确认卡且确认只认当前卡 | 给定待确认卡在场时新探测提交，则新卡接管；当确认，则恒作用于屏上当前卡——下发新卡的输入与代数，同一张卡只可确认一次（已消费卡的再次确认被拒）。沿革：原名“陈旧确认被拒”，2026-10-27 随确认语义核实修订——被顶掉卡的确认从未被尝试，第二次拒绝的真实守卫是卡已消费、视图不再是待确认卡 | 2026-10-27 |
| a_cache_hit_settles_the_pending_card_straight_to_the_outcome | 缓存命中待确认卡直落产物卡 | 给定落定待确认卡的会话，当 `settle_cached` 带缓存产物，则整卡定格为产物卡（原文与缓存产物同卡）直达 `Show`、无取消令牌无重试出口、代数不动；已定格的卡拒收第二次定格（缓存命中免确认，2026-11-04） | 2026-11-04 |
| a_cached_degraded_hit_keeps_the_request_copy_for_retry | 缓存命中的降级产物保留重试 | 给定落定待确认卡的会话，当 `settle_cached` 带降级缓存产物，则产物卡上 `retry` 可原样重发该输入并回流式视图（与 `accept_done` 的降级口径同款） | 2026-11-04 |
| cached_settle_only_accepts_the_pending_card | 缓存定格守卫只认待确认卡 | 给定空白机器、流式中的会话、已定格产物卡三种非待确认状态，当 `settle_cached`，则一律 `false` 且流式会话原样无损（实现红线：先判形再取视图——先取后判会把流式会话整卡吞掉，此测试即该缺陷的回归钉） | 2026-11-04 |
| a_cached_pasteboard_hit_settles_with_the_image_retained | 缓存命中的图像提交定格保图 | 给定落定待确认卡的图像会话，当 `settle_cached`，则直落产物卡、经位原文恒空串（经是图本身）、`attached_image` 以 `Arc` 指针不变随行 | 2026-11-04 |
| a_probe_leaves_a_visible_session_completely_untouched | 探测与误滑不扰动可见会话 | 给定推理中的可见会话，当新划词探测、再当探测以空选区失败收场，则状态、视图、取消令牌全程原样，流式正文照常追加 | 2026-10-01 |
| image_input_for_text_kind_is_rejected | 划词探测拒收图像输入 | 给定在途划词探测收图像产物，当提交，则 `Ignored`（模态按探测的取材来源配对） | 2026-10-10 |
| hide_abandons_inflight_and_drops_late_events | 隐藏放弃在途并拒迟到事件 | 给定 `Translating` 态隐藏，当收起，则令牌取消、视图清空回 `Idle`，迟到同代数产物、失败被拒 | 2026-09-19 |
| hide_overlay_drops_the_outstanding_probe | 隐藏作废在途探测 | 给定在途探测（划词与粘贴板各验一遍），当收起浮层，则迟到的探测产物与失败均被拒（不得把浮层弹回） | 2026-10-10 |
| pasteboard_probe_takes_an_id_without_touching_the_session | 粘贴板探测只领编号不动会话 | 给定空白机器与推理中的可见会话，当 `begin_probe`，则发 `AcquireClipboardImage`、只领探测编号不动代数、状态与视图原样、在途任务不被取消、无随行图像 | 2026-11-12 |
| the_scene_gate_stops_the_pasteboard_probe_too | 场景闸门同样停住粘贴板探测 | 给定安全输入开启与前台应用在名单内两种场景，当 `begin_probe`，则两次都 `None`、无探测编号、状态留 `Idle`；场景恢复后同一观察照常探测（仍不动代数） | 2026-11-12 |
| pasteboard_commit_retains_the_image_and_parks_the_pending_card | 粘贴板提交留图落待确认卡 | 给定在途粘贴板探测的图像产物，当 `commit_probe`，则落 `PendingConfirm`、探测编号提升为代数、图像以 `Arc` 零拷贝留存进 `attached_image`、无取消令牌（待确认阶段无在途推理）；当确认，则下发请求携带同一图像输入、进 `Translating`、流式卡经位原文为空（经是图本身） | 2026-10-27 |
| probes_are_mutually_exclusive_across_triggers | 两路探测互斥（同一探测槽） | 给定划词探测在途时来粘贴板观察（反向亦然），当新探测到达，则顶掉旧探测；旧编号的产物与失败一律 `Ignored`、新编号照常提交 | 2026-10-10 |
| text_on_a_pasteboard_probe_is_ignored_without_consuming | 粘贴板探测拒收文本不消费探测 | 给定在途粘贴板探测收文本产物，当 `commit_probe`，则 `Ignored`；同编号的合法图像产物随后仍可提交 | 2026-10-10 |
| pasteboard_commit_supersedes_and_a_new_task_clears_the_image | 粘贴板提交接管且新任务清图 | 给定推理中的文本任务，当粘贴板提交，则旧任务取消、随行图像就位；随后新文本任务提交，则 `attached_image` 清空 | 2026-10-10 |
| hide_overlay_clears_the_attached_image | 隐藏清空随行图像 | 给定已提交图像任务的会话，当 `hide_overlay`，则 `attached_image` 清空 | 2026-10-10 |
| image_too_large_failure_lands_in_the_failure_card | 图片超上限失败落失败卡 | 给定在途粘贴板探测，当收 `ImageTooLarge`，则 `Shown` 落 `Error` 弹失败卡（无动作按钮——超限没有按钮意义上的出口、`retry` 为 `None`）、探测消费、无随行图像 | 2026-10-10 |
| retrying_a_failed_image_task_keeps_the_attached_image | 图像任务失败重试保住随行图像 | 给定图像任务流中失败落 `Error`，当 `retry`，则同代数原样重发（`input` 含同一图像 `Arc`、新令牌）、`attached_image` 指针不变、回流式卡（经位原文为空） | 2026-10-10 |
| the_streaming_view_waits_unclassified_until_the_llm_layer_reports | 流式视图等大语言模型（LLM）层分类精化 | 给定划词提交，当检查流式视图，则 `classified` 创建为 `None`（等 `LLM` 层回传；语言不上流式视图——唯一来源是产物 `structured.code_language`） | 2026-10-11 |
| failed_guard_matches_translating_only | 失败守卫只认推理在途态 | 给定推理中任务的失败、以及隐藏后的迟到失败，当采纳，则前者落 `Error`（`Shown`）、后者被拒（`Ignored`） | 2026-10-03 |
| probe_no_selection_failures_are_silently_dropped | 探测空选区静默丢弃 | 给定在途划词探测，当收 `SelectionUnavailable`、`SelectionEmpty` 失败，则静默丢弃不弹卡，状态机与当前显示一律不动、探测编号消费 | 2026-10-01 |
| probe_permission_failures_still_raise_the_card | 探测权限失败仍弹卡 | 给定在途划词探测，当收 `AccessibilityDenied` 失败，则接管会话落 `Error` 弹失败卡（真实故障需要显式反馈） | 2026-10-01 |
| selection_failures_outside_the_probe_still_raise_the_card | 推理期取材类失败仍弹卡 | 给定划词提交已进推理态，当收 `SelectionUnavailable`，则落 `Error` 弹卡（静默只覆盖探测一腿） | 2026-10-01 |
| stale_probe_results_are_dropped | 陈旧探测产物整体丢弃 | 给定新探测替换旧探测，当旧编号的产物、失败到达，则一律 `Ignored` | 2026-10-01 |
| probe_in_the_self_frontmost_scene_is_suppressed_without_an_id | 自身前台的划词被拦且不占编号 | 给定前台应用 `is_self` 为真，当手势 `trigger_decision`，则 `SelfSuppressed`、探测为 `None`、无浮层 | 2026-10-03 |
| modality_mismatch_preserves_pending_options | 模态错配保留待定选项 | 给定模态错配被拒后，当同编号合法产物到达，则仍可按冻结选项提交 | 2026-10-03 |
| transport_failure_lands_in_error | 传输失败落错误态 | 给定推理通道不可用，当 `fail_transport`，则落 `Error`、失败视图无动作按钮、`retry` 为 `None` | 2026-09-19 |
| retryable_failure_keeps_request_and_retry_redispatches_it | 可重试失败保留请求 | 给定网络类失败，当落 `Error`，则失败原因按变体记录（`FailureCause::Task(EngineNetwork)`）、`retry` 同代数按原 `input+options` 新令牌重发且回流式视图（不回读配置） | 2026-10-03 |
| error_actions_follow_the_mapping_table | 错误动作按映射表 | 给定限流、鉴权、模态、配置、JSON 模式不支持类失败，当映射，则限流可重试，其余引导打开设置且不可重试（`JSON` 模式不支持重发同请求无益） | 2026-10-11 |
| new_commit_and_hide_supersede_the_retry_request | 新提交与隐藏取代重试 | 给定失败卡在场时新探测，则 `retry` 仍在（误滑不得杀掉重试出口）；当提交产物或隐藏，则 `retry` 返回 `None` | 2026-10-03 |
| suspicious_input_is_dropped_while_the_visible_session_survives | 可疑内容丢弃且当前显示保留 | 给定可见会话（产物卡在场）时到达带令牌的探测产物，当提交，则结果是 `Blocked{Token}`、探测消费、可见会话的状态与视图原样保留、已完结会话的令牌不被取消 | 2026-10-01 |
| blocked_input_is_not_redispatched_by_any_later_path | 被拦下的取材没有旁路 | 给定已拦下的探测产物（卡号命中），当 `retry`、同编号重复提交、同编号 `chunk/done/failed`、以及同编号通道故障（`fail_acquire`、`fail_transport`）陆续到达，则全部被拒且状态机不被触碰；新探测后同一份可疑文本仍被拦下 | 2026-10-01 |
| ordinary_input_still_passes_the_content_gate | 日常文本照常通过内容闸门 | 给定一段普通中文，当提交，则照常 `Dispatch` 并进 `Translating`（闸门只认高置信度模式） | 2026-09-24 |
| auto_show_policy_decides_when_the_overlay_pops | 自动弹出按事件类别 | 给定事件类别与采纳的组合，当逐事件判定，则失败即弹；取材成功不再露面（显形随划词提交置位）、完成与 `chunk` 不弹（浮层已可见）；未采纳一律不弹 | 2026-09-26 |
| auto_show_survives_a_mixed_batch | 混合批次自动弹出取或 | 给定一批混合回传，当按批取或，则任一被采纳的失败即弹、整批陈旧不弹、空批不弹 | 2026-09-26 |
| pending_reveal_shows_only_over_a_live_view | 挂起显形的守卫只认活视图 | 给定挂起的显形请求与机器视图有无两种状态，当判显形，则视图在场即显形、视图为空不显形（弹出渲染自检卡）、无挂起且批次无失败即弹也不显形 | 2026-10-03 |
| reveal_decision_combines_the_pending_request_with_the_batch | 显形决策对挂起请求与批次取或 | 给定挂起显形请求配整批陈旧回传、无挂起配被采纳的失败、无挂起配取材成功与流式增量，当判显形，则挂起显形不依赖批次（陈旧批也拦不下）、失败即弹独立成立、取材成功与流式增量不负责露面 | 2026-09-27 |

### `crates/gloss-app/src/present/gpu.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| alpha_mode_prefers_premultiplied | `alpha` 模式优先 `PreMultiplied` | 给定含 `PreMultiplied` 的候选，当挑选，则选它 | 2026-09-19 |
| alpha_mode_falls_back_to_postmultiplied | `alpha` 模式回退 `PostMultiplied` | 给定缺 `PreMultiplied` 的候选，当挑选，则回退 `PostMultiplied` | 2026-09-19 |
| alpha_mode_degrades_to_auto_without_transparency | 无透明支持降级 `Auto` | 给定仅 `Opaque/Auto` 的候选或空列表，当挑选，则降级 `Auto` | 2026-09-19 |
| errors_describe_their_cause | 图形硬件（GPU）错误文案含根因 | 给定各 `GpuError` 变体，当 `to_string`，则文案包含根因 | 2026-09-19 |

### `crates/gloss-app/src/present/render.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| repaint_delay_max_means_no_wakeup | `MAX` 延迟不唤醒 | 给定 `Duration::MAX` 延迟，当换算唤醒时刻，则 `None` | 2026-09-19 |
| repaint_delay_becomes_a_deadline | 延迟换算为截止时刻 | 给定 250 毫秒与 0 两种延迟，当换算，则分别得 `now` 加延迟与 `now` | 2026-09-19 |

### `crates/gloss-app/src/present/windows.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| position_inside_the_monitor_is_untouched | 屏内位置原样保留 | 给定显示器范围内的位置，当钳制，则原样返回 | 2026-09-20 |
| position_past_the_right_or_bottom_edge_pulls_back | 右下越界向内收 | 给定越出右、下边缘的位置，当钳制，则收到屏宽、高减浮层尺寸处 | 2026-09-20 |
| position_before_the_origin_clamps_to_it | 负坐标钳到屏原点 | 给定负全局坐标（主屏左侧显示器），当钳制，则收到该显示器原点而非主屏 | 2026-09-20 |
| clamping_respects_the_monitor_origin_on_secondary_displays | 副屏钳制按全局原点 | 给定副屏（全局原点非零）右缘位置，当钳制，则按副屏全局区间收口而非主屏 | 2026-09-20 |
| monitor_smaller_than_the_overlay_pins_to_the_origin | 显示器小于浮层贴原点 | 给定比浮层还小的显示器与屏内位置，当钳制，则贴显示器原点（上限取 0） | 2026-09-20 |
| height_caps_at_the_golden_share_of_the_screen_minus_the_margin | 高度上限为屏高占比减余量 | 给定逻辑高 1080 的显示器与超限请求，当 `capped_height`，则上限取屏高乘 `HEIGHT_CAP_FRACTION`（黄金分割率平方的倒数 0.382）再减工作区余量（316.56） | 2026-10-21 |
| smaller_requests_pass_through_untouched | 未超限的高度原样保留 | 给定远小于上限的请求，当 `capped_height`，则原样返回 | 2026-10-02 |
| short_screens_bottom_out_at_the_default_height | 矮屏回落默认高度 | 给定屏高占比扣余量后仍低于默认高度的显示器，当 `capped_height`，则下限取默认高度 296 | 2026-10-21 |
| width_never_changes_during_streaming | 流式锁宽 | 给定当前宽度与更宽请求，当流式防抖，则宽度保持当前值（浮层宽度固定）、高度量化到步长倍数 | 2026-10-19 |
| height_steps_up_in_quantized_increments | 步进增高 | 给定略高于当前档的高度请求，当流式防抖，则上升一个完整步长 | 2026-09-26 |
| height_never_shrinks_during_streaming | 流式不回缩 | 给定明显小于当前档的请求，当流式防抖，则保持当前高度（回缩留给完成态精确重排） | 2026-09-26 |
| current_size_quantizes_up_to_the_step_boundary | 首帧落在步长边界 | 给定与当前相等的请求，当流式防抖，则高度量化到当前之上的步长边界、宽度不变 | 2026-09-26 |
| in_bounds_requests_pass_through_untouched | 界内缩放请求原样保留 | 给定设计档内的宽高请求，当用户缩放钳制，则原样返回 | 2026-10-28 |
| width_clamps_to_the_design_envelope | 缩放宽钳到设计档 | 给定过窄与过宽的请求，当用户缩放钳制，则宽收到最小档与最大档 | 2026-10-28 |
| height_clamps_to_the_minimum_and_tall_requests_pass_through | 缩放高只钳下限、超高请求原样落窗 | 给定过矮与超高请求，当用户缩放钳制，则矮请求收到最小档、超高请求不设上限原样返回（拖拽最大高度限制撤除，流式内容路径的按屏上限另行保留） | 2026-11-03 |
| narrow_screens_collapse_the_width_envelope | 窄屏收窄宽度上限 | 给定窄显示器与超宽请求，当用户缩放钳制，则上限取显示器工作区宽、更窄时保最小档 | 2026-10-28 |
| west_edge_pulls_the_left_edge_and_pins_the_east_edge | 西缘拖拽锚定东缘 | 给定按下锚点（窗口原点横 600 纵 400、宽 480）与西缘内 3 点按压，当指针左移 60，则尺寸宽加 60（含抓取偏移）、窗口原点西移同量、东锚不动 | 2026-10-29 |
| north_edge_anchors_the_south_edge | 北缘拖拽锚定南缘 | 给定北缘内 4 点按压，当指针上移 40，则高加 40、窗口原点上移同量、南锚不动 | 2026-10-29 |
| east_edge_keeps_the_window_origin | 东缘拖拽窗口原点不动 | 给定东缘按压，当指针右移 80，则宽加 80、位置与按下帧相同 | 2026-10-29 |
| north_west_corner_combines_both_anchors | 西北角双锚合解 | 给定西北角按压，当指针左移 50、上移 30，则宽高各加同量、原点按两向反推、东南锚不动 | 2026-10-29 |
| north_east_corner_combines_both_anchors | 东北角双锚合解 | 给定东北角按压，当指针右移 40、上移 20，则宽加 40 高加 20、原点按北向反推、西南锚不动 | 2026-10-29 |
| south_west_corner_combines_both_anchors | 西南角双锚合解 | 给定西南角按压，当指针左移 30、下移 36，则宽加 30 高加 36、原点按西向反推、东北锚不动 | 2026-10-29 |
| clamping_pins_the_dragged_edge_at_the_design_envelope | 钳制钉住被拖边 | 给定西缘按压，当指针右移远超最小宽，则宽钳到 `OVERLAY_MIN_WIDTH`、原点钉在东锚减最小宽（钳制不拽离对侧边） | 2026-10-29 |
| press_frame_solves_to_the_current_window | 按下帧求解不跳变 | 给定三组边界组合，当指针未动求解，则尺寸与位置恒等于按下窗口（抓取偏移吸收按压点与边的偏差） | 2026-10-29 |
| north_drag_past_the_screen_keeps_the_size_and_pins_the_top | 北缘拖出屏幕：尺寸原样、顶缘钉屏顶 | 给定北缘按压，当指针拖到远超屏幕上缘，则超高尺寸原样通过（拖拽无高度上限）、落窗钳制把顶缘钉在屏幕原点（与显示器小于浮层同策） | 2026-11-04 |

### `crates/gloss-app/src/present/workspace.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| apply_theme_elides_writes_until_the_preference_changes | 主题未变不重写 | 给定未变的偏好，当重复 `apply_theme`，则不写；偏好变了则跟上 | 2026-09-23 |

### `crates/gloss-app/src/runtime/cache.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| different_inputs_get_different_keys | 不同输入必不同 `key` | 给定不同文本与不同模态（图像对同载荷文本）的输入，当 `cache_key(input, options)`，则 `key` 互不相同 | 2026-10-03 |
| different_options_get_different_keys | 不同选项必不同 `key` | 给定同一输入，当换 `target_lang/prompt_locale/model`（非出厂值），则 `key` 均不同 | 2026-10-03 |
| key_derivation_is_stable | `key` 派生稳定 | 给定同一输入与选项，当重复派生 `cache_key`，则得同一个 `key` | 2026-10-10 |
| image_keys_follow_png_bytes | 图像 `key` 随字节 | 给定同、异 `png` 字节的 `Image` 输入与同载荷文本输入，当 `cache_key`，则同字节同 `key`、异字节异 `key`、图像与同载荷文本异 `key` | 2026-10-10 |
| entries_store_and_isolate_outcomes | 条目存取与 `key` 隔离 | 给定 `miss`→`set`→`hit` 序列，当按不同 `key` 查询，则命中且无关 `key` 互不可见 | 2026-10-03 |
| ttl_expiry_takes_effect | 存活时长（TTL）过期生效 | 给定存活时长（TTL） 60 毫秒的条目，当过 120 毫秒并 `run_pending_tasks` 后读，则 `miss` | 2026-10-03 |
| factory_ttl_matches_the_config_default | 默认（TTL）单点一致 | 给定出厂（TTL）（`Config::cache_ttl_secs`），当与 `gloss_app::cache` 的 `DEFAULT_TTL` 比对，则相等 | 2026-10-03 |

### `crates/gloss-app/src/runtime/finalize.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| a_contract_reply_builds_the_normal_outcome | 契约回复出正常产物 | 给定词卡、代码、句译的嵌套契约（JSON），当 `complete`，则 `note` 原样、`structured` 按 `kind` 落变体（代码语言随 `Code`）、`degraded=false` | 2026-10-11 |
| soft_field_gaps_stay_on_the_normal_path | 软字段缺口不降级 | 给定 `structured` 缺失与坏条目的契约（JSON）（`note` 在场），当 `complete`，则字段按软规则回落（`phonetic` `None`、坏条目跳过）、`degraded=false` | 2026-10-11 |
| an_image_reply_lands_on_the_plain_variant | 图像回复落 `Plain` 变体 | 给定嵌套契约（`note` 与 `structured.examples`）的 `ImageExplain` 回复，当 `complete`，则 `note` 为描述、`examples` 按序落 `Plain` 疏位、不降级 | 2026-10-11 |
| a_general_reply_lands_on_the_plain_variant | 通用回复落 `Plain` 变体 | 给定嵌套契约（`note` 与 `structured.examples`）的 `General` 回复，当 `complete`，则 `kind` 为 `General`、`examples` 按序落 `Plain` 疏位、不降级（通用任务与句译同形） | 2026-10-12 |
| contract_violations_degrade_to_an_empty_marked_outcome | 契约违约落降级产物 | 给定空回复、散文、围栏输出、缺 `note`、纯空白 `note` 五类违约回复，当 `complete`，则 `degraded=true`、`note` 空串、空 `Code` 变体（降级必有痕、原文垃圾不保留） | 2026-10-11 |
| the_degrade_warn_carries_the_category_only | 降级告警只带违约类别 | 给定散文回复，当 `complete` 并捕获日志，则 `warn` 留痕（`violated` `the` `contract`）、携带固定措辞类别、不含回复原文 | 2026-10-11 |

### `crates/gloss-app/src/runtime/pipeline.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| classified_chunks_and_done_flow_back_in_order | 分类、增量与完成按序回流 | 给定脚本化引擎流（脚本解析不出分类 `kind`，落兜底），当跑完整桥，则 `TaskClassified` → `TaskChunk`（按序携带代数）→ `TaskDone`（完成态解析后正文）依次回传 | 2026-10-03 |
| a_degraded_done_never_enters_the_cache | 降级产物不进缓存 | 给定任务回复为散文（`validate_task` 违约），当跑完整桥两轮同输入，则事件流含 `TaskRepairing` 修复标记、`TaskDone` 携带 `degraded` 产物（`note` 空）、两轮各耗 6 次引擎调用（分类 2 加任务 4——每轮违约都重放，降级产物不投毒缓存） | 2026-10-12 |
| cancel_takes_effect_mid_stream | 流中取消即时生效 | 给定慢流中紧随首 `chunk` 的取消，当取消，则不再有任何后续事件 | 2026-09-19 |
| engine_failure_becomes_task_failed | 引擎失败映射 `TaskFailed` | 给定 `execute` 整体失败，当运行，则映射为带代数的 `TaskFailed` | 2026-09-19 |
| background_panic_becomes_task_failed_and_the_loop_survives | 后台 `panic` 转失败且循环存活 | 给定注入 `panic` 的引擎，当任务炸掉，则转 `EngineResponse` 失败且循环存活、第二个任务照常完成 | 2026-09-19 |
| closing_commands_stops_the_consumer | 关闭命令通道停消费循环 | 给定通道三关闭，当 `drop` 运行时，则超时内干净关停 | 2026-09-19 |
| prewarm_secret_reads_the_store_and_reports_success | 预热读密成功回执 | 给定已预置密钥的存储，当发 `PrewarmSecret`，则回执 `Ok`（值留在存储进程内缓存，不进事件） | 2026-10-06 |
| prewarm_failure_surfaces_as_err_without_the_secret | 预热失败回执只带原因 | 给定读取必失败的存储，当发 `PrewarmSecret`，则回执 `Err(Config)`（不含密钥值，不拦主流程） | 2026-10-06 |
| prewarm_receipt_never_carries_a_secret_value | 预热回执永不携带密钥值 | 给定 `SecretPrewarmed` 回执，当 `Debug` 格式化，则不含密钥值形态（`sk-` 前缀不存在） | 2026-10-06 |
| a_lang_degraded_done_caches_the_full_outcome_for_replay | 语言降级产物入缓存可重放 | 给定语言违约的契约合格脚本队列，当跑完首轮任务（引擎 3 次调用：分类、任务、语言修复）得到降级且内容完整、违约清单在场的产物，再以同输入同选项触发第二次，则缓存直出且清单逐条一致、引擎调用数不再增长（清单随完成产物恒存，回放不重判） | 2026-10-30 |

### `crates/gloss-app/src/ui/code_hl/fallback.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| generic_tokens_color_strings_and_line_comments | 兜底集着色字符串与行注释 | 给定含字符串与 `--` 行注释的片段，当走通用兜底，则两类各出一段；空输入无 `token` | 2026-10-19 |
| generic_tokens_color_block_comments_across_lines | 兜底块注释跨行 | 给定跨行 `/* */` 片段，当走通用兜底，则块注释为单个跨行 `token`，止于闭合星斜杠 | 2026-10-19 |
| generic_tokens_keep_hex_literals_whole | 兜底十六进制整体着色 | 给定含十六进制字面量的片段，当走通用兜底，则十六进制字面量为单个 `Number` `token` | 2026-10-19 |
| generic_tokens_color_type_names_and_leave_lowercase_alone | 兜底类型只认首字母大写 | 给定 `let value: Widget = build()`，当走通用兜底，则 `Widget` 为 `Type`、`build` 为 `Function`，且 `let/value/build` 都不落 `Type` | 2026-10-19 |
| generic_tokens_color_function_shapes_without_the_delimiter | 兜底函数形裁掉定界符 | 给定 `compute(value)`，当走通用兜底，则函数 `token` 止于左括号前 | 2026-10-19 |

### `crates/gloss-app/src/ui/code_hl/mod.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| adopt_keeps_plausible_verdicts_and_normalizes_aliases | 语言判定采纳与别名归一 | 给定规范名、大小写混排、别名（`py`、`objective-c`、`c++`）判定，当 `code_hl::adopt`，则落规范名（门槛加 `normalize_language` 收敛于此） | 2026-10-11 |
| adopt_rejects_prose_placeholders_and_missing_verdicts | 散文与占位串不上角标 | 给定 `None`、空串、纯空白、“…或 `null`”、含空白散文的判定，当 `adopt`，则恒 `None`（无角标、通用启发集着色） | 2026-10-11 |
| adopt_trims_surrounding_whitespace_before_the_gate | 采纳前先修剪首尾空白 | 给定带首尾空白的合法判定，当 `adopt`，则修剪后采纳（不因空白误判成占位串） | 2026-10-11 |
| normalize_language_collapses_aliases_to_canonical_names | 语言名归一化收拢别名 | 给定 `rs/py/TS/golang/c++` 与规范名、清单外名，当归一化，则别名折叠到规范名（`rust/python/typescript/go/cpp`）、规范名原样、未知名保留小写原形 | 2026-10-02 |
| normalize_language_rejects_blank_names | 空白语言名无语言 | 给定空串与全空白名，当归一化，则恒为 `None` | 2026-10-02 |
| canonical_languages_resolve_in_the_default_syntax_set | 规范语言名在语法集中可解析 | 给定规范语言表，当在 `two-face` 的语法集按名字、扩展名查找，则每个条目都命中（缺失时一次性列出全部） | 2026-10-19 |
| unmapped_languages_have_no_syntect_token | 表外语言不猜语法 | 给定 `None`、空白与表外语言（`fortran`），当取语法名，则恒为 `None`；`rs` 别名落（Rust） | 2026-10-19 |
| aliases_always_resolve_through_the_syntax_whitelist | 别名与语法白名单一致 | 给定别名表逐条，当取其语法名，则每条别名都与它的规范名落到同一语法（别名目标的规范名漏进白名单会在此报出；纯规范名不在覆盖内） | 2026-10-19 |
| custom_themes_sit_under_the_keys_egui_extras_indexes | 自定义主题顶替内置键 | 给定进程内 `SyntectSettings`，当其主题集，则含 `egui_extras` 按 `CodeTheme` 取用的两个键 `base16-mocha.dark` 与 `Solarized (light)` | 2026-10-19 |
| highlight_keeps_the_source_and_emits_sections | 着色保留原文并产出多段 | 给定 `rust` 片段，当经 `egui_extras` 着色，则 `LayoutJob` 文本与输入一致、`sections` 非空且至少一段颜色不同于首段 | 2026-10-19 |
| scope_mapping_keeps_keyword_string_comment_number | 关键字、字符串、注释、数字按色板着色并清斜体下划线 | 给定含关键字、注释、字符串、数字的 `rust` 片段，当明暗各着色一次，则 `fn`、字符串、数字逐 `token` 等于色板值（`token` 被 `scope` 拆段时按区间全覆盖）、注释保持斜体且无下划线（绕开 `egui_extras` 的 `ITALIC`→`UNDERLINE` 缺陷） | 2026-10-19 |
| data_format_and_keyword_scopes_keep_their_classes | 数据格式与关键字 `scope` 对齐类别 | 给定 `yaml` 键、`toml` 节名与键、`css` 色值、`json` 键与字符串值、`python`、`ruby`、`lua` 的 `and`、`ruby`、`rust` 的 `self`、`js` 的 `this` 与 `super`，当明暗各着色一次，则依次为 `Type/Function/Type/Number/Type/String/Keyword×7`；负例（`python self`、`bash $@`、`rust &self`）保持平文 | 2026-10-19 |

### `crates/gloss-app/src/ui/code_hl/palette.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| every_class_has_its_own_color_per_theme | 六类色明暗两套互异 | 给定六类别与明暗两主题，当取色，则同主题内六色两两不同、注释恒斜体 | 2026-10-02 |

### `crates/gloss-app/src/ui/context.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| theme_preference_covers_every_variant | 主题三档全映射 | 给定三档主题，当映射界面库（egui）偏好，则一一对应且出厂跟随系统 | 2026-09-23 |
| new_context_carries_fonts_and_theme | 新上下文恒绑定族加延迟补装接上中日韩（CJK） | 给定主题，当新建上下文，则主题偏好落上且命名字体族恒绑定（两段不依赖宿主发现面，任何宿主照跑）；当 `apply_system_fonts` 补装，则中日韩（CJK）后备接上（依赖宿主机字体）；宿主无系统字体发现面时仅补装段无此前提，提前让路 | 2026-11-07 |
| reapply_writes_every_context | 重施加写满每个上下文 | 给定两个已装好的上下文，当施加各档主题，则每个都被写；空集写 0 个不 `panic` | 2026-09-23 |
| text_cursor_stroke_follows_the_palette_per_theme | 插入符描边随调色板明暗两档 | 给定新建上下文，当读明暗两棵样式树的文本插入符描边，则颜色取调色板同源档、宽度取加粗档（缺省淡色在输入框底上不可辨） | 2026-10-28 |

### `crates/gloss-app/src/ui/fonts.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| cjk_fallback_appends_after_builtin_fonts | 中日韩（CJK）后备排在内置字体后 | 给定字体字节，当接入字体定义，则接入成功且中日韩（CJK）后备排在比例与等宽两族内置字体之后 | 2026-10-01 |
| named_family_without_bytes_binds_the_builtin_glyphs | 命名字体族无字节时绑内置字形 | 给定缺失的字体字节，当注册宋体命名字体族，则不发明字体数据、族仍绑定到内置字形（`epaint` 对未绑定族直接 `panic`，族必须恒存在） | 2026-10-01 |
| cjk_fallback_shares_bytes_across_contexts | 后备字体字节按借用登记、零拷贝共享 | 给定同一段字体字节接入两份字体定义，当检查，则两份定义都按借用登记且指向同一地址 | 2026-10-01 |
| named_family_registers_bytes_verbatim | 命名字体族按字节注册 | 给定字体字节，当注册宋体命名字体族，则族名指向自身且字节按借用原样登记 | 2026-10-01 |
| definitions_always_bind_the_named_typography_families | 宋楷与等宽命名族恒绑定 | 给定真实系统的字体定义，当取 `definitions`，则宋楷与等宽三命名族要么有自身字体数据、要么绑定到内置字形（`epaint` 对未绑定族直接 `panic`，族恒存在） | 2026-10-02 |
| zhu_family_is_always_named | 注区字体族恒为命名族 | 给定 `zhu_family` 入口，当取值，则恒为楷体命名字体族 | 2026-10-01 |
| mono_family_is_always_named | 代码、音标字体族恒为命名族 | 给定 `mono_family` 入口，当取值，则恒为 `gloss-mono` 命名字体族 | 2026-10-02 |
| mono_family_chain_appends_the_cjk_fallback | 等宽族绑定含中日韩（CJK）后备 | 给定等宽与中日韩（CJK）两段字体字节（`CJK` 已按后备登记），当注册等宽命名字族，则族链为系统等宽在前、中日韩（CJK）后备在后（代码内中文可读） | 2026-10-02 |
| mono_family_without_a_system_mono_binds_the_builtin_glyphs | 等宽族不向比例（CJK）降级 | 给定缺失的系统等宽字节，当注册等宽命名字族，则族绑内置字形、不落（CJK）比例字体（等宽对齐优先于字形覆盖） | 2026-10-02 |
| system_cjk_font_is_discoverable | 系统（CJK）字体可发现 | 给定真实系统，当执行（CJK）字体发现，则必须找到且字节非空（依赖宿主机）；宿主无系统字体发现面时无此前提，直接让路 | 2026-11-05 |
| system_monospace_font_is_discoverable | 系统等宽字体可发现 | 给定真实系统，当执行等宽字体发现——候选序随目标，系统（macOS）为 `SF Mono` → `Menlo` → `Monaco` → `DejaVu Sans Mono` → `Consolas`，系统（Windows）为 `Consolas`——则必须找到（依赖宿主机）；宿主无系统字体发现面时无此前提，直接让路 | 2026-11-06 |
| system_cjk_font_is_loaded_once | 系统（CJK）字体只加载一次且两份定义共享 | 给定真实系统，当连续两次取字体字节并各自接入字体定义，则两次返回同一段内存、两份定义登记到同一地址（依赖宿主机）；宿主无系统字体发现面时无此前提，直接让路 | 2026-11-05 |
| single_face_files_register_face_zero | 单面文件恒登记 0 号面 | 给定真实系统的等宽字体，当其为单面文件时，则登记面序号为 0（依赖宿主机）；宿主无系统字体发现面时无此前提，直接让路 | 2026-11-05 |
| face_postscript_name_parses_a_synthetic_collection | 合成 `ttc` 解析出 `postscript` 名 | 给定手工拼装的最小 `ttc` 夹具（表偏移按规范从文件头量起），当解析 0 号面的 `name` 表，则交出夹具写入的 `postscript` 名 | 2026-10-09 |
| face_postscript_name_rejects_a_truncated_collection | 截断输入安全返回无名 | 给定被截断的 `ttc` 字节，当解析面目录与 `name` 表，则不越界、返回“无偏移、无名” | 2026-10-09 |
| face_postscript_name_rejects_an_out_of_range_table_offset | 越界表偏移安全返回无名 | 给定 `name` 表偏移被改写到文件末尾之外的 `ttc`，当解析 `postscript` 名，则返回无名（该候选降级） | 2026-10-09 |
| builtin_definitions_binds_named_families_without_system_fonts | 内置快路径定义恒绑定命名字体族且不装系统字体 | 当取 `builtin_definitions`，则宋、楷、等宽三族都绑到内置字形且字体表无（CJK）后备条目 | 2026-10-06 |

### `crates/gloss-app/src/ui/i18n.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| both_locale_files_declare_the_same_keys | 两份文案表键集合一致 | 给定 `zh.toml` 与 `en.toml`，当递归收集叶子键路径，则两份逐条一致且整表条数为 95（条数钉住，防遍历退化）。沿革：原登记 88，2026-10-28 随设置窗两字段标签（服务地址、密钥）入表更新；2026-10-30 随降级批注四词条（悬停标题、注位、音标位、例句位）入表更新；2026-11-10 随剪贴板图片监听提示分平台键（一键拆二）更新 | 2026-11-10 |
| every_entry_is_translated_in_the_english_catalog | 英文表逐条真译不照抄 | 给定两份文案表的全部词条，当逐条比对取值，则无一与中文表逐字相同。沿革：原对语言自称词条（`gloss_ui_language_en`）留同键特例，2026-10-28 随词条语言门禁去豁免删除——中文表该词条改写“英语”后两表必不同，特例失去存在理由 | 2026-10-28 |
| entries_are_written_fully_qualified | 词条键写成下划线全限定名 | 给定两份文案表的每一行非注释行，当解析键名，则键一律以 `gloss_` 开头且不含点号（前缀落在每一行、无节头；退回节头或点号连接即红） | 2026-09-23 |
| placeholders_match_across_locales | 占位符名两语言一一对应 | 给定两份文案表，当逐条比对词条里的 `{{`占位符`}}` 名集合，则两语言一致（拼错名不会单边漏改），且带占位符的词条逐条列名（新增模板漏登记即红；2026-10-30 随批注例句位词条 `{{n}}` 入列） | 2026-10-30 |
| catalogs_parse_into_typed_fields | 文案表解析进类型化字段 | 给定编译期嵌入的两份文件，当取用，则解析成功、两语言取值可区分，且“正在注疏”与“`Re-annotating`”词条逐字就位 | 2026-10-27 |
| the_clipboard_image_hint_names_the_platform_shot_keys_per_platform | 剪贴板图片提示分平台各名其键 | 给定双语文案表，当比对剪贴板图片监听提示的两条平台键，则系统（macOS）键点名 `⌘⌃⇧4`、系统（Windows）键点名 `Win+Shift+S`，且取用面按构建目标取对应键（提示回退混写即红） | 2026-11-10 |
| violation_position_words_align_with_the_seal_names | 批注位置词与印名同字 | 给定双语文案表，当比对批注位置词与印章印名，则注位词与注印同字、例句位词以疏印起头（用户照词即可定位被指认的字段，印名改动时此处连带审查） | 2026-10-30 |
| fill_replaces_every_named_placeholder | 占位符按名填充 | 给定含同名多处的模板与无参数、无对应参数的模板，当填充，则同名全替换、无占位符原样、无参数占位符原样保留 | 2026-09-22 |
| fill_replaces_multiple_distinct_placeholders | 多参填充各自替换对应占位符 | 给定含 `{{a}}` 与 `{{b}}` 两类占位符的模板与两参，当填充，则各自替换对应占位符、互不串位 | 2026-10-14 |
| fill_multi_arg_values_take_part_in_later_scans | 多参逐参替换保留旧的顺序替换语义 | 给定 `a` 参替换值为 `{{b}}`、`b` 参替换值为乙的模板，当填充，则后参重扫先参插入的值、结果为乙（与旧逐参 `str::replace` 语义一致） | 2026-10-14 |
| error_text_maps_every_variant_per_locale | 错误文案按变体覆盖两语言 | 给定十一个无诊断文本变体（含 `JSON` 模式不支持与平台未支持；`SelectionEmpty` 未入本组），当取失败卡文案，则各自的两种语言都非空且互不相同；两个带诊断文本的变体按模板填出整句 | 2026-10-11 |
| each_error_variant_maps_to_its_own_entry | 错误变体各取自己的词条 | 给定十一个无诊断文本变体（含 `JSON` 模式不支持与平台未支持；`SelectionEmpty` 未入本组），当取失败卡文案与错因细节，则各等于本变体对应的词条（两臂对调会被抓住）；带诊断的两个变体按模板填诊断 | 2026-10-11 |
| error_detail_prefers_the_variant_diagnostic | 复合提示取诊断细节 | 给定带诊断的变体与不带诊断的变体，当取错因细节，则前者只出诊断原文、后者回落本地化整句 | 2026-09-22 |

### `crates/gloss-app/src/ui/popup/content.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| watermark_is_the_untranslated_brand_name | 页脚水印是品牌名 | 给定水印取值入口，当取值，则恒为 `Gloss`（品牌名不翻译） | 2026-10-01 |

### `crates/gloss-app/src/ui/popup/icon.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| render_app_icon_rejects_bad_bytes | 图标渲染失败隔离降级 | 给定非（SVG）字节，当栅格化应用图标，则返回 `None`（页头退化为无图标行，不 `panic`） | 2026-10-19 |
| render_app_icon_rasterizes_the_content_square | 图标按画布比例栅本体 | 给定内嵌的应用图标（SVG）与目标边长 40，当矢量栅格化，则得 40 像素见方的图形本体（按 100 比 824 的画布比例平移缩放跳过透明边距）且中心像素不透明 | 2026-10-19 |

### `crates/gloss-app/src/ui/popup/image.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| decode_attached_rejects_bad_bytes | 经位图像解码失败隔离降级 | 给定非位图（PNG）字节，当 `decode_attached`，则返回 `None`（经位落占位，不 `panic`） | 2026-10-10 |
| decode_attached_keeps_the_pixel_dimensions | 经位图像按原像素尺寸解码 | 给定宽 4 高 2 像素的夹具位图（PNG），当 `decode_attached`，则得宽 4 高 2 的像素（RGBA）容器（纹理侧等比随卡宽） | 2026-10-10 |

### `crates/gloss-app/src/ui/popup/mod.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| the_language_badge_arrives_only_with_the_outcome_verdict | 语言角标只随产物判定到达 | 给定 `ExplainCode` 流式视图，当渲染，则树上无语言标签（流式期无判定——唯一来源是产物 `structured.code_language`，经 `code_hl::adopt` 渲染时现算）；给定带 `rust` 判定的代码完成态卡，当渲染，则角标 `rust` 入树；给定翻译流式视图，当渲染，则树上无标签 | 2026-10-11 |
| pending_card_exposes_the_source_and_its_two_exits | 待确认卡的原文与两个出口 | 给定文本待确认卡视图，当渲染，则“选中的原文”与经印章在树、主按钮标签“注疏”与“取消”钮就位；点击主钮上交 `Confirm`，点击取消钮走共用收起路径上交 `Dismiss` | 2026-10-27 |
| the_done_card_copy_button_submits_copy_annotation_then_flips_to_copied | 完成态卡复制钮上交并翻面已复制 | 给定词卡完成态视图，当点击“复制注疏”，则上交 `CopyAnnotation`、随后页脚翻出“已复制”反馈（反馈有时限，不常驻） | 2026-10-27 |
| degraded_and_streaming_cards_take_no_copy_button | 降级与流式卡无复制钮 | 给定降级完成态与流式两种视图，当渲染，则树上无“复制注疏”节点（复制钮只属于定格完成态卡） | 2026-10-27 |
| a_card_with_a_different_source_clears_the_copied_feedback | 换不同原文的卡即清复制反馈 | 给定词卡完成态上已点复制翻出“已复制”，当同一段渲染态换到原文不同的另一张完成态卡，则反馈消失、新卡页脚回到“复制注疏”（反馈不串卡） | 2026-10-27 |
| a_rerender_of_the_same_source_keeps_the_copied_feedback | 同原文重渲染保留复制反馈 | 给定词卡完成态上已点复制翻出“已复制”，当视图以相同原文重渲染一帧，则“已复制”反馈原样保留（原文相同即视为同一张卡） | 2026-10-27 |
| snapshots_match_baseline（popup 代码视图随高亮再录） | 两份代码视图基线随语法着色重录 | 给定 `popup_code_streaming`、`popup_code_outcome` 的英文夹具，当 `wgpu` 渲染并 `diff`，则与基线一致（2026-10-02 随 `T4` 单趟正则六类着色再录：关键字、函数形、字符串着色；其余七份 `popup` 与 `settings` 基线零变化——着色只落代码分支。单趟正则已在 2026-10-19 由 `syntect` 取代，最新沿革见浮层基线 `snapshots_match_baseline` 一行） | 2026-10-02 |

### `crates/gloss-app/src/ui/popup/note.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| stream_fields_extracts_the_json_fields_progressively | 流式正文按（JSON） `note` 渐进提取 | 给定完整（JSON）（含转义），当 `stream_fields`，则 `note` 反转义前缀可见、`examples` 恒空 | 2026-10-11 |
| stream_fields_descends_into_structured_for_the_examples | 扫描器下钻 `structured` 取疏证 | 给定 `note` 与 `structured.examples` 的嵌套契约（JSON），当 `stream_fields`，则两字段全量提取（描述流完不等整个响应，疏证条目随流上屏） | 2026-10-11 |
| stream_fields_shows_the_partial_item_while_the_array_streams | 在写疏条目带上屏 | 给定 `structured.examples` 数组截断在第二条字符串中段的原始流，当 `stream_fields`，则已闭合条目全量、在写条目带反转义前缀、`closed` 停在数组处 | 2026-10-11 |
| stream_fields_keeps_note_only_until_its_value_closes | `note` 未闭合保持进度态 | 给定 `note` 值未闭合、刚起头、冒号后无值三类原始流，当 `stream_fields`，则 `note` 按已到达前缀或空串落进度态、`examples` 恒空 | 2026-10-11 |
| stream_fields_note_stays_visible_while_waiting_for_the_array | 等疏数组期间注保持可见 | 给定 `note` 闭合、`structured` 内数组首条闭合后逗号截断的原始流，当 `stream_fields`，则 `note` 全量可见、疏只带已闭合条目（已到字段不因后字段未到而回退） | 2026-10-11 |
| stream_fields_tolerates_foreign_keys_around_the_contract | 外键跳过不碍契约字段 | 给定顶层外键、`structured` 内字符串同胞键（`code_language`、`phonetic`）、`structured` 截断、`examples` 数组外键、`tail` 数值外键五类原始流，当 `stream_fields`，则外键整值跳过、`note` 与 `structured.examples` 照常渐进 | 2026-10-11 |
| stream_fields_decodes_escapes_in_both_fields | 双字段转义解码与残缺截断 | 给定引号、反斜杠、`unicode` 转义与尾部孤反斜杠的原始流，当 `stream_fields`，则 `note` 与 `structured.examples` 完整转义解码、残缺序列就地截断留待下帧 | 2026-10-11 |
| stream_fields_stays_in_progress_off_contract | 非契约输入恒进度态 | 给定空串、`plain markdown`、裸 `{`、部分键、键无冒号、旧围栏契约六类输入，当 `stream_fields`，则两字段恒空（完成态由降级卡兜住） | 2026-10-11 |
| stream_fields_stops_at_the_closing_brace | 对象闭合即收工 | 给定 `note+structured` 加 尾随外键的完整（JSON），当 `stream_fields`，则两字段提取后在内层 `}` 与外层 `}` 处逐层收工（不误吞尾随结构） | 2026-10-11 |

### `crates/gloss-app/src/ui/popup/outcome.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| annotation_text_joins_the_note_and_the_examples | 复制文本以换行拼接注与疏 | 给定带两条疏条目的产物，当取 `annotation_text`，则疏条目逐条换行拼在注后 | 2026-10-27 |
| annotation_text_without_examples_carries_no_trailing_newline | 无疏条目不带尾随换行 | 给定无疏条目的产物，当取 `annotation_text`，则恰为注文本身（无尾随换行） | 2026-10-27 |
| annotation_text_keeps_the_source_out_of_the_copy | 复制文本不含原文侧信息 | 给定词卡产物（音标加疏条目），当取 `annotation_text`，则音标不在文本中（进剪贴板的只有注与疏） | 2026-10-27 |

### `crates/gloss-app/src/ui/settings.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| save_trims_endpoint_and_treats_blank_key_as_unchanged | 保存端点 `trim`、空白密钥视为未改 | 给定带空白的端点与空白密钥草稿，当 `build_save`，则端点被 `trim`、密钥按 `Keep` 上交 | 2026-09-22 |
| blank_model_saves_as_the_factory_default | 空白模型折叠出厂默认 | 给定模型输入框为空白的草稿，当 `build_save`，则落盘 `config.model` 为 `DEFAULT_TEXT_MODEL`（下游对空串明确失败，不接住会锁死任务） | 2026-10-03 |
| save_carries_the_key_outside_the_config | 密钥走带外通道不上配置 | 给定非空密钥草稿，当 `build_save`，则密钥走 `KeyUpdate::Replace`、不进配置（连 `Debug` 表示也不含） | 2026-09-22 |
| clear_key_is_deferred_to_save_and_revocable | 清除密钥延迟到保存且可撤销 | 给定“清除密钥”标记，当交互与保存，则删除延迟到保存生效、重新输入可撤销标记 | 2026-09-22 |
| invalid_draft_blocks_save_and_enters_the_error_state | 非法草稿阻断保存进入错误态 | 给定非法 `Base URL` 草稿，当 `build_save`，则返回 `Idle`、置校验态、该字段提示含 `https` 规则 | 2026-09-22 |
| fixing_the_field_restores_save | 改对字段恢复保存 | 给定被阻断的校验态，当修正 `Base URL`，则错误清空、再次 `build_save` 上交 `Save` | 2026-09-22 |
| open_copies_the_snapshot_into_the_draft | 打开设置拷贝快照进草稿 | 给定打开时的快照，当建草稿并随后改原配置，则草稿不跟随、可携带提示 | 2026-09-19 |
| clipboard_watch_toggle_travels_with_the_saved_draft | 剪贴板监听开关随草稿保存 | 给定出厂配置的设置窗（英文表），当渲染并点 `Watch the clipboard for images` 行的开关滑块再保存，则出厂默认 `on`、点击切换为 `off` 后随 `Save` 上交进落盘快照（热切换由保存路径承接）；开关以 `WidgetType::Checkbox` 与 `toggled` 态上报（`egui` 0.36 角色映射无 `switch` 档） | 2026-10-12 |
| field_errors_are_worded_per_locale | 字段错误按 `locale` 出措辞 | 给定全部九类字段错误（含行号与触发键回显两种模板），当按中英文表取文案，则各出对应措辞（换臂或漏译会被抓住） | 2026-09-23 |
| every_notice_renders_its_localized_prefix_and_detail | 三类提示的中英措辞 | 给定三类壳回写提示（各带同一诊断），当按中英表取文案，则前缀与诊断都按表落地、且无残留的 `{{`占位符`}}`（两条从未渲染过的模板由此覆上） | 2026-09-22 |
| base_url_errors_map_to_their_own_field_error | `Base URL` 错因映射到字段错误 | 给定五类 `BaseUrlError`，当映射，则空、语法与 `https`、内嵌凭据、查询参数各落到对应 `FieldError`（内嵌凭据与查询参数两臂易错） | 2026-09-22 |
| update_section_buttons_follow_the_phase | 更新区动作按钮随相位 | 给定七个带动作的更新相位，当点对应按钮，则上交 `Check/ConfirmDownload/Cancel/ConfirmRestart/Retry` 之一 | 2026-09-27 |
| update_busy_phases_offer_no_action_and_show_the_target_version | 忙碌相位无动作、显示目标版本 | 给定 `Checking/ReadyToRestart`，当渲染，则无任何更新动作上交；`UpdateAvailable` 的行标签含目标版本号 | 2026-09-27 |
| failed_install_shows_the_install_hint | 替换失败附安装指引 | 给定 `Failed(Install)`，当渲染，则指引行可见（`DMG` 装入、权限出口）且无副作用动作 | 2026-09-27 |
| arrow_keys_scroll_the_overflowing_settings | 溢出内容随方向键滚动 | 给定压到 240 像素视口高的设置窗（三分区必溢出），当按下方向键再按上方向键，则“更新”分区先上移后回位（浮层 `arrow_keys_scroll_the_overflowing_body` 为同构用例，回位容差 1 像素） | 2026-11-03 |
| arrow_keys_yield_to_the_focused_text_input | 输入框持焦时方向键归光标 | 给定视口溢出的设置窗，当点端点输入框持焦后按下方向键，则“更新”分区纹丝不动（`egui` 文本编辑不消费按键事件，守卫缺席即双触发滚动） | 2026-11-03 |
| arrow_keys_yield_to_the_open_combo | 下拉展开时方向键归弹层 | 给定视口溢出的设置窗，当把目标语言下拉滚入视野点开并按下方向键，则弹层背后的“更新”分区纹丝不动（守卫取 `Popup::is_any_open`，`Context` 侧查询恒不命中） | 2026-11-03 |

### `crates/gloss-app/src/ui/style/tokens.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| space_ladder_is_strictly_descending | 间距阶梯严格递减 | 给定间距阶梯（分区、段落、条目、紧邻、行内），当逐档比较，则每档严格大于下一档（档位语义不塌缩） | 2026-10-27 |

### `crates/gloss-app/src/ui/style/typography.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| size_ladder_is_strictly_descending | 字号档位严格递减 | 给定字号档位（词头、标题、疏注、小字、标签五档），当逐档比较，则每档严格大于下一档（档位语义不塌缩） | 2026-10-27 |

### `crates/gloss-app/src/update/download.rs`

整包下载与校验（`L1`，`mock` `HTTP` 服务器驱动）。

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| full_download_verifies_and_lands_at_dest | 全量下载落位 | 给定 200 全量响应，当下载，则 `size/sha256` 校验通过、原子改名落位且 `.partial` 消失 | 2026-09-26 |
| truncated_stream_keeps_partial_for_resume | 断连保留残料 | 给定提前断连的响应，当下载失败，则报长度、传输错误且 `.partial` 保留在途字节 | 2026-09-26 |
| resume_from_partial_completes_and_verifies | 续传完成并全量校验 | 给定遗留 `.partial` 与支持 `Range` 的服务器（206），当 `resume` 下载，则拼接完整、校验通过、落位 | 2026-09-26 |
| server_without_range_support_restarts_from_scratch | 不支持 `Range` 整体重下 | 给定忽略 `Range` 的服务器（200），当 `resume` 下载，则从头重下且结果完整不重复 | 2026-09-26 |
| sha_mismatch_discards_the_partial | 校验不符拒绝并丢弃 | 给定 `sha256` 与清单不符的响应，当下载完成，则报 `ShaMismatch` 且 `.partial` 已丢弃 | 2026-09-26 |
| oversize_response_is_rejected_and_discarded | 超长响应拒绝 | 给定超过清单 `size` 的响应，当下载，则报 `TooLarge` 且 `.partial` 已丢弃 | 2026-09-26 |
| cancellation_returns_cancelled_without_dest | 取消即取消 | 给定已取消的令牌，当下载，则报 `Cancelled` 且无落位文件 | 2026-09-26 |
| bad_artifact_url_is_rejected_before_any_request | 非法产物名前置拒绝 | 给定取不出文件名的 `url`，当下载，则在发起请求前报 `Network` | 2026-09-26 |

### `crates/gloss-app/src/update/install.rs`

`bundle` 原位替换（`L1`，临时目录夹具加 `ditto` 构造 `zip`）。

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| replace_swaps_bundle_and_leaves_no_litter | 替换换装无残留 | 给定旧 `bundle` 与合法 `zip`，当 `replace`，则新 `bundle` 就位原路径、`.app.old` 清除、解压现场清空 | 2026-09-26 |
| replace_over_a_stale_app_old_still_succeeds | 残留 `.app.old` 先清场 | 给定上次替换遗留的 `.app.old`，当替换，则先清场并成功换装 | 2026-09-26 |
| zip_without_a_structured_app_leaves_the_bundle_intact | 坏 `zip` 不动原 `bundle` | 给定非 `zip` 文件，当 `replace`，则报 `Unzip` 且已安装 `bundle` 原样 | 2026-09-26 |
| zip_with_an_app_missing_macos_dir_is_invalid_structure | 包结构检查 | 给定缺 `Contents/MacOS` 的 `.app` `zip`，当 `replace`，则报 `InvalidStructure` 且原 `bundle` 原样 | 2026-09-26 |
| unwritable_dir_is_reported_before_anything_is_touched | 只读目录前置拒绝 | 给定只读的 `bundle` 目录，当安装，则报 `Unwritable` 且 `bundle` 未被触碰 | 2026-09-26 |

### `crates/gloss-app/src/update/install_shared.rs`

原位替换共享核心（`L1`，纯文件系统夹具，两平台载荷契约在任一宿主同跑）。

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| bundle_payload_requires_a_structured_app_dir | 目录载荷结构校验 | 给定解包现场，当定位目录载荷，则结构完整的应用包目录命中、缺 `Contents/MacOS` 的应用包目录落空 | 2026-11-07 |
| flat_payload_requires_the_current_executable_name | 平铺载荷结构校验 | 给定解包现场，当按可执行文件锚点定位平铺载荷，则含同名文件的目录命中、只有旁置文件的目录落空 | 2026-11-07 |
| flat_payload_found_inside_a_nested_folder | 平铺载荷不假设层数 | 给定可执行文件在压缩包顶层目录内的解包现场，当定位平铺载荷，则该顶层目录为载荷根 | 2026-11-07 |
| bundle_entries_swap_the_whole_app_dir | 目录载荷条目推导 | 给定载荷根与安装位的应用包路径，当推导条目，则单条目源为载荷、目标与腾位为应用包路径加旧件后缀 | 2026-11-07 |
| flat_entries_cover_every_file_beside_the_executable | 平铺载荷条目推导 | 给定载荷根内的可执行文件与旁置文件，当推导条目，则每个文件一条目且腾位路径带旧件后缀 | 2026-11-07 |
| swap_moves_entries_into_place_without_litter | 换装就位无残留 | 给定带旧内容的安装位与换装条目，当换装并收尾，则新载荷就位原路径、腾位副本清除 | 2026-11-07 |
| swap_rolls_back_everything_when_a_move_fails | 换装失败全量回滚 | 给定首条目成功、次条目源缺席的条目表，当换装失败，则报 `Move` 且已就位与已腾位的条目全部回到旧内容 | 2026-11-07 |
| swap_reports_a_stash_failure_with_the_dst_untouched | 腾位失败不动安装位 | 给定腾位路径不可达的条目，当换装，则报 `Stash` 且安装位旧载荷原样 | 2026-11-07 |
| swap_clears_a_stale_stash_before_swapping | 上次换装残留先清场 | 给定腾位路径上的上次残留，当换装，则先清场并成功就位、无残留 | 2026-11-07 |

### `crates/gloss-app/src/update/install_windows.rs`

系统（Windows）原位替换（`L1`，压缩包（zip）库构造夹具；目标面测试，随测试矩阵在系统（Windows）运行器执行）。

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| windows_replace_swaps_the_executable_and_side_files | 可执行文件加旁置文件换装 | 给定含可执行文件与旁置文件的 `zip` 与旧安装位，当 `replace`，则新文件就位原路径、腾位副本清除、解压现场清空 | 2026-11-07 |
| windows_replace_rejects_a_zip_without_the_executable | 包结构检查 | 给定不含可执行文件的 `zip`，当 `replace`，则报 `InvalidStructure` 且已安装文件原样 | 2026-11-07 |

### `crates/gloss-app/src/update/manifest.rs`

清单校验矩阵与版本比较（L1）。

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| valid_manifest_parses_with_its_version_and_artifact | 合法清单解析出本机条目 | 给定 `schema=1` 三平台齐备的清单，当 `parse`，则版本与本机平台条目的 `url/size/sha256` 全部就位 | 2026-11-07 |
| unknown_fields_are_ignored_for_forward_compatibility | 未知字段前向兼容 | 给定含 `dmg_url` 等未知字段的清单，当 `parse`，则照常通过 | 2026-09-26 |
| legacy_manifest_still_parses_on_its_home_platform | 旧清单双向兼容（新解析器读旧清单） | 给定发布矩阵扩 `Windows` 前的旧清单（仅双架构键），当 `parse`，则在宿主平台（macOS）照常通过；无本机平台键的平台按 `Invalid` 拒 | 2026-11-07 |
| windows_entry_keeps_the_darwin_pair_intact_for_old_clients | 新清单双向兼容（旧解析器读新清单） | 给定含 `Windows` 键的新清单，当 `parse`，则通过且双架构键仍在条目表——旧解析器与新解析器只差键位强制，条目校验同源，旧版照此放行 | 2026-11-07 |
| unknown_schema_is_fail_closed | 未知 `schema` 拒绝 | 给定 `schema` 为 0、2、99 的清单，当 `parse`，则 `UnknownSchema` 拒绝且不继续解析 | 2026-09-26 |
| unparsable_body_is_rejected | 非法 `body` 拒绝 | 给定空串、非 JSON、数组、`schema` 类型不符的 `body`，当 `parse`，则 `Unparsable` | 2026-09-26 |
| version_must_be_a_semver_triple | 版本必须可解析 | 给定空串、两段、带 `v`、非数字的 `version`，当 `parse`，则 `Invalid` | 2026-09-26 |
| platform_key_is_required_by_the_matrix | 本机平台键必须齐备 | 给定缺本机平台键（其余平台条目齐在）、缺 `channels`、缺 `stable` 的清单，当 `parse`，则 `Invalid` | 2026-11-07 |
| an_empty_platform_key_is_rejected_on_every_host | 空平台键在任何宿主都拒绝 | 给定含空键且三平台条目齐在的清单，当 `parse`，则 `Invalid`（空键宿主的 `contains_key` 不再凑齐矩阵） | 2026-11-07 |
| artifact_fields_follow_the_matrix | 条目字段矩阵 | 给定缺 `url/size/sha256` 或类型不符的条目，当 `parse`，则 `Invalid`（类型不符 `Unparsable`） | 2026-09-26 |
| artifact_url_must_be_https | 条目 `url` 仅接受 `https` | 给定 `http://` 的 `url`，当 `parse`，则 `Invalid` | 2026-09-26 |
| sha256_must_be_64_lowercase_hex | `sha256` 形态校验 | 给定非 64 位、大写、非 `hex` 的 `sha256`，当 `parse`，则 `Invalid` | 2026-09-26 |
| is_newer_follows_semver_precedence | `semver` 全序比较 | 给定更高、相等、更低、预发布、构建元数据版本，当比较，则按 `semver` 全序判定是否更新 | 2026-09-26 |
| current_arch_is_the_compiled_platform_triple | 架构键随编译目标 | 给定编译目标——双架构细分与 `Windows`，当取 `current_arch`，则恰为该目标的发布三元组 | 2026-11-07 |
| current_version_matches_the_cargo_package_version | 本地版本回落分支锁定 | 给定 `CARGO_PKG_VERSION`，当解析，则 `current_version` 与之相等（回落分支不可达） | 2026-09-26 |

### `crates/gloss-app/src/update/mod.rs`

模块接线与集成时序（L1）：经公共（API）（消息通道加 `watch` 广播）驱动，桩 `hooks` 用本地 `oneshot` 触发回包，同步点全走 `watch`，不碰网络。

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| initial_broadcast_is_the_idle_snapshot | 启动即广播初始快照 | 给定刚拉起的模块，当订阅 `watch`，则先收到 `Idle` 快照 | 2026-09-26 |
| shared_client_hands_out_one_process_wide_instance | 超文本传输（HTTP）客户端进程内唯一 | 给定共享客户端入口，当连续两次取用，则两次拿到同一实例（构建成功后常驻，失败不缓存下次重建） | 2026-10-09 |
| state_transition_wakes_the_ui_without_input | 相位迁移唤醒壳层 | 给定已安装的唤醒桩，当检查被受理与在途回包被采纳，则两次迁移各触发一次唤醒、全程无输入 | 2026-09-27 |
| confirmed_flow_runs_check_download_install_to_completion | 两道确认全流程到替换成功 | 给定新版清单与下载、替换成功桩，当检查→确认下载→确认重启，则相位依次推进且替换收到 `zip` 路径、任务以“已安装”收尾 | 2026-09-26 |
| not_newer_manifest_lands_in_up_to_date_without_a_target | 无新版落 `UpToDate` | 给定与本地等版本的清单回包，当检查完成，则落 `UpToDate` 且无目标版本 | 2026-09-26 |
| manifest_failure_lands_in_failed_and_retry_rechecks | 清单步失败与重试 | 给定清单拉取失败回包，当检查完成再点重试，则先落 `Failed(Manifest)` 再回 `Checking` 并重跑检查桩 | 2026-09-26 |
| download_failure_lands_in_failed_and_retry_resumes | 下载步失败与续传重试 | 给定下载失败回包，当失败后再点重试，则落 `Failed(Download)`、重跑下载桩且 `resume` 为真 | 2026-09-26 |
| install_failure_lands_in_failed_install_and_retry_installs | 替换步失败与重试 | 给定替换失败回包，当失败后再点重试，则落 `Failed(Install)`、重跑替换桩并以成功收尾 | 2026-09-26 |
| cancel_during_download_returns_to_update_available | 取消下载与迟到回包丢弃 | 给定下载中任务，当取消，则回 `UpdateAvailable`；其后的迟到下载回包被丢弃（确认下载仍可用即相位未被动过） | 2026-09-26 |
| fetch_manifest_parses_a_valid_body_from_the_wire | 线上清单解析 | 给定本地服务器回的合法清单（带 `Content-Length`），当 `fetch`，则解析出版本 | 2026-09-27 |
| fetch_manifest_rejects_oversize_body_without_content_length | 无长度声明的超限 `body` 拒绝 | 给定不声明 `Content-Length`、逐块送出 1.5 兆二进制字节（MiB）的服务器，当 `fetch`，则中途判超限拒绝（无界内存禁入） | 2026-09-27 |

### `crates/gloss-app/src/update/state.rs`

更新子状态机迁移表逐行（L1）。

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| starts_from_idle_into_checking_with_a_command | 发起检查进 `Checking` | 给定 `Idle` 状态机，当 `check`，则落 `Checking` 并交出清单拉取命令与令牌 | 2026-09-26 |
| checking_manifest_newer_lands_in_update_available | 新版落 `UpdateAvailable` | 给定更高版本的清单，当采纳，则锁目标版本与本机架构产物 | 2026-09-26 |
| checking_manifest_not_newer_lands_in_up_to_date | 无新版落 `UpToDate` | 给定不高于本地的清单，当采纳，则落 `UpToDate` 且清掉旧目标 | 2026-09-26 |
| checking_manifest_unavailable_lands_in_failed_manifest_step | 清单失败落清单步 | 给定清单不可用，当采纳，则落 `Failed(Manifest)` 且令牌清空 | 2026-09-26 |
| recheck_from_any_active_phase_cancels_and_returns_to_checking | 重复检查先取消 | 给定下载中的任务，当再次检查，则回 `Checking` 且下载令牌被取消 | 2026-09-26 |
| ready_to_restart_refuses_a_new_check | 替换执行中拒绝新检查 | 给定 `ReadyToRestart`，当 `check`，则拒绝且相位不动 | 2026-09-26 |
| confirm_download_only_from_update_available | 确认下载仅限待确认态 | 给定 `Idle/Checking/UpdateAvailable`，当 `confirm_download`，则仅最后者交出全量下载命令（`resume` 为假） | 2026-09-26 |
| download_verified_lands_in_update_ready_then_install_confirms | 下载完成与确认重启 | 给定下载完成回包，当采纳并确认重启，则经 `UpdateReady` 落 `ReadyToRestart` 并交出替换命令 | 2026-09-26 |
| download_failed_lands_in_failed_download_step | 下载失败落下载步 | 给定下载失败回包，当采纳，则落 `Failed(Download)` 且目标保留 | 2026-09-26 |
| retry_returns_to_the_recorded_step | 重试回到记录步骤 | 给定三类失败态，当 `retry`，则分别回 `Checking/Downloading(resume)/ReadyToRestart` 并交出对应命令 | 2026-09-26 |
| install_failed_lands_in_failed_install_step | 替换失败落替换步 | 给定 `ReadyToRestart`，当采纳替换失败，则落 `Failed(Install)` | 2026-09-26 |
| cancel_only_interrupts_a_download | 取消仅对下载有效 | 给定 `Idle/Checking/Downloading`，当 `cancel`，则仅下载中取消令牌并回 `UpdateAvailable`，其余拒绝 | 2026-09-26 |
| late_outcomes_are_rejected_by_phase_guards | 迟到回包被相位守卫拒绝 | 给定已离开中间相位的状态机，当重放各回包，则一律拒绝且状态不动 | 2026-09-26 |
| update_available_clears_when_a_new_check_finds_nothing_newer | 新检查收回旧提示 | 给定 `UpdateAvailable`，当重新检查发现无新版，则落 `UpToDate` 且目标撤下 | 2026-09-26 |
