# 测试行为清单（`gloss-platform`）

本文件登记平台适配层（`gloss-platform`）的测试行为；总纲、人工测试与发版人工步骤见 `bdd.md`。

## 单元测试

### `crates/gloss-platform/src/common/clipboard_watch.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| advance_with_an_image_fires_once_and_records_the_change | 有图前进恰好发一次并记下观察 | 给定基线 7 的武装哨兵，当计数读数前进到 8 且判定存在图像，则产出一个观察事件、观察记录更新为 8；同读数的下一轮不重发也不复查图像存在性 | 2026-11-14 |
| text_advance_is_consumed_without_firing_or_rechecking | 文本前进消费基线不触发不复查 | 给定基线 7 的武装哨兵，当计数读数前进到 8 而判定无图像，则无观察事件、图像存在性恰查一次、观察记录不更新；同读数下一轮不再复查 | 2026-11-14 |
| queries_are_throttled_within_the_interval | 节流窗口内不查询 | 给定武装哨兵，当节奏点轮询后再在窗口内（100 毫秒小于 250 毫秒）轮询，则计数读数不被查询；到 250 毫秒节奏点才查下一轮 | 2026-11-14 |
| a_disabled_switch_never_queries_and_drops_the_baseline | 开关关零查询且基线作废 | 给定开关关的哨兵，当轮询，则计数读数与图像存在性都不被查询；重开后当轮只重记基线（关闭期间的旧内容不触发），新的前进照常触发 | 2026-11-14 |
| an_unarmed_source_arms_without_firing | 未武装只记基线不触发 | 给定未武装（基线缺失）的哨兵，当首轮轮询，则只记基线无观察事件；下一次前进照常触发 | 2026-11-14 |
| a_failed_count_query_stays_quiet_and_recovers | 读数失败静默且可恢复 | 给定武装哨兵，当计数读数返回空，则本轮静默；下一轮读数正常则照常触发 | 2026-11-14 |
| an_acquire_round_latch_outlives_the_command_and_swallows_the_round | 取材轮次闩锁按计数归因吞掉自身写盘并重记基线 | 给定武装哨兵（基线 7）且置位带计数归因的闩锁（前置 7、自报写盘 1），当轮询读到计数 8，则消费闩锁、图像存在性不被查询、无观察事件且基线就地重记到 8（自身写盘不触发）；下一次前进到 9 且有图照常触发恰一次 | 2026-11-14 |
| a_foreign_write_inside_the_round_fires_after_the_round | 轮次内外来写入不吞照常触发 | 给定武装哨兵（基线 7）且置位自报写盘 0 的闩锁（前置 7），当轮询读到计数 8 且判定有图（轮次内用户的真实复制，如截图工具拖拽既触发划词探测又写入图片），则放行走前进判定并产出观察事件、观察记录更新为 8（用户复制不被误吞） | 2026-11-14 |
| a_foreign_write_beyond_our_own_writes_fires | 超出自身写盘数的变化触发 | 给定武装哨兵（基线 7）且置位自报写盘 1 的闩锁（前置 7），当轮询读到计数 9 且判定有图（注入复制之外还有一次外来复制），则产出观察事件（9 减 7 等于 2 超出自身写盘 1） | 2026-11-14 |
| a_second_round_merges_into_the_pending_latch | 连续两轮写盘合并归因不误判外来 | 给定武装哨兵（基线 7）先后置位两轮闩锁（前置 7 写 2 与前置 9 写 2），当后一轮置位时合并为前置 7 写 4（不覆盖前一轮），轮询读到计数 11 则两轮自身写盘全部归因、吞掉并重记基线到 11、图像存在性不被查询 | 2026-11-14 |
| a_round_latch_on_an_unarmed_source_falls_back_to_the_round_pre_count | 未武装哨兵消费闩锁以轮次前置计数兜底 | 给定未武装（基线缺失）的哨兵与置位闩锁（前置 7、写 1），当轮询读到 8 则按前置计数兜底归因吞掉并武装基线到 8；换自报写盘 0 的闩锁读到 9（外来），则放行落入前进判定、当轮只武装基线到 9 不触发不查图像存在性 | 2026-11-14 |
| an_unreadable_count_at_the_round_latch_stays_quiet_and_rearms | 轮次消费遇读数失败保守吞掉并重记 | 给定武装哨兵且置位闩锁，当消费轮询时计数读数返回空，则静默、基线作废；随后的首轮只重记基线不触发，再下一次前进照常触发 | 2026-11-14 |
| a_text_round_restoring_an_image_board_must_not_refire_it | 划词恢复写回旧图不得再触发图片任务 | 给定武装哨兵已观察过一次图片复制（基线 8），当划词兜底轮次置位闩锁（前置 8、自报写盘 1：恢复写回）且计数前进到 9，则消费闩锁按计数归因吞掉、无观察事件、基线重记到 9；同读数不复查图像存在性，新的图片复制照常触发恰一次 | 2026-11-14 |
| a_restore_self_reporting_full_jumps_is_swallowed | 恢复写盘按真实跳变数自报即被吞 | 给定武装哨兵（基线 7）与按跳变口径置位的闩锁（前置 7、自报 5：取材 1 加清板 1 加放回 3），当轮询读到计数 12 且判定有图（板上正是被恢复的图），则闩锁按计数归因吞掉、无观察事件、基线重记到 12（恢复写盘不得被当成用户复制） | 2026-11-14 |
| a_restore_underreporting_jumps_fires_a_spurious_observation | 恢复写盘少报跳变即伪触发（反向钉子） | 给定武装哨兵（基线 7）与少报的闩锁（前置 7、自报 2），当轮询读到计数 12 且判定有图，则差额判为外来写入并产出观察事件（钉住写盘归因必须按真实跳变数自报：少报会让恢复的图伪装成用户复制） | 2026-11-14 |
| observer_mismatch_flags_the_race | 观察记录比对判竞态 | 给定观察记录，当现值与记录一致则匹配；记录更新后旧值不匹配、新值匹配（记录与现值之间被覆盖即竞态） | 2026-11-14 |

### `crates/gloss-platform/src/common/image.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| flavor_budget_rejects_bytes_over_the_limit | 交付字节上限先于头解析 | 给定恰好上限与超一字节的输入，当判预算与解码，则边界内通过、超限整体报 `ImageTooLarge`（头解析从未发生） | 2026-11-14 |
| header_dimensions_are_bounded_before_the_decode | 像素面积在整图解码前判 | 给定位图（PNG）编码后把头块尺寸补丁为宽 4097 高 4096（并重算头块校验和）的输入，当 `decode_to_png`，则 `ImageTooLarge`（整图解码从未发生——发生了会报解码失败） | 2026-11-14 |
| a_truncated_flavor_decodes_to_a_read_failure | 交付截断按读取失败 | 给定头块之后截断的位图（PNG），当 `decode_to_png`，则头可读、解码失败映射 `ImageUnavailable`（读取失败变体，指引重新复制） | 2026-11-14 |
| pixel_budget_rejects_area_over_the_limit | 像素面积上限边界 | 给定恰好宽高各 4096 像素与超一像素（宽 4097 高 4096）两种面积，当判预算，则前者在内、后者拒收 | 2026-11-14 |
| a_mismatched_rgba_buffer_is_a_read_failure | 透明像素（RGBA）缓冲与尺寸不符按读取失败 | 给定声明宽高各 2 像素但缓冲 3 字节的输入，当 `encode_png`，则报 `ImageUnavailable`（不 `panic`、不产残缺图） | 2026-11-14 |
| pixel_budget_is_checked_before_the_buffer | 面积检查先于缓冲检查 | 给定面积在预算内但缓冲为空的输入，当 `encode_png`，则按 `ImageUnavailable` 处理（面积超限的输入不会走到缓冲检查） | 2026-11-14 |
| a_covered_board_is_dropped_without_touching_the_content | 观察比对先于内容读取 | 给定记录值 7 而现值读到 8 的注入读数，当 `read_with`，则竞态静默丢弃（`Ok(None)`）且交付闭包从未被调——内容读取才触发授权或开板（一份锁两平台） | 2026-11-14 |
| an_undeliverable_flavor_is_dropped_quietly | 板交付不出数据按静默丢弃 | 给定记录值与现值一致但交付闭包缺数据的注入读数，当 `read_with`，则 `Ok(None)`（`warn` 留痕供 `just logs --level warn` 排查；系统（macOS）成因授权被拒、系统（Windows）成因板被短时占有或查询失败） | 2026-11-14 |
| an_over_limit_flavor_fails_with_the_size_error | 超限交付按尺寸错误上抛 | 给定记录值与现值一致但交付闭包报 `ImageTooLarge` 的注入读数，当 `read_with`，则原样上抛 `ImageTooLarge`（锁页与拷贝未发生） | 2026-11-14 |
| an_unobservable_board_is_a_read_failure | 板不可观察按读取失败 | 给定计数读不到的注入读数，当 `read_with`，则 `Err(ImageUnavailable)` 且不调交付闭包 | 2026-11-14 |
| a_matching_board_still_decodes_to_png | 观察一致照常取材解码 | 给定记录值与现值一致、交付为真实位图（PNG）字节的注入读数，当 `read_with`，则产出可解码回宽高各 2 像素的位图（PNG） | 2026-11-14 |
| byte_budget_rejects_length_over_the_limit | 位图（PNG）字节上限整体拒收 | 给定恰好上限与超一字节两种长度，当判预算，则前者在内、后者拒收，上限 20 兆二进制字节（MiB） | 2026-11-14 |
| downscaled_dims_preserve_aspect_and_the_cap | 降采样目标尺寸保比例收边 | 给定上限内与超限（正方、横长、竖长、极窄）的尺寸，当 `downscaled_dims`，则上限内返回 `None` 原样编码，超限按比例缩到最长边 1536、至少保留 1 像素 | 2026-11-14 |
| oversized_products_are_downscaled_to_the_side_cap | 超长边产物降采样出图 | 给定宽 2100 高 1000 的位图（PNG）交付，当 `decode_to_png`，则产出宽 1536 高 731 的可解码位图（PNG）（保比例收进上限，不拒收） | 2026-11-14 |

### `crates/gloss-platform/src/common/locale.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| locale_maps_language_names_to_locales | 语言名映射界面语言档位 | 给定 `zh-Hans-CN`、`zh_CN`、大写变体、`en-US`、`ja-JP` 与空值，当映射，则 `zh` 前缀（大小写不敏感）归中文档位、其余（含读不到语言名）归英文档位 | 2026-11-14 |

### `crates/gloss-platform/src/common/menu_labels.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| labels_follow_their_language_per_locale | 菜单词条守语言规范 | 给定中英两份菜单词条表与按档位查取，当逐条读，则中文表含汉字、英文表无汉字且两表词条均非空 | 2026-11-14 |

### `crates/gloss-platform/src/common/storage.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| save_then_load_round_trips_every_field | 全字段配置存取往返 | 给定全字段差异样例配置，当 `save`→`load`，则逐字段相等 | 2026-11-14 |
| load_generates_default_when_file_missing | 缺文件生成默认配置 | 给定缺文件目录，当 `load`，则返回出厂默认且落盘文件可再解析 | 2026-11-14 |
| load_creates_missing_directories | 缺目录自动创建 | 给定连父级都缺的目录，当 `load` 落盘，则目录被创建 | 2026-11-14 |
| composite_delegates_document_methods | 组合存储委托文档半边 | 给定 `CompositeConfigStore`，当 `save`→`load`，则完整委托文档半边、逐字段一致 | 2026-11-14 |
| composite_forwards_secret_methods_to_the_store | 组合存储转发密钥端口 | 给定内存桩密钥存储，当经组合体 `SecretStore` 的 `set_secret/secret/delete_secret`，则读回写入值、删除后 `None` | 2026-11-14 |
| persisted_document_stores_no_secret_material | 落盘文档不含密钥本体 | 给定 `save` 写出的配置文档（TOML），当检查内容，则含 `keychain_id` 但不含密钥本体 | 2026-11-14 |
| default_store_targets_standard_config_dir | 默认存储落在标准目录 | 给定默认构造与真实用户目录，当定位，则路径为 `gloss/config.toml`（`BaseDirs` 缺失时跳过） | 2026-11-14 |
| load_rejects_corrupt_file_and_quarantines_it | 损坏配置隔离降级 | 给定损坏配置，当 `load`，则报 `Config` 错、错误文本不转述内容、原文件字节原样挪入唯一 `.bak` | 2026-11-14 |
| legacy_document_gets_factory_endpoint_and_provider | 老配置回填出厂端点 | 给定老配置（空 `provider` 数组），当 `load`，则回填出厂端点与出厂 `provider`、模型 | 2026-11-14 |
| load_recovers_after_quarantine | 隔离后自愈 | 给定损坏文件已被隔离，当再次 `load`，则自愈落一份出厂默认 | 2026-11-14 |
| concurrent_saves_keep_the_document_parseable | 并发保存文件可解析 | 给定 4 线程各并发 `save` 20 次，当结束后 `load`，则文件完整可解析（主题为某一完整版本） | 2026-11-14 |
| error_line_reports_the_offending_line | 错误行号指向真实出错行 | 给定解析错误与 `span`，当报告，则行号落在真实出错行；`span` 缺失返回 `None` | 2026-11-14 |
| default_cache_ttl_matches_core_cache_semantics | 默认（TTL）与 `core` 语义一致 | 给定默认（TTL）字段，当与 `cache` 语义对照，则恰为 1 小时 | 2026-11-14 |

### `crates/gloss-platform/src/event_thread.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| tick_drains_commands_before_polling_sources | `tick` 内命令先全部抽干后才轮询事件源 | 给定两条已排队命令与一个记录调用顺序的事件源，当 `tick` 一轮，则两条命令先按序消费、事件源轮询恰在其后一次（取材轮次闩锁依赖的 `tick` 内顺序不变量） | 2026-11-07 |
| sink_send_tolerates_closed_receivers | `sink` 容忍关闭的接收端 | 给定接收端已关闭的 `sink`，当发送，则返回 `false` 不 `panic` | 2026-11-07 |
| successful_send_wakes_main_thread | 发送成功唤醒主线程 | 给定主线程在事件循环等待，当发送成功，则唤醒回调被触发；发送失败不唤醒 | 2026-11-07 |

### `crates/gloss-platform/src/macos/appearance/icon.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| install_reports_an_error_off_the_main_thread | 非主线程安装图标报错 | 给定非主线程调用与坏位图（PNG）字节，当 `install`，则不 `panic` 且如实返回 `Err` | 2026-10-30 |

### `crates/gloss-platform/src/macos/clipboard/image.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| png_and_tiff_flavors_decode_to_png | 标记图像（TIFF）与位图（PNG）双风格映射后解码出位图（PNG） | 给定同一宽高各 2 像素的透明像素（RGBA）位图（PNG）与标记图像（TIFF）编码字节，当映射与 `decode_to_png`，则各自产出可解码回宽高各 2 像素的位图（PNG） | 2026-11-14 |

### `crates/gloss-platform/src/macos/events/mod.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| commands_are_consumed_in_order_then_thread_exits | 命令按序消费线程干净退出 | 给定 16 条命令，当事件线程消费，则按序产出、`Sender` `drop` 后线程退出且无多余事件 | 2026-09-19 |
| late_commands_are_still_consumed | 晚到命令仍被消费 | 给定跨定时器周期（60 毫秒）晚到的命令，当消费，则仍被处理 | 2026-09-19 |
| sources_are_polled_and_forwarded | 事件源轮询转发 | 给定事件源每轮抽干，当 `tick`，则产出经 `sink` 送平台通道、命令照常消费 | 2026-09-19 |
| panicking_handler_does_not_kill_thread | 处理器 `panic` 不杀线程 | 给定会 `panic` 的命令处理器，当处理，则 `panic` 被拦截、后续命令照常消费 | 2026-09-19 |
| panicking_source_does_not_kill_thread | 事件源 `panic` 不杀线程 | 给定首轮 `panic` 的事件源，当轮次推进，则线程存活、后续轮次照常抽干 | 2026-09-19 |

### `crates/gloss-platform/src/macos/events/mouse.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| drag_release_emits_selection | 拖拽释放判定划词手势 | 给定按下→时长与位移双达标→释放序列，当驱动 `GestureDetector`，则判定一次划词手势 | 2026-09-30 |
| flick_shorter_than_the_minimum_press_is_filtered | 快甩被最短按压时长滤除 | 给定按压仅 10 毫秒但位移充足的事件对，当驱动，则不产出手势 | 2026-09-30 |
| press_duration_is_observed_at_the_boundary | 按压时长在边界被遵守 | 给定比下限短 1 纳秒与恰好等于下限（150 毫秒）的两对事件，当驱动，则前者滤除、后者判定 | 2026-09-30 |
| plain_click_and_jitter_do_not_trigger | 原地点击与抖动不触发 | 给定原地点击与阈值内抖动，当驱动，则不产出手势 | 2026-09-19 |
| displacement_comes_from_the_events_themselves | 位移取自事件自身坐标 | 给定长距离拖拽，当算位移，则取按下、释放事件自身坐标（释放位置即位移来源） | 2026-09-19 |
| state_resets_after_each_gesture | 每轮手势后状态重置 | 给定连续两轮完整手势，当驱动，则每轮后状态机重置、第二轮照常判定 | 2026-09-19 |
| stray_events_are_ignored | 杂散事件静默忽略 | 给定无按下的释放、双按下等杂散序列，当驱动，则静默忽略、后续手势照常 | 2026-09-19 |
| poll_drains_channel_through_state_machine | `poll` 抽干通道过状态机 | 给定事件通道，当 `poll`，则抽干并经状态机计数（第二次 `poll` 为 0） | 2026-09-19 |
| keyboard_events_are_never_subscribed | 键盘事件永不订阅 | 给定生产订阅掩码与 `classify`，当断言，则只有左键按下、抬起两位、键盘、滚轮、`flags` 各位不置（回归护栏） | 2026-09-19 |
| spawn_resolves_synchronously_and_startup_failure_stays_off_the_flag | `tap` 启动同步可知且失败不置运行中标志 | 给定真实 `tap` 启动（结果随测试机授权而异），当 `spawn` 返回，则立即返回且启动失败不经 `degraded` 标志表达（标志只属运行中失效） | 2026-10-06 |

### `crates/gloss-platform/src/macos/ffi/carbon.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| secure_input_query_links_and_answers_consistently | 安全输入查询可链接且自洽 | 给定真实系统，当连续查询两次安全输入，则两次一致——不断言系统的当前取值（那是环境事实），只要 `Carbon` 框架没链上或符号对不上，链接期就红 | 2026-09-26 |

### `crates/gloss-platform/src/macos/ffi/cf.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| cf_string_round_trips_back_to_utf8 | `CFString` 往返回环 | 给定 `C` 字符串，当构造 `CFString` 再转回（Rust）字符串，则内容逐字一致（含非 `ASCII`） | 2026-09-23 |
| string_from_rejects_non_string_objects | 非 `CFString` 拒绝转换 | 给定 `CFData`，当按字符串转换，则返回 `None`（先验类型） | 2026-09-23 |
| string_from_respects_the_byte_ceiling | 字节上界起作用 | 给定远低于所需的字节上界，当转换，则返回 `None` | 2026-09-23 |
| empty_cf_string_converts_to_empty_rust_string | 空串转换 | 给定空 `CFString`，当转换，则得到空（Rust）字符串而非 `None` | 2026-09-23 |

### `crates/gloss-platform/src/macos/menu.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| menu_install_degrades_off_the_main_thread | 菜单装配离主线程降级 | 给定非主线程测试线程，当调 `install`，则返回 `false` 且不装菜单 | 2026-10-29 |

### `crates/gloss-platform/src/macos/permissions.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| armed_watch_fires_once_then_disarms | 授权落定只产出一次并解除武装 | 给定武装的观察器与先拒后准的授权序列，当按查询间隔 `poll_at`，则落定时恰返回一次 `true`，此后解除武装不再查询 | 2026-10-09 |
| unarmed_watch_never_queries | 未武装观察器不查询 | 给定预检已就绪（未武装）的观察器，当 `poll_at`，则恒 `false` 且授权查询闭包不被调用 | 2026-10-09 |
| queries_are_throttled_to_the_interval | 真实查询按间隔节流 | 给定武装观察器，当以半个间隔与整间隔推进 `poll_at`，则半个间隔跳过查询、整间隔才再次查询 | 2026-10-09 |
| a_poll_exactly_at_the_interval_queries | 恰到间隔的轮询放行查询 | 给定上轮查询后恰过一个完整间隔，当 `poll_at`，则再次执行真实查询 | 2026-10-09 |

### `crates/gloss-platform/src/macos/scene.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| scene_probe_answers_with_a_coherent_snapshot | 场景探针给出一致快照 | 给定真实系统，当读一次场景事实，则若报出前台应用则其身份字段非空（不断言安全输入取值——那是环境事实，任何进程持有它都会变） | 2026-09-26 |

### `crates/gloss-platform/src/macos/selection/accessibility.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| untrusted_maps_to_accessibility_denied | 未授权一律映射拒绝 | 给定未授权进程的取值结果，当 `interpret` 映射，则一律 `AccessibilityDenied` | 2026-09-19 |
| selected_text_passes_through_verbatim | 选中文本原样透传 | 给定读到的选中文本（含纯空白），当映射，则原样透传不裁剪 | 2026-09-19 |
| an_empty_selection_is_empty_but_a_missing_value_stays_unavailable | 空选区与读不到分开建档 | 给定空串与 `None`（超上限、非字符串），当映射，则前者 `SelectionEmpty`（没选东西）、后者 `SelectionUnavailable`（选了但读不动，兜底仍值得一试）；`NoValue` 归前者 | 2026-09-30 |
| api_disabled_after_trusted_check_maps_to_denied | 授权后（API）禁用仍映射拒绝 | 给定授权后 `AX` 报 `APIDisabled`，当映射，则仍是 `AccessibilityDenied` | 2026-09-19 |
| adjacent_error_codes_are_told_apart | 相邻错误码区分 | 给定相邻的负数错误码 25211（`APIDisabled`）与 25212（`NoValue`），当映射，则前者 `Denied`、后者 `SelectionEmpty`（错一位语义就反转） | 2026-09-30 |
| other_ax_errors_map_to_unavailable | 其余 `AX` 错误映射不可用 | 给定负数错误码 25206、25213、25200，当映射，则归 `SelectionUnavailable` | 2026-09-19 |

### `crates/gloss-platform/src/macos/selection/activation.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| chromium_family_frameworks_are_recognized | `Chromium` 家族框架名被识别 | 给定 `Electron/Chrome/Brave/CEF` 形态的框架名，当 `is_chromium_family`，则一律识别为 `Chromium` 宿主 | 2026-10-12 |
| unrelated_frameworks_are_not_chromium | 无关框架不误判 | 给定空目录与 `Mantle/Squirrel/ReactiveObjC` 等第三方框架及缺“空格加 `Framework`”后缀的条目，当 `is_chromium_family`，则一律不判为 `Chromium` 宿主 | 2026-10-12 |
| the_ledger_settles_definitive_verdicts_exactly_once | 确定性结论只记账一次 | 给定默认记账表，当 `settle` 某 `pid` 后再查，则该 `pid` 本会话不再待评估 | 2026-10-12 |
| an_unsettled_pid_stays_pending_for_the_next_gesture | 未记账的 `pid` 保持待评估 | 给定默认记账表与未曾 `settle` 的 `pid`，当反复查询 `is_pending`，则始终待评估（注入瞬时失败不锁门，下一手势自然重试） | 2026-10-12 |
| a_rescued_pid_is_marked_for_rewake | 唤醒常客按 `pid` 记账 | 给定默认记账表，当 `settle` 并 `mark_rescued` 某 `pid`，则该 `pid` 不再待评估且成为唤醒常客（重注入放行），其它 `pid` 不受影响 | 2026-10-16 |

### `crates/gloss-platform/src/macos/selection/clipboard.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| change_is_detected_when_generation_bumps | 写入确认按代数探测 | 给定基线代数 0 与第 3 轮才变化的 `probe`，当 `wait_for_write`，则确认成功且恰好轮询 3 次 | 2026-09-19 |
| timeout_returns_false_without_hanging | 超时返回不悬挂 | 给定 `probe` 恒不变化，当到 `deadline`，则返回 `false` 且 2 秒内返回 | 2026-09-19 |
| unavailable_probe_keeps_polling_until_deadline | `probe` 恒 `None` 轮询到期限 | 给定 `probe` 恒 `None`，当到 `deadline`，则返回 `false`、不提前放弃也不永久等 | 2026-09-19 |
| injection_failure_releases_every_pressed_key_in_reverse | 注入失败按已按下键逆序补放 | 给定复制快捷键序列（修饰键按下 → `C` 按下 → `C` 释放 → 修饰键释放），当分别在第 1 至 4 个事件失败，则补放列表依次为空、`[modifier]`、`[C, modifier]`、`[C, modifier]`（逆序；重复 `key-up` 幂等） | 2026-10-17 |
| retry_stops_at_the_first_success | 重试首次成功即止 | 给定第 1 次即成功的 `once` 与 3 次上限，当 `restore_with_retry`，则只调 1 次、成功返回且清板数记 1 | 2026-10-19 |
| retry_counts_the_clears_of_every_attempt_until_success | 重试累加各次尝试的实际清板数 | 给定依次“清 1 失败、清 0 失败、清 1 成功”的 `once` 与 3 次上限，当 `restore_with_retry`，则调满 3 次、成功返回且清板数记 2 | 2026-10-19 |
| exhausted_retries_carry_the_last_attempt_stage_status_and_total_clears | 重试耗尽保留末次环节状态码与总清板数 | 给定两次皆失败的 `once`（首次 `Alloc/5` 清 1、末次 `Clear/42` 清 0），当 `restore_with_retry`，则失败返回、清板数记 1、环节状态码取末次（`Clear/42`） | 2026-10-19 |

### `crates/gloss-platform/src/macos/selection/composite.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| fallback_is_lazy_on_ax_success | `AX` 成功时兜底惰性求值 | 给定 `AX` 成功，当 `combine`，则采纳 `AX` 结果且剪贴板兜底闭包不被求值 | 2026-10-01 |
| permission_denied_skips_fallback | 权限拒绝跳过兜底 | 给定 `AccessibilityDenied`，当 `combine`，则原样上抛且不兜底 | 2026-10-01 |
| empty_selection_settles_then_falls_back_to_the_clipboard | 空选区让渡补读后再落兜底 | 给定持续 `SelectionEmpty` 的 `AX` 读、零让渡策略与拒绝唤醒的激活闭包，当 `combine`，则补读至节奏表耗尽（恰 4 读）、激活问一次落空后才落一次剪贴板兜底并采纳其结果（确认写入把关，陈旧剪贴板不得冒充本次选区） | 2026-10-16 |
| empty_then_ready_ax_read_adopts_the_late_selection | 补读等到迟到的选区 | 给定首次空、二次有值的 `AX` 读，当 `combine`，则采纳第二次读到的选区且不兜底 | 2026-10-01 |
| settle_budget_is_bounded_and_positive | 让渡节奏表有界且为正 | 给定节奏表内的尝试序号，当 `settle_delay`，则逐位返回正的几何让渡时长；表耗尽（含 `usize::MAX`）返回 `None` | 2026-10-16 |
| unavailable_ax_falls_back_to_clipboard | `AX` 不可用过激活闸门后落兜底 | 给定 `SelectionUnavailable`，当 `combine`，则激活恰被问一次、落空后落到兜底结果；兜底也失败则以兜底错误收口 | 2026-10-12 |
| acquired_text_is_trimmed_at_the_boundary | 取材入口修剪两端空白 | 给定两端带空白的选中文本，当 `normalize`，则返回修剪后的文本；本无空白的文本原样放行 | 2026-10-11 |
| acquired_text_keeps_interior_whitespace_verbatim | 入口修剪只动两端不动内部 | 给定内部含换行、制表、空格的选中文本，当 `normalize`，则内部空白逐字保留、只去两端 | 2026-10-11 |
| pure_whitespace_selection_maps_to_selection_empty | 纯空白选区按空选区收口 | 给定纯空白（空格、换行、制表、全角空格）与空串的读取产物，当 `normalize`，则一律 `SelectionEmpty`（误滑走既有静默丢弃语义） | 2026-10-11 |
| success_and_denial_never_consult_the_activator | 激活只在 `AX` 让步后被问及 | 给定 `AX` 成功与 `AccessibilityDenied` 两种读取，当 `combine`，则激活闭包一次都不被求值（成功直接采纳、拒绝原样上抛） | 2026-10-12 |
| a_declined_wake_goes_straight_to_the_clipboard | 激活落空直接落兜底 | 给定持续 `SelectionEmpty` 的 `AX` 读、零让渡策略与拒绝唤醒的激活闭包，当 `combine`，则让渡节奏表跑满、激活问一次即落兜底并采纳 | 2026-10-16 |
| a_fresh_wake_re_reads_and_adopts_the_late_selection | 刚激活的树补读即采纳 | 给定首次空、二次有值的 `AX` 读与肯定唤醒的激活闭包，当 `combine`，则激活后第一次补读就采纳迟到的选区且不落兜底 | 2026-10-12 |
| a_woken_tree_that_stays_silent_falls_back_after_the_budget | 激活预算跑满仍空落兜底 | 给定持续 `SelectionEmpty` 的 `AX` 读、零让渡与零激活等待、肯定唤醒的激活闭包，当 `combine`，则让渡预算与激活预算各自跑满（恰 10 读）才落兜底 | 2026-10-12 |
| a_woken_tree_that_errors_does_not_burn_the_activation_budget | 激活后读到硬错不烧预算 | 给定 `SelectionUnavailable` 的 `AX` 读与肯定唤醒的激活闭包，当 `combine`，则激活后只补读一次即落兜底（读不到不等建树） | 2026-10-12 |
| a_woken_tree_that_loses_permission_propagates_the_denial | 激活补读中权限缺失原样上抛 | 给定让渡预算跑满仍空、激活补读时 `AccessibilityDenied` 的 `AX` 读与肯定唤醒的激活闭包，当 `combine`，则让渡环跑满进入激活补读后拒绝原样上抛且不落兜底（注入会被系统静默忽略，兜底白等） | 2026-10-12 |
| activation_budget_is_bounded_and_positive | 激活预算有界且为正 | 给定预算内、末次与超界的尝试序号，当 `activation_delay`，则预算内返回正的等待时长、预算用尽（含 `usize::MAX`）返回 `None` | 2026-10-12 |
| a_rescued_host_skips_the_settle_schedule | 唤醒常客宿主跳过让渡环 | 给定首次空、二次有值的 `AX` 读、唤醒常客判定为真与肯定唤醒的激活闭包，当 `combine`，则首次空读直接进激活补读（恰 2 读）并采纳补读产物，让渡节奏表未跑 | 2026-10-16 |
| the_wait_budget_ends_the_settling_early | 等待预算耗尽让渡段转激活闸门 | 给定首次空、二次有值的 `AX` 读与已耗尽的等待预算，当 `combine`，则让渡环在首次等待前即止、直接进激活补读采纳（恰 2 读，不落兜底） | 2026-10-16 |
| the_wait_budget_ends_the_activation_re_reads | 等待预算耗尽激活段落兜底 | 给定持续 `SelectionEmpty` 的 `AX` 读、零等待策略、肯定唤醒的激活闭包与已耗尽的等待预算，当 `combine`，则激活后首次补读即止落兜底并采纳 | 2026-10-16 |
| a_rescued_host_with_a_declined_wake_falls_back_without_settling | 唤醒常客宿主唤醒落空不跑让渡环 | 给定持续空选区的 `AX` 读、唤醒常客判定为真与拒绝唤醒的激活闭包，当 `combine`，则跳过让渡节奏表（恰 1 读）直接落兜底并采纳（常客跳让渡与唤醒裁决两个事实互不遮蔽） | 2026-11-14 |

### `crates/gloss-platform/src/macos/storage/keychain.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| missing_entry_reads_as_none（keychain） | 缺条目读为 `None` | 给定先删除预清的条目，当 `get`，则 `None` 而非错误 | 2026-09-19 |
| delete_missing_entry_is_ok（keychain） | 删除缺条目幂等成功 | 给定不存在的条目，当 `delete`，则幂等成功 | 2026-09-19 |
| cached_reads_stay_consistent_with_writes（keychain） | 缓存读写一致 | 给定写后读与删除后读，当经克隆共享缓存，则写后读新值、删除后克隆读 `None`（不走真实 `keychain` 写入） | 2026-09-19 |
| racing_first_reads_settle_on_the_cached_value | 并发首读共享同一次未命中 | 给定不存在条目的全新实例（空缓存直逼未命中路径）与 4 线程并发 `get`，当首读竞赛，则全部读到一致结果、不悬挂，且底层读恰好 1 次（测试构建的读计数接缝：首读按 `key` 过闸门串行，授权框阻塞期间并发读者等缓存回填而非各撞一次授权框） | 2026-10-08 |

### `crates/gloss-platform/src/windows/clipboard/image.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| dib_dibv5_and_png_flavors_decode_to_png | 位图家族与位图（PNG）三风格映射后解码出位图（PNG） | 给定同一宽高各 2 像素的系统（Windows）布局位图（`CF_DIB`、`CF_DIBV5` 共用同一份设备无关位图（DIB）流）与位图（PNG）编码字节，当映射与 `decode_to_png`，则各自产出可解码回宽高各 2 像素的位图（PNG） | 2026-11-14 |
| dib_decode_keeps_row_order_and_channels | 合成文件头解码保行序与通道序 | 给定自下而上存储、蓝绿红透明字节序的两行测试位图（首存储行绿、末存储行蓝），当映射与 `decode_to_png`，则产物首行蓝、次行绿（行序与通道序端到端不翻转） | 2026-11-14 |
| dib_pixel_offset_covers_header_variants | 像素偏移公式覆盖信息头变体 | 给定版本 5（124 字节）、版本 4（108 字节）、基线 40 字节（无调色板）、基线加位域（掩码块随头）、8 位调色板（零色数取位深满额、声明色数优先）各一例，当 `dib_pixel_offset`，则偏移依次为头尺寸、头尺寸、头尺寸、头尺寸加 12、头尺寸加调色板字节 | 2026-11-07 |
| dib_pixel_offset_rejects_malformed_headers | 非法信息头整体拒算 | 给定截断头、核心 12 字节头、调色板字节溢出三例，当 `dib_pixel_offset`，则一律返回空（核心头不在支持面，溢出算术不出错值） | 2026-11-07 |
| dib_a_malformed_flavor_maps_to_a_read_failure | 非法位图家族交付按读取失败 | 给定截断到信息头中段的设备无关位图（DIB）流，当映射，则报 `ImageUnavailable`（位图文件头拒绝合成，读取失败变体随共享管线指引重新复制） | 2026-11-14 |

### `crates/gloss-platform/src/windows/events/mod.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| pump_commands_are_consumed_in_order_then_thread_exits | 泵线程按序消费后退出 | 给定真实泵线程与十六命令，当消费完后断开命令通道，则事件按序回传、线程退出且退出后无事件 | 2026-11-06 |
| pump_late_commands_are_still_consumed | 迟来命令仍被消费 | 给定泵线程在先批命令消费完后的新命令，当跨节拍再发一条，则新命令照常消费回传 | 2026-11-06 |
| pump_sources_are_polled_and_forwarded | 事件源轮询并转发 | 给定产出两事件的事件源，当泵节拍轮询，则两事件转发到平台通道且命令回传不受影响 | 2026-11-06 |
| pump_panicking_handler_does_not_kill_thread | 命令处理器恐慌不杀线程 | 给定首命令即恐慌的处理器，当连发两命令，则线程存活且第二条事件照常回传 | 2026-11-06 |
| pump_panicking_source_does_not_kill_thread | 事件源恐慌不杀线程 | 给定首拍即恐慌后自愈的事件源，当下拍产出事件，则线程存活且事件照常转发 | 2026-11-06 |

### `crates/gloss-platform/src/windows/events/mouse.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| hook_drag_release_emits_selection | 拖拽释放判定划词手势 | 给定按下到时长与位移双达标后释放的事件序，当驱动 `GestureDetector`，则判定一次划词手势 | 2026-11-06 |
| hook_plain_click_and_jitter_do_not_trigger | 普通点击与抖动不触发 | 给定零位移长按与阈值内抖动两组事件序，当驱动判定器，则都不出手势 | 2026-11-06 |
| hook_flick_shorter_than_the_minimum_press_is_filtered | 瞬时甩动被时长下限滤掉 | 给定十毫秒大幅位移的按下释放序，当驱动判定器，则不出手势 | 2026-11-06 |
| hook_press_duration_is_observed_at_the_boundary | 按压时长按毫秒边界判定 | 给定差一毫秒与恰好达下限的两组时长，当驱动判定器，则差一毫秒过滤、恰好达标放行 | 2026-11-06 |
| hook_displacement_comes_from_the_events_themselves | 位移取释放事件自身坐标 | 给定按下点与释放点分离的事件序，当驱动判定器，则手势坐标为释放事件自身位置 | 2026-11-06 |
| hook_state_resets_after_each_gesture | 每次手势后状态复位 | 给定成功手势后的第二次按压释放序，当驱动判定器，则旧按压不参与第二次判定 | 2026-11-06 |
| hook_stray_events_are_ignored | 野事件被忽略 | 给定无按压的释放与重复按下等野事件序，当驱动判定器，则野事件不出手势、后续正常序照常判定 | 2026-11-06 |
| hook_poll_drains_channel_through_state_machine | 轮询侧抽干通道经状态机 | 给定通道里的按下释放对，当 `GestureDetector` 轮询，则产出一次手势且再轮询为空 | 2026-11-06 |
| hook_spawn_resolves_synchronously_and_startup_failure_stays_off_the_flag | 建立同步握手且启动失败不复位降级标志 | 当 `MouseSource::spawn`，则同步返回且降级标志为假（真机装真实钩子，随测试进程消亡） | 2026-11-06 |
| hook_mouse_messages_are_the_only_subscription | 订阅面只有左键按下释放 | 给定键盘、移动、右键与未知消息号，当按消息号分类，则仅左键按下释放有分类、其余为空 | 2026-11-06 |

### `crates/gloss-platform/src/windows/ffi/inject.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| copy_sequence_presses_modifier_first_and_releases_it_last | 复制键序修饰键首尾 | 给定 `copy_sequence`，当读四步键序，则修饰键按下、键按下、键释放、修饰键释放依次排列 | 2026-11-06 |
| injection_failure_releases_pressed_keys_in_reverse | 注入失败按已按下键逆序补放 | 给定复制键序，当分别在第 1 至 4 步失败，则补放列表依次为空、修饰键、键与修饰键（逆序，重复的抬起事件幂等） | 2026-11-06 |

### `crates/gloss-platform/src/windows/menu.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| tray_icon_asset_decodes_into_the_brand_bitmap | 品牌资产解码为托盘位图 | 给定内嵌品牌 `PNG` 资产，当解码为原始位图，则尺寸为 256×256 且字节面为满 `RGBA` 覆盖 | 2026-11-07 |

### `crates/gloss-platform/src/windows/scene.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| image_name_survives_full_path_extraction | 进程映像名取路径末段去扩展名 | 给定反斜杠、正斜杠、裸名、多点文件名与裸目录五例路径，当 `image_name`，则依次取到映像名与去扩展名进程名、裸目录返回空 | 2026-11-06 |
| front_app_probe_reports_a_usable_identity | 探针快照连贯且安全输入恒关 | 当读一次场景事实，则安全输入态恒为假，报告出的前台应用至少带一个可用标识 | 2026-11-06 |

### `crates/gloss-platform/src/windows/selection/clipboard.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| seq_change_confirms_when_generation_bumps | 写入确认按序列号差探测 | 给定基线序列号 0 与第 3 轮才前进的探针，当 `wait_for_write`，则确认成功且恰好轮询 3 次 | 2026-11-06 |
| seq_timeout_returns_false_without_hanging | 确认超时返回不悬挂 | 给定恒不前进的探针，当到截止时间，则返回否且两秒内返回 | 2026-11-06 |
| seq_unavailable_probe_keeps_polling_until_deadline | 探针不可用轮询到期限 | 给定恒空的探针，当到截止时间，则返回否、不提前放弃也不永久等 | 2026-11-06 |

### `crates/gloss-platform/src/windows/selection/clipboard/restore.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| board_retry_stops_at_the_first_success | 恢复重试首次成功即止 | 给定第 1 次即成功的单次恢复与 3 次上限，当 `restore_with_retry`，则只调 1 次、成功返回且写盘数记 1 | 2026-11-06 |
| board_retry_accumulates_the_writes_of_every_attempt_until_success | 恢复重试累加各次写盘数 | 给定两次失败后第三次成功的单次恢复与 3 次上限，当 `restore_with_retry`，则调满 3 次、成功返回且写盘数记 1（失败轮不写盘） | 2026-11-06 |
| board_retry_exhaustion_carries_the_last_stage_and_total_writes | 恢复耗尽保留末次环节与累计写盘数 | 给定两次皆失败的单次恢复（首次写环节、末次清环节），当 `restore_with_retry`，则失败返回且环节取末次（清环节） | 2026-11-06 |

### `crates/gloss-platform/src/windows/selection/clipboard/snapshot.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| snapshot_keeps_readable_text_verbatim | 可读文本逐字保全 | 给定可读文本读取结果与任意格式计数，当 `snapshot_decision`，则文本快照逐字保全 | 2026-11-06 |
| snapshot_accepts_a_confirmed_empty_board | 确认空板保全为空板 | 给定无内容读取结果与零格式计数，当 `snapshot_decision`，则空板快照成立（恢复轮清板） | 2026-11-06 |
| snapshot_preserves_read_image_flavors_for_write_back | 图像风格原字节保全供写回 | 给定无内容读取结果与非零格式计数、且图像风格捕获可读，当 `snapshot_decision`，则按捕获的风格原字节保全（图片挂板的兜底轮照常前进） | 2026-11-10 |
| snapshot_declines_a_non_text_board_it_cannot_restore | 保不住的风格全跳过才放弃兜底 | 给定无内容读取结果与非零格式计数、且图像风格一个都捕获不出，当 `snapshot_decision`，则放弃（注入后原内容无法如实写回）。沿革：原判据为非文本板即整体放弃，2026-11-10 随图像风格保全收窄为全跳过才放弃 | 2026-11-10 |
| snapshot_declines_unreadable_board_state | 板状态读不出整体放弃 | 给定无内容读取结果与读不出的格式清单、以及其它文本读取失败两例，当 `snapshot_decision`，则一律放弃兜底 | 2026-11-06 |

### `crates/gloss-platform/src/windows/selection/composite.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| combined_fallback_is_lazy_on_uia_success | 主路成功不触兜底 | 给定成功的用户界面自动化读取与计数兜底，当 `combine`，则采纳主路产物且兜底零调用 | 2026-11-06 |
| combined_empty_selection_settles_then_falls_back_to_the_clipboard | 空选区让渡跑满再落兜底 | 给定持续空选区的主读与零等待策略，当 `combine`，则主读按节奏表跑满（恰 4 读）后兜底恰一次并采纳 | 2026-11-06 |
| combined_empty_then_ready_read_adopts_the_late_selection | 迟到选区被让渡补读采纳 | 给定首次空、二次有值的主读，当 `combine`，则恰 2 读采纳迟到选区且不落兜底 | 2026-11-06 |
| combined_unavailable_uia_falls_back_to_clipboard | 主路不可用直接落兜底 | 给定读取不可用的主读，当 `combine` 两例（兜底成与败），则成例采纳兜底产物、败例如实上抛 | 2026-11-06 |
| combined_the_wait_budget_ends_the_settling_early | 等待预算耗尽缩短让渡段 | 给定持续空选区的主读与已耗尽的等待预算，当 `combine`，则恰 1 读（本平台无激活闸门）即落兜底并采纳 | 2026-11-06 |
| combined_accessibility_denied_falls_back_like_any_other_failure | 权限拒绝照常落兜底 | 给定 `AccessibilityDenied` 的主读与兜底两例（成与败），当 `combine`，则照常落兜底——成例采纳兜底产物、败例如实上抛（本平台无权限语义，拒绝不短路） | 2026-11-14 |
| combined_settle_schedule_is_bounded_and_positive | 让渡节奏表有界且全正 | 给定预算内、末次与超界的尝试序号，当 `settle_delay`，则预算内返回正等待、耗尽（含 `usize::MAX`）返回空 | 2026-11-06 |
| combined_acquired_text_is_trimmed_at_the_boundary | 取材产物入口修剪两端空白 | 给定两端带空白的文本，当 `normalize`，则两端空白修剪且内部保留 | 2026-11-06 |
| combined_acquired_text_keeps_interior_whitespace_verbatim | 内部空白逐字保留 | 给定内部混排空白的文本，当 `normalize`，则内部空白逐字保留 | 2026-11-06 |
| combined_pure_whitespace_selection_maps_to_selection_empty | 纯空白选区按空选区上抛 | 给定四例纯空白（空格、换行制表、全角空格、空串），当 `normalize`，则一律报空选区（下游只认一份已修剪文本） | 2026-11-06 |

### `crates/gloss-platform/src/windows/selection/uia.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| uia_selected_text_passes_through_verbatim | 选区文本逐字采纳 | 给定非空读取结果（含两端空白原文），当 `interpret`，则文本逐字上抛（修剪在组合通道入口） | 2026-11-06 |
| uia_an_empty_selection_is_empty_but_a_missing_value_stays_unavailable | 空串与超限拒取分开报 | 给定空串与空值（超上限）两类结果，当 `interpret`，则空串报空选区（可让渡补读）、空值报读取不可用（落兜底） | 2026-11-06 |
| uia_unsupported_patterns_and_failed_calls_stay_unavailable | 模式缺席与调用失败报不可用 | 给定模式缺席标记码与任意失败码（含零），当 `interpret`，则一律报读取不可用（本平台无权限拒绝语义） | 2026-11-06 |

### `crates/gloss-platform/src/windows/storage/cred.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| missing_entry_reads_as_none（cred） | 缺条目读为 `None` | 给定先删除预清的条目，当 `get`，则 `None` 而非错误 | 2026-11-10 |
| delete_missing_entry_is_ok（cred） | 删除缺条目幂等成功 | 给定不存在的条目，当 `delete`，则幂等成功 | 2026-11-10 |
| cached_reads_stay_consistent_with_writes（cred） | 缓存读写一致 | 给定写后读与删除后读，当经克隆共享缓存，则写后读新值、删除后克隆读 `None`（不走真实凭据管理器写入） | 2026-11-10 |
| racing_first_reads_settle_on_one_answer | 并发首读结果一致 | 给定不存在条目的全新实例与 4 线程并发 `get`，当首读竞赛，则全部读到一致结果、不悬挂，缓存随后读保持一致（凭据读取无授权框，读数不做单次收敛） | 2026-11-10 |
