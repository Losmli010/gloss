# 测试行为清单（BDD）

以测试代码为唯一事实源：本清单逐条描述仓库当前测试的行为；新增、修改、删除测试时在同一 PR 内登记并刷新该条目的更新时间；描述与代码冲突时以代码为准并立即修正。

组织：人工测试 → 集成测试 → 性能测试 → 快照测试 → 单元测试。条目四字段：测试名称、测试目标、测试场景（给定/当/则）、更新时间。

## 总览

| 类别 | 数量 | 运行 |
| --- | --- | --- |
| 人工测试 | 10 | `cargo test -p gloss-platform -- --ignored` |
| 集成测试 | 5 | `just test` |
| 性能测试 | 1 | `just selftest` |
| 快照测试 | 12 | `just test` |
| 单元测试 | 223 | `just test` |

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
- 测试场景：给定辅助功能授权与 rdev 注入的拖拽序列（(100,100) 按下 → 五段移动 → 释放），当经真实事件 tap，则 2s 内监听到手势且监听器未降级。
- 测试步骤：
  1. 系统设置 → 隐私与安全性 → 辅助功能 → 放行运行测试的终端 App
  2. 运行总览中人工测试的命令
  3. 测试注入真实全局鼠标事件，屏幕光标会移动
  4. 未授权时前置检查当场失败并打印修复指引
- 更新时间：2026-09-19

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

## 集成测试

文件：crates/gloss-app/tests/pipeline.rs（L1，经公共 API 与通道两端驱动状态机 + 通道③④ + tokio 桥 + mock 引擎的全时序）。

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| full_flow_streams_and_settles | 全链路流式回流与终态 | 给定触发→取材→三段流式响应，当全链路推进并逐条回喂状态机，则 chunk 逐条回流、TaskDone 后定格 Show、正文剥离围栏且词卡解析出 word/senses | 2026-09-19 |
| superseded_trigger_cancels_and_filters_late_events | 新触发取消旧任务并过滤迟到事件 | 给定 A 未完成时触发 B，当 B 触发，则 A 的令牌立即取消、代数 +1、A 代数的迟到 chunk 被状态机拒绝，B 的 chunk/done 正常回流至 Show | 2026-09-19 |
| failure_lands_in_error_and_retry_succeeds | 失败落错误态且重试可达 | 给定首次注入 EngineRateLimited 失败，当失败回传后再次触发，则落 Error 态、第二次任务完成落 Show | 2026-09-19 |
| error_card_retry_redispatches_the_same_task | 重试动作重发同一任务 | 给定可重试失败的 Retry 出口，当 retry 并重发 RunTask，则同代数重发同一任务并完成落 Show | 2026-09-19 |
| config_change_invalidates_cache_for_the_next_task | 配置变更对缓存 key 的失效 | 给定同文本连续任务与运行时保存的新配置，当执行，则未改配置命中缓存（引擎 1 次）、换模型与换目标语言各触发一次重新请求（共 3 次） | 2026-09-19 |

## 性能测试

文件：tests/overlay_selftest.rs（L3，harness = false 自带 main()，跑在主线程，需窗口服务与 GPU）。

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| overlay_selftest | 真实窗口栈显隐生命周期与首帧预算 | 给定经公共 API build_window_stack 预创建的生产窗口栈，当反复 show → 渲染 → hide 共 100 轮（每轮停留 80ms），则统计 show→首帧延迟并计入门禁：跑满 100 轮且有延迟统计退出 0，无帧或首帧超 100ms 预算退出 1；窗口句柄数仅进日志供人工走查（验证复用不增长） | 2026-09-19 |

## 快照测试

popup 快照基线：popup_word_card、popup_streaming、popup_failed、popup_failed_auth；settings 快照基线：settings_main。

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| word_card_exposes_entries_to_accesskit | 词卡视图的无障碍树结构 | 给定词卡 Outcome 视图，当渲染，则 AccessKit 树可按文本定位节点：gloss、音标、释义、例句（复制按钮已移除，划选即复制） | 2026-09-20 |
| long_body_is_rendered_in_full | 长正文完整渲染不截断 | 给定超长正文（尾部带标记），当渲染，则 AccessKit 树含尾部内容——无字符截断 | 2026-09-20 |
| width_hysteresis_does_not_oscillate_between_frames | 宽度滞回不振荡 | 给定上一帧宽度与内容高，当决策宽度，则长内容加宽、带内保持原档、明显变矮才收回 | 2026-09-20 |
| width_hysteresis_band_bounds_are_symmetric | 滞回阈值边界对称 | 给定阈值附近的内容高，当按当前档决策，则过加宽阈值才加宽、过收回阈值才收回 | 2026-09-20 |
| streaming_view_hides_structured_block | 流式视图不暴露结构化围栏 | 给定流式视图（正文含围栏），当渲染，则「已流式到达的正文」「选中的原文」可见而 ```gloss 围栏不在树中 | 2026-09-19 |
| failed_view_shows_retry_hint | 失败卡重试动作 | 给定 Retry 失败卡，当渲染并点击「重试」，则收集器收到 ErrorAction::Retry | 2026-09-19 |
| auth_failed_view_offers_open_settings | 鉴权失败卡设置入口 | 给定鉴权失败卡，当渲染并点击「打开设置」，则收到 ErrorAction::OpenSettings | 2026-09-19 |
| bare_failed_view_has_no_action_button | 无动作失败卡形态 | 给定 action=None 失败卡，当渲染，则无「重试」节点、无动作 | 2026-09-19 |
| snapshots_match_baseline（popup） | 浮层四视图渲染基线 | 给定四个视图，当 wgpu 渲染并 diff，则与 popup_word_card / popup_streaming / popup_failed / popup_failed_auth 四份基线一致，结果合并进单个 SnapshotResults | 2026-09-19 |
| all_sections_render_and_save_submits_the_draft | 设置窗渲染与保存提交 | 给定默认配置的设置窗口，当渲染并点保存，则各区块控件可定位且上交未改动的出厂快照 | 2026-09-19 |
| task_toggle_flips_enabled_kinds | 任务开关写回启用表 | 给定点掉「启用词卡」后保存，当检查上交配置，则 TranslateWord 已停用 | 2026-09-19 |
| cancel_and_clear_key_actions_are_submitted | 取消与清除密钥动作 | 给定「取消」与「清除密钥」按钮，当分别点击，则取消上交 Close、清除只置标记（按钮变「撤销清除」）、保存时才上交 Clear | 2026-09-19 |
| hotkey_rows_expose_trigger_and_kind | 热键行无障碍结构 | 给定出厂三条绑定，当渲染，则 3 行「划词」源标签进 AccessKit 树 | 2026-09-19 |
| snapshots_match_baseline（settings） | 设置窗渲染基线 | 给定设置窗口，当 wgpu 渲染，则与 settings_main 基线 diff 不超阈值 | 2026-09-19 |

## 单元测试

### crates/gloss-core/tests/stubs_behavior.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| selection_reader_mock_returns_presets | 选区读取桩的两路透传 | 给定预置的成功/失败结果，当调用 SelectionReader 桩的 read，则两路都原样返回（Ok 与 AccessibilityDenied） | 2026-09-19 |
| region_capture_mock_returns_png | 区域截图桩的字节透传 | 给定预置 PNG 字节，当 capture 一个 4×4 区域，则返回同一份 Arc 缓冲（ptr_eq 断言） | 2026-09-19 |
| config_store_mock_round_trips_secrets | 密钥存取桩往返 | 给定密钥未设置时读为 None，当 set_secret 后再读，则读回写入值 | 2026-09-19 |
| config_store_mock_round_trips_document | 配置文档桩往返 | 给定默认存储，当 save 一份 auto_show=false 的文档，则 load 原样读回 | 2026-09-19 |
| cache_mock_stores_and_isolates_keys | 缓存桩存取与 key 隔离 | 给定 miss→set→hit 序列，当按不同 key 查询，则命中且无关 key 互不可见 | 2026-09-19 |
| hotkey_binder_mock_records_every_call | 热键绑定桩如实记录 | 给定多次 rebind（含空表），当读桩的 call_count 与 last，则按调用序记录、生效条数如实、最后一次覆盖、空表计 0 | 2026-09-19 |
| chunk_delay_paces_the_stream | chunk 延迟为流定速 | 给定 30ms chunk 间延迟的三段脚本，当消费流，则内容按序且总耗时下界为两段延迟 | 2026-09-19 |
| failures_are_injectable | 失败位置可注入 | 给定注入的各类 GlossError 与流中 Err，当 execute，则失败在注入位置原样发生、流继续按脚本 | 2026-09-19 |
| execute_failure_once_fails_exactly_once | 一次性失败只生效一次 | 给定注入一次性失败与恢复脚本的引擎，当连续两次 execute，则首次返回注入错误、第二次照常产流 | 2026-09-19 |
| execute_panic_fires_on_first_poll | panic 注入在首次 poll 触发 | 给定注入一次 panic 的引擎，当在 tokio 任务里驱动 execute，则任务以 panic 收场 | 2026-09-19 |
| call_count_tracks_execute_invocations | 调用计数如实增长 | 给定多次 execute（含克隆体），当读 call_count，则共享计数如实增长 | 2026-09-19 |
| empty_script_yields_empty_stream | 空脚本产出空流 | 给定空脚本，当 execute 并消费，则立即结束 | 2026-09-19 |

### crates/gloss-core/src/cache.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| same_text_different_kinds_do_not_share_cache | 缓存 key 按 kind 隔离 | 给定同文本不同 kind，当派生 key 并写词卡条目，则句译 key 不命中、词卡 key 命中 | 2026-09-19 |
| model_id_participates_in_key | 模型 id 参与 key | 给定同任务同文本，当换模型 id，则 key 不同 | 2026-09-19 |
| input_and_options_participate_in_key | 输入选项参与 key | 给定同 kind 同文本，当加 hint 或改 target_lang，则 key 均不同 | 2026-09-19 |
| ttl_expiry_takes_effect | TTL 过期生效 | 给定 TTL 60ms 的条目，当过 120ms 并 run_pending_tasks 后读，则 miss | 2026-09-19 |
| cache_key_falls_back_when_serialization_fails | 序列化失败时 key 稳定兜底 | 给定含 NaN 的 Audio 输入（序列化失败），当两次派生 key，则结果稳定且与 1.5 的 key 不同 | 2026-09-19 |

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

### crates/gloss-core/src/config.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| factory_defaults_match_spec | 出厂默认符合规格 | 给定出厂 Config，当逐字段抽查，则默认语言/任务/热键三条/提供商/文本模型等全部符合规格 | 2026-09-19 |
| config_round_trips_through_serde | 配置 serde 往返无损 | 给定含 Lang::Other 等携数据变体的完整配置，当 serde_json 往返，则无损 | 2026-09-19 |
| partial_document_fills_factory_defaults | 部分文档补全出厂默认 | 给定只写 theme 的 JSON，当加载，则该字段保留、其余走出厂默认（含三条热键与全部 kind 启用） | 2026-09-19 |
| explicit_empty_enabled_kinds_disables_everything | 显式空数组语义 | 给定 enabled_kinds 显式空数组，当加载，则所有 kind 停用 | 2026-09-19 |
| missing_fields_default_while_explicit_empty_stays_empty | 缺省回退与显式空的区分 | 给定 provider_keys/model_by_kind 显式空，当加载，则纯查找为空、resolved 查找回退出厂项、图像 kind 不借文本模型 | 2026-09-19 |
| selection_kind_falls_back_for_image_kinds | 误配图像默认回退文本 | 给定 default_text_kind 误配成 ImageOcr，当解析划词任务，则回退 TranslateWord | 2026-09-19 |
| default_cache_ttl_matches_cache_implementation | 默认 TTL 单点一致 | 给定出厂 TTL，当与 cache::DEFAULT_TTL 比对，则相等 | 2026-09-19 |
| edit_helpers_keep_tables_canonical | 编辑助手保持表规范 | 给定启用/停用与模型编辑操作，当调用助手，则不重复追加、停用幂等、模型按 kind 替换、空白视为未配置 | 2026-09-19 |
| lookups_prefer_later_entries | 重复条目查找后者胜出 | 给定重复 kind 的多条目，当查找，则后条胜出、未配置为 None、未知 provider 无 keychain id | 2026-09-19 |

### crates/gloss-core/src/prompt.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| text_kinds_render_system_and_user_with_kind_content | 文本 kind 渲染两段消息 | 给定三个文本 kind，当渲染，则得 [System, User] 两段，系统指令含各自关键词与结构化契约围栏，用户消息为原文 | 2026-09-19 |
| structured_contract_matches_outcome_schema | 结构化契约与 schema 对齐 | 给定词卡与代码解释模板，当检查系统指令，则分别声明 senses/phonetic 与 title 字段 | 2026-09-19 |
| missing_target_lang_defaults_to_chinese | 目标语言缺省中文 | 给定未设 target_lang，当渲染句译，则系统指令含「中文」 | 2026-09-19 |
| explicit_target_lang_is_rendered | 显式目标语言渲染 | 给定 Lang::Ja，当渲染，则系统指令含「日语」 | 2026-09-19 |
| hint_is_injected_and_defaults_to_nothing | hint 注入与缺省不注入 | 给定 CodeLanguage hint，当渲染，则注入系统与用户两处；无 hint 则两处均无注入行、用户消息为原文 | 2026-09-19 |
| source_lang_hint_is_injected | 源语言 hint 注入 | 给定 SourceLang(法语)，当渲染，则系统指令含「源语言：法语」 | 2026-09-19 |
| image_kinds_are_placeholders_until_m5 | 图像 kind 占位拒绝 | 给定图像 kind 的图像任务，当渲染，则报 UnsupportedModality | 2026-09-19 |
| modality_mismatch_is_rejected_before_rendering | 模态错配在渲染前拒绝 | 给定图像 kind 配文本输入，当渲染，则先被模态约束拒绝 | 2026-09-19 |
| messages_serialize_to_openai_shape | 消息序列化为 OpenAI 形态 | 给定 ChatMessage，当序列化，则得 {"role","content"} 的 OpenAI 形态 | 2026-09-19 |

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

### crates/gloss-core/src/task.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| task_input_carries_text_and_hint | 文本输入携带 hint | 给定文本与 hint，当构造 TaskInput::Text，则两者正确携带 | 2026-09-19 |
| task_binds_kind_input_and_options | Task 三元正确绑定 | 给定构造参数，当建 Task，则 kind/input/options 正确绑定 | 2026-09-19 |
| image_input_shares_png_bytes_via_arc | 图像输入 Arc 共享 | 给定 PNG 字节，当构造 Image 输入，则经 Arc 共享（ptr_eq）并携带区域 | 2026-09-19 |
| accepts_text_follows_the_modality_matrix | kind 接受文本的模态矩阵 | 给定五种 kind，当查 accepts_text，则文本类 true、图像类 false | 2026-09-19 |
| hotkey_binding_pairs_kind_with_source | 热键绑定与源配对 | 给定 trigger/kind/source，当构造 HotkeyBinding，则 source 正确配对 | 2026-09-19 |
| outcome_carries_structured_variants | 词卡结构化字段携带 | 给定 WordCard 变体，当构造 TaskOutcome，则 word 字段完整携带 | 2026-09-19 |
| modality_matrix_is_enforced_cell_by_cell | 模态矩阵逐格校验 | 给定 5 kind × 3 输入全矩阵，当逐格 validate，则文本/Image 各占合法列、Audio 全列非法 | 2026-09-19 |
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
| streams_chunks_in_order_and_assembles_body | 流式增量按序转发并拼正文 | 给定三段流式脚本，当 execute，则增量按序转发、正文原样拼接、引擎调 1 次 | 2026-09-19 |
| cache_hit_skips_the_engine | 缓存命中跳过引擎 | 给定已执行的同一任务，当第二次 execute，则结果与首次一致且引擎调用数仍为 1 | 2026-09-19 |
| different_model_misses_the_cache | 模型参与缓存判定 | 给定同任务换模型 id，当两次执行，则引擎被调 2 次 | 2026-09-19 |
| word_card_is_parsed_from_structured_block | 词卡从结构化围栏解析 | 给定正文+```gloss 围栏 JSON，当 execute，则围栏从正文剥离、词卡结构化字段完整回填 | 2026-09-19 |
| phonetic_null_maps_to_none | phonetic null 映射 None | 给定 "phonetic":null 的围栏 JSON，当解析，则 phonetic 为 None、senses 为空 | 2026-09-19 |
| bad_sense_entries_are_skipped_not_fatal | 坏词条跳过不致命 | 给定含缺字段坏条目的 senses，当 parse_structured，则两条好条目保留、坏条目跳过 | 2026-09-19 |
| trailing_text_after_fence_is_dropped | 围栏后尾随文字丢弃 | 给定围栏后的契约外尾随文字，当 parse_structured，则正文不含尾随文字、结构化为 Plain{title} | 2026-09-19 |
| missing_structured_block_falls_back_to_plain | 无围栏回退纯文本 | 给定无围栏的流式正文，当 execute，则正文原样、结构化为无标题 Plain | 2026-09-19 |
| ocr_fallback_extracts_whole_body | OCR 回退全文提取 | 给定 OCR 的三种输入（无围栏/坏 JSON/合法 text 围栏），当 parse_structured，则回退全文提取、残片不剥离、合法时剥离正文 | 2026-09-19 |
| engine_failures_propagate | 引擎失败原样上抛 | 给定 execute 整体失败与流中 Err，当执行，则错误原样上抛且失败前的增量已转发 | 2026-09-19 |
| modality_mismatch_is_rejected_before_engine | 模态错配在引擎前拒绝 | 给定模态错配任务，当 execute，则 UnsupportedModality 且引擎 0 调用 | 2026-09-19 |
| image_tasks_stay_unsupported | 图像任务保持不支持 | 给定真实图像输入的图像任务，当走编排，则仍报 UnsupportedModality 且引擎 0 调用 | 2026-09-19 |

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

### crates/gloss-app/src/app/render.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| repaint_delay_max_means_no_wakeup | MAX 延迟不唤醒 | 给定 Duration::MAX 延迟，当换算唤醒时刻，则 None | 2026-09-19 |
| repaint_delay_becomes_a_deadline | 延迟换算为截止时刻 | 给定 250ms/0 延迟，当换算，则 now+延迟/now | 2026-09-19 |

### crates/gloss-app/src/app/handler.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| sooner_picks_the_earliest_deadline | 取更早的截止时刻 | 给定两个时刻（可含 None），当取更早，则 None 让位、双 None 不唤醒 | 2026-09-19 |

### crates/gloss-app/src/app/channels.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| late_events_of_superseded_trigger_do_not_bleed | 被顶代数的迟到事件不渗漏 | 给定连续触发 A→B，当 A 的迟到 chunk/TaskDone 到达，则被陈旧过滤，B 的产物照常 Show | 2026-09-19 |
| failed_task_lands_in_error_and_retry_works | 失败落错误态且可再触发 | 给定推理中任务，当匹配代数的失败到达，则落 Error；陈旧失败丢弃；再次触发回 Fetching | 2026-09-19 |
| stale_input_ready_is_dropped_entirely | 陈旧 InputReady 整体丢弃 | 给定陈旧代数 InputReady，当采纳，则整体丢弃、不下发通道③ | 2026-09-19 |
| saved_config_applies_to_the_next_trigger | 新配置对下次触发生效 | 给定保存新配置，当下一次触发，则目标语言与模型随任务下发 | 2026-09-19 |
| saved_config_does_not_leak_into_the_inflight_task | 在途任务用触发时快照 | 给定触发后、产物到达前保存新配置，当在途任务下发，则仍用触发时快照 | 2026-09-19 |

### crates/gloss-app/src/app/overlay.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| retry_action_redispatches_the_failed_task | Retry 动作重发失败任务 | 给定失败卡 Retry 动作，当执行，则同代数同任务新令牌重发通道③，重试产物照常采纳 | 2026-09-19 |
| open_settings_action_keeps_the_error_card | 打开设置保留错误卡 | 给定鉴权失败卡，当执行 OpenSettings 动作，则停在 Error、通道③无流量、编辑会话就位 | 2026-09-19 |
| auto_show_policy_decides_when_the_overlay_pops | 自动弹出按策略表 | 给定事件类别×采纳×开关组合，当逐事件判定，则按策略表露面、未采纳一律不弹、chunk 从不弹 | 2026-09-19 |
| auto_show_survives_a_mixed_batch | 混合批次自动弹出取或 | 给定一批混合回传，当按批取或，则一条被采纳的完成/失败即弹、整批陈旧不弹、空批不弹 | 2026-09-19 |

### crates/gloss-app/src/app/settings_session.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| open_settings_request_starts_an_edit_session | 打开设置即编辑会话 | 给定 OpenSettingsRequested，当消费，则编辑会话打开、草稿=当前快照、不占代数 | 2026-09-19 |
| settings_save_writes_keychain_and_swaps_config | 保存写密钥串并换配置 | 给定含密钥替换的保存，当成功，则密钥进 keychain、快照换新、会话关闭，新模型随后续触发生效 | 2026-09-19 |
| clearing_the_key_deletes_the_secret_on_save | 清除密钥保存即删除 | 给定 KeyUpdate::Clear，当保存，则 keychain 条目删除、会话关闭 | 2026-09-19 |
| failed_save_keeps_the_session_open_with_a_notice | 失败保存会话不关 | 给定落盘必失败存储，当保存，则会话保持打开、错误进提示、快照不变 | 2026-09-19 |
| saving_settings_rebinds_hotkeys_from_the_new_snapshot | 保存按新快照重注册热键 | 给定保存新热键表，当成功，则按新快照重注册一次（启动注册不计入 App） | 2026-09-19 |
| every_save_rebinds_hotkeys_not_just_the_first | 每次保存都重注册 | 给定连续两次成功保存，当各次执行，则都重注册且第二次生效第二份 | 2026-09-19 |
| failed_save_does_not_rebind_hotkeys | 失败保存不重注册 | 给定落盘失败，当保存，则重注册计数为 0 | 2026-09-19 |

### crates/gloss-app/src/app/theme.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| theme_preference_covers_every_variant | 主题三档全映射 | 给定三档主题，当映射 egui 偏好，则一一对应且出厂跟随系统 | 2026-09-19 |
| apply_theme_writes_every_context | 主题写满每个 Context | 给定两个 egui Context，当施加各档主题，则每个都被写；空集写 0 个不 panic | 2026-09-19 |
| apply_theme_elides_writes_until_the_preference_changes | 主题未变不重写 | 给定未变的偏好，当重复 apply_theme，则不写；偏好变了则跟上 | 2026-09-19 |

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
| chunks_and_done_flow_back_in_order | 增量与完成按序回流 | 给定脚本化引擎流，当跑完整管道，则 chunk 按序携带代数转发、TaskDone 携剥离后正文 | 2026-09-19 |
| cancel_takes_effect_mid_stream | 流中取消即时生效 | 给定慢流中紧随首 chunk 的取消，当取消，则不再有任何后续事件 | 2026-09-19 |
| engine_failure_becomes_task_failed | 引擎失败映射 TaskFailed | 给定 execute 整体失败，当运行，则映射为带代数的 TaskFailed | 2026-09-19 |
| image_task_without_a_model_fails_before_the_engine | 无视觉模型的图像任务先失败 | 给定未配视觉模型的图像任务，当运行，则以 Config 错误失败且引擎 0 调用 | 2026-09-19 |
| background_panic_becomes_task_failed_and_the_loop_survives | 后台 panic 转失败且循环存活 | 给定注入 panic 的引擎，当任务炸掉，则转 EngineResponse 失败且循环存活、第二个任务照常完成 | 2026-09-19 |
| closing_commands_stops_the_consumer | 关闭命令通道停消费循环 | 给定通道③关闭，当 drop 运行时，则超时内干净关停 | 2026-09-19 |

### crates/gloss-app/src/machine.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| trigger_mapping_covers_wired_events_only | 触发映射只覆盖已接线事件 | 给定划词手势与未接线的框选热键，当 trigger，则前者发 AcquireText、后者 None 且不占代数 | 2026-09-19 |
| selection_kind_and_options_pair_with_one_snapshot | kind 与选项出自同一快照 | 给定自定义配置快照，当划词触发并采纳输入，则 kind 与模型/语言选项出自同一份快照 | 2026-09-19 |
| image_default_kind_falls_back_to_a_text_kind | 误配图像默认回退文本 kind | 给定 default_text_kind 误配图像类，当划词触发，则回退 TranslateWord | 2026-09-19 |
| disabled_kinds_are_not_acquired_and_consume_no_generation | 停用 kind 不取材不占代数 | 给定含停用 kind 的配置，当划词/热键触发停用项，则 None 且不占代数 | 2026-09-19 |
| options_freeze_at_trigger_time | 选项在触发时刻冻结 | 给定触发后更换配置，当采纳输入，则任务仍带触发时快照的选项；第二次触发才用新值 | 2026-09-19 |
| accept_input_yields_run_request_and_guards_state | 采纳输入下发请求并守卫状态 | 给定合法 InputReady，当采纳，则返回下发请求、进 Translating、持有取消令牌；同代数重复采纳被拒 | 2026-09-19 |
| image_input_for_text_kind_is_rejected | 文本 kind 拒绝图像输入 | 给定文本 kind 配图像输入，当采纳，则 None | 2026-09-19 |
| hide_abandons_inflight_and_drops_late_events | 隐藏放弃在途并拒迟到事件 | 给定 Translating 态隐藏，当收起，则令牌取消、视图清空回 Idle，迟到同代数产物/失败被拒 | 2026-09-19 |
| failed_guard_matches_fetching_and_translating_only | 失败守卫只认两个在途态 | 给定取材失败与隐藏后的迟到失败，当采纳，则前者落 Error、后者被拒 | 2026-09-19 |
| modality_mismatch_preserves_pending_task | 模态错配保留待定任务 | 给定模态错配被拒后，当同代数合法输入到达，则仍可采纳 | 2026-09-19 |
| transport_failure_lands_in_error | 传输失败落错误态 | 给定取材通道不可用，当 fail_transport，则落 Error、失败视图无动作按钮、retry 为 None | 2026-09-19 |
| retryable_failure_keeps_task_and_retry_redispatches_it | 可重试失败保留任务 | 给定网络类失败，当落 Error，则消息点名类别、retry 同代数同任务新令牌重发且回流式视图 | 2026-09-19 |
| error_actions_follow_the_mapping_table | 错误动作按映射表 | 给定限流/鉴权/模态/配置类失败，当映射，则限流可重试，其余引导打开设置且不可重试 | 2026-09-19 |
| new_trigger_and_hide_supersede_the_retry_task | 新触发与隐藏取代重试 | 给定失败卡在场时新触发或隐藏，当发生，则 retry 返回 None | 2026-09-19 |
| error_messages_name_the_fix | 错误文案给出可执行指引 | 给定权限/鉴权/协议类错误，当生成文案，则给出可执行指引且保留诊断文本 | 2026-09-19 |

### crates/gloss-app/src/ui/fonts.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| cjk_fallback_appends_after_builtin_fonts | CJK 后备排在内置字体后 | 给定注入后备字体后的 FontDefinitions，当检查，则 CJK 后备排在比例与等宽两族内置字体之后 | 2026-09-19 |
| system_cjk_font_is_discoverable | 系统 CJK 字体可发现 | 给定真实系统，当执行 CJK 字体发现，则必须找到（依赖宿主机） | 2026-09-19 |

### crates/gloss-app/src/ui/settings.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| save_trims_endpoint_and_treats_blank_key_as_unchanged | 保存端点 trim、空白密钥视为未改 | 给定带空白的端点与空白密钥草稿，当 build_save，则端点被 trim、密钥按 Keep 上交 | 2026-09-19 |
| save_carries_the_key_outside_the_config | 密钥走带外通道不上配置 | 给定非空密钥草稿，当 build_save，则密钥走 KeyUpdate::Replace、不进配置（连 Debug 表示也不含） | 2026-09-19 |
| clear_key_is_deferred_to_save_and_revocable | 清除密钥延迟到保存且可撤销 | 给定「清除密钥」标记，当交互与保存，则删除延迟到保存生效、重新输入可撤销标记 | 2026-09-19 |
| open_copies_the_snapshot_into_the_draft | 打开设置拷贝快照进草稿 | 给定打开时的快照，当建草稿并随后改原配置，则草稿不跟随、可携带提示 | 2026-09-19 |
| source_labels_cover_all_variants | 输入源标签穷举 | 给定两种输入源，当映射标签，则穷举为「划词」「框选」 | 2026-09-19 |

### crates/gloss-platform/src/appearance/icon.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| install_degrades_to_false_off_the_main_thread | 非主线程安装图标优雅降级 | 给定非主线程调用与坏 PNG 字节，当 install，则不 panic 且如实返回 false | 2026-09-19 |

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
| empty_and_missing_selection_are_unavailable | 空与缺失映射不可用 | 给定空选区与 None，当映射，则 SelectionUnavailable | 2026-09-19 |
| api_disabled_after_trusted_check_maps_to_denied | 授权后 API 禁用仍映射拒绝 | 给定授权后 AX 报 APIDisabled，当映射，则仍是 AccessibilityDenied | 2026-09-19 |
| adjacent_error_codes_are_told_apart | 相邻错误码区分 | 给定相邻码 -25211（APIDisabled）与 -25212（NoValue），当映射，则前者 Denied、后者 Unavailable | 2026-09-19 |
| other_ax_errors_map_to_unavailable | 其余 AX 错误映射不可用 | 给定 -25206/-25213/-25200，当映射，则归 SelectionUnavailable | 2026-09-19 |

### crates/gloss-platform/src/selection/composite.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| fallback_is_lazy_on_ax_success | AX 成功时兜底惰性求值 | 给定 AX 成功，当 combine，则采纳 AX 结果且剪贴板兜底闭包不被求值 | 2026-09-19 |
| permission_denied_skips_fallback | 权限拒绝跳过兜底 | 给定 AccessibilityDenied，当 combine，则原样上抛且不兜底 | 2026-09-19 |
| unavailable_ax_falls_back_to_clipboard | AX 不可用落兜底 | 给定 SelectionUnavailable，当 combine，则落到兜底结果；兜底也失败则以兜底错误收口 | 2026-09-19 |

### crates/gloss-platform/src/selection/clipboard.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| change_is_detected_when_generation_bumps | 写入确认按代数探测 | 给定基线代数 0 与第 3 轮才变化的 probe，当 wait_for_write，则确认成功且恰好轮询 3 次 | 2026-09-19 |
| timeout_returns_false_without_hanging | 超时返回不悬挂 | 给定 probe 恒不变化，当到 deadline，则返回 false 且 2s 内返回 | 2026-09-19 |
| unavailable_probe_keeps_polling_until_deadline | probe 恒 None 轮询到期限 | 给定 probe 恒 None，当到 deadline，则返回 false、不提前放弃也不永久等 | 2026-09-19 |

### crates/gloss-platform/src/events/hotkey.rs

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| parses_modifier_combinations | 修饰键组合解析 | 给定 "Cmd+Shift+1"/"ctrl+alt+p"/"Option+K" 等，当 parse_trigger，则解析出 HotKey 与修饰键（大小写不敏感、Option≈Alt） | 2026-09-19 |
| rejects_malformed_triggers | 坏串拒绝解析 | 给定空串、"Cmd"、"Cmd+Foo"、"Cmd+1+2"、"++" 等，当解析，则返回 Err | 2026-09-19 |
| code_covers_digits_letters_and_named_keys | 键码覆盖数字字母命名键 | 给定数字/字母/F 键/命名键，当 parse_code，则映射到物理键码；未知键 None | 2026-09-19 |
| factory_bindings_are_all_registerable | 出厂绑定全部可注册 | 给定出厂默认绑定表，当逐条解析，则全部可解析且都带修饰键 | 2026-09-19 |
| degraded_registrar_keeps_parsed_table_and_stays_quiet | 降级注册器保持安静 | 给定 None 管理器，当装配 registrar，则不 panic、表保留全部出厂绑定、registered 为空、rebind 回报 0 | 2026-09-19 |
| registrar_construction_never_panics | 注册器构造与 poll 不 panic | 给定真管理器，当构造与 poll，则不 panic、poll 空（真实注册系统热键，测后注销） | 2026-09-19 |
| bare_keys_and_duplicates_are_rejected_before_registration | 裸键与重复在注册前拒绝 | 给定含裸键/重复/仅修饰键的表走公共构造，当装配，则非法条目不进表、重复收敛为一条 | 2026-09-19 |
| rejects_invalid_and_duplicate_triggers | 非法与重复触发过滤 | 给定混合合法/非法/重复的表走纯过滤路径，当过滤，则只留两条互不重复的合法绑定、无注销记录 | 2026-09-19 |
| distinct_spelling_of_one_physical_key_loses_to_the_first | 同物理键先到者占表 | 给定同物理键的两种写法 Cmd+Shift+1 / Super+Shift+1，当注册，则后者被拒、先到者占表 | 2026-09-19 |
| pump_observes_the_rebound_table | 泵观察重绑后的表 | 给定 rebind 换的新表，当 pump 读，则共享同一 Arc 表、只见新绑定；生效数至多 1 | 2026-09-19 |
| rebind_replaces_instead_of_appending | rebind 替换不追加 | 给定连续改小的绑定表，当连续 rebind，则表替换不追加、空表清空全部 | 2026-09-19 |
| binder_port_is_object_safe_and_reports_applied_count | 绑定端口对象安全并回报生效数 | 给定 Arc<dyn HotkeyBinder>，当对空表/裸键 rebind，则正常工作且回报 0 | 2026-09-19 |
| drain_pressed_skips_released_and_unknown_ids_without_stopping | 抽取按下事件跳过杂项 | 给定 Pressed/Released/未知 id 混合流，当 drain_pressed，则跳过后者继续收集同批有效 Pressed（两条） | 2026-09-19 |

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
| drag_release_emits_selection | 拖拽释放判定划词手势 | 给定按下→位移超阈值→释放序列，当驱动 GestureDetector，则判定一次划词手势 | 2026-09-19 |
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
| request_body_has_the_openai_envelope | 请求体 OpenAI 信封 | 给定 EngineRequest，当构造请求体，则 model/messages/stream 三键 wire 形态精确匹配 | 2026-09-19 |
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
