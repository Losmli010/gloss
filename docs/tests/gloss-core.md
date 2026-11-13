# 测试行为清单（`gloss-core`）

本文件登记核心库（`gloss-core`）的测试行为；总纲、人工测试与发版人工步骤见 `bdd.md`。

## 单元测试

### `crates/gloss-core/src/contract/config_handle.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| load_takes_first_snapshot_from_store | `load` 的首份快照来源 | 给定存储已有配置，当 `ConfigHandle::load`，则首份快照即存储内容 | 2026-09-19 |
| save_writes_through_and_swaps_snapshot | `save` 穿透落盘并换快照 | 给定一次 `save`，当执行，则磁盘文档与运行时快照同时为新版 | 2026-09-19 |
| failed_save_keeps_previous_snapshot | 失败保存保持旧快照 | 给定落盘必失败的存储，当 `save`，则错误上抛且快照保持旧版 | 2026-09-19 |
| load_or_default_uses_store_snapshot_or_falls_back | `load_or_default` 的两路 | 给定健康与损坏两种存储，当 `load_or_default`，则分别取存储快照与出厂默认 | 2026-09-19 |
| load_propagates_store_failure | 加载失败原样上抛 | 给定加载失败，当 `ConfigHandle::load`，则 `Config` 错误原样上抛 | 2026-09-19 |
| saved_version_is_visible_from_another_thread | 保存结果跨线程可见 | 给定另一线程的读者，当 `save` 完成、信号放行后读取，则必见新版本 | 2026-09-19 |
| concurrent_readers_never_see_a_mixed_version | 并发读者不见混合版本 | 给定 4 读者与 200 轮 `A/B` 交替保存，当读者全程校验，则每份快照都是完整版本且末次保存胜出 | 2026-09-19 |
| concurrent_saves_keep_disk_and_snapshot_in_step | 并发保存盘与快照同步 | 给定 4 写者并发各存 50 次，当全部结束，则磁盘与内存快照同版 | 2026-09-19 |

### `crates/gloss-core/src/domain/config.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| factory_defaults_match_spec | 出厂默认符合规格 | 给定出厂 `Config`，当逐字段抽查，则默认语言、任务、提供商、文本模型等全部符合规格 | 2026-09-19 |
| config_round_trips_through_serde | 配置 `serde` 往返无损 | 给定含两变体目标语言的完整配置，当 `serde_json` 往返，则无损 | 2026-10-30 |
| partial_document_fills_factory_defaults | 部分文档补全出厂默认 | 给定只写 `theme` 的 JSON，当加载，则该字段保留、其余走出厂默认（含全部 `kind` 启用） | 2026-09-19 |
| retired_fields_are_ignored_on_load | 退役字段仍能加载 | 给定含 `guard_enabled`、`guard_blocked_apps`、`hotkey_bindings` 的旧配置（字段已从 `Config` 移除），当加载，则照常读出、已知字段取值不变、缺字段仍回出厂默认——旧版本落盘不会被当成非法配置隔离降级 | 2026-10-03 |
| retired_target_lang_values_degrade_to_the_default_with_a_trace | 退役目标语言值有痕降级 | 给定 `target_lang` 为日语、韩语、法语、自定义名与非法形态的存量配置，当反序列化，则目标语言回落缺省中文、其余字段不受牵连，且每次降级留警告痕迹 | 2026-10-30 |
| supported_target_lang_values_still_parse | 在册目标语言值照常解析 | 给定 `target_lang` 为 `Zh` 与 `En` 的配置，当反序列化，则各自解析为对应变体 | 2026-10-30 |
| language_resolves_to_locale | 界面语言落定具体 `locale` | 给定 `System/Zh/En` 三态与注入的系统语言，当解析界面语言，则 `System` 取系统语言、显式选择不被系统语言覆盖（同一处取值供 `UI` 文案表与 `prompt` 模板选表） | 2026-09-22 |
| lookups_prefer_later_entries | 重复条目查找后者胜出 | 给定重复 `kind` 的多条目，当查找，则后条胜出、未配置为 `None`、未知 `provider` 无 `keychain` `id` | 2026-09-19 |
| base_url_validation_rejects_structural_problems | `Base URL` 结构性拒绝 | 给定空串、非 `https`、缺 `scheme`、缺 `host`、内嵌凭据、带 `query` 或 `fragment`、中部空白的地址，当 `validate_base_url`，则按类别返回 `Err` | 2026-09-22 |
| base_url_validation_accepts_legal_addresses | `Base URL` 合法放行 | 给定 `https` 地址（含首尾空白、无路径、带端口），当 `validate_base_url`，则 `Ok` | 2026-09-22 |
| missing_fields_fall_back_to_defaults_on_deserialize | 缺字段回落出厂默认 | 给定缺 `language` 字段的配置（JSON），当反序列化，则 `language` 按出厂跟随系统补齐 | 2026-09-22 |

### `crates/gloss-core/src/domain/guard.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| scene_gate_blocks_secure_input_and_listed_apps | 场景闸门的两路拦截与优先级 | 给定安全输入开启、前台应用在内建名单里、两者同时成立、名单外的应用、无事实五种场景，当判闸门，则各返回对应因由（两者同时成立时报安全输入、名单外与无事实放行） | 2026-09-26 |
| entry_matching_covers_every_identity_an_app_can_offer | 名单匹配按身份逐字比对 | 给定内建名单，当经公共入口 `trigger_block` 判闸门，则 `bundle` `id` 命中（`ASCII` 大小写不敏感、返回名单原文）、显示名只在名单携带同名（含系统名单规范化键）时命中、名单外的应用与无身份的应用都放行 | 2026-11-07 |
| token_prefixes_are_detected | 各类令牌前缀识别 | 给定 `sk-/ghp_/xoxb-/AKIA/Bearer` 各形态、带空白与引号包裹的令牌、以及 `NAME=<token>` 与 `AUTH=Bearer <token>` 两种赋值形态（`.env` 行、`shell` 导出与 `Authorization` 头），当检测，则都命中 `Token` | 2026-09-26 |
| tokens_embedded_in_structured_text_are_detected | 嵌在结构里或汉字紧贴的令牌照样命中 | 给定（JSON）值、查询串参数、链接（URL）路径段、`.env` 行、代码围栏包裹的令牌，以及紧贴汉字的令牌，夹具字面为 `sk-…` 直连汉字，当检测，则都命中 `Token`（前缀判定看的是“前面不是 `ASCII` 字母数字”，不是词首） | 2026-09-26 |
| short_dummy_keys_are_detected | 手写的短假密钥照样命中 | 给定 `sk-123456`、`sk-abc123`、`sk-XXXXXXXX`、`sk_live_1234abcd`、`ghp_1234abcd` 这类十几字符内的假密钥、以及被换行截断的令牌，当检测，则都命中 `Token`（主体 不低于 4 且含数字或大写即算） | 2026-09-26 |
| invisible_characters_do_not_hide_a_token | 不可见字符藏不住令牌 | 给定紧跟前缀、或嵌在卡号与高熵串里的六类不可见字符（零宽空格、连接符、词连接符、`BOM`、软连字符），当检测，则 `Token`、`CardNumber`、`HighEntropy` 各自仍命中（判定前从副本里剥掉它们，且发生在所有检测器之前） | 2026-09-26 |
| prose_about_tokens_is_not_a_hit | 谈论令牌的散文不误报 | 给定本仓库文档里描述敏感信息守则的原话、过短的 `sk-abc`、以及“`AKIA` 是前缀”这类说明，当检测，则都不命中 | 2026-09-24 |
| hyphenated_words_are_not_mistaken_for_prefixed_tokens | 英文复合词不被误判成令牌 | 给定 `disk-space-2024`、`risk-managed-portfolio`、`mask-the-answer`、`sk-learn-scikit` 这类含 `sk-` 的普通复合词，当检测，则都不命中（前缀前面是字母即不算，普通小写主体也不算短档） | 2026-09-26 |
| private_key_blocks_are_detected_before_tokens | 私钥块先于令牌判定 | 给定（PEM）私钥块（含它同时含 `sk-` 形式串），当检测，则报 `PrivateKey`（更确定的类别胜出） | 2026-09-24 |
| card_numbers_pass_luhn_only | 卡号只有过 `Luhn` 才算 | 给定 4111 1111 1111 1111、差一位的变体、全零串、以及“卡号加有效期与（CVC）”与连续分隔符形态（`4111 1111 1111 1111 12/26`、`4111  1111  1111  1111`），当检测，则只有真正过 `Luhn` 且数字不重复的才命中 `CardNumber`（组分界处也判一次） | 2026-09-26 |
| high_entropy_strings_are_detected | 高熵长随机串识别 | 给定 32 位以上、含三类字符的高熵串，当检测，则命中 `HighEntropy` | 2026-09-24 |
| identifiers_and_prose_stay_below_the_entropy_threshold | 标识符与散文不触熵阈值 | 给定长下划线标识符、纯小写长词、中文句子，以及三类字符齐备但分布均匀的长串，当检测，则都不命中——最后那类只可能被熵阈值否掉（含一条熵 4.32 的近界样本），阈值被删或放宽到 4.0 即红 | 2026-09-26 |
| bearer_and_aws_key_ids_keep_their_own_bounds | `Bearer` 与 `AKIA` 各守自己的边界 | 给定带 `base64` 填充符的 `AKIA` 编号、过短或非大写的 `AKIA` 串、带连字符的伪编号、以及标准 `base64` 主体的 `Bearer` 令牌，当检测，则只有第一类命中 `Token`——`Bearer` 支仍用窄字符集，`AKIA` 只看连续的大写字母与数字 | 2026-09-26 |
| the_long_tier_of_prefixed_bodies_ignores_shape | 前缀令牌长档不看形状 | 给定 13 字符与 16 字符的纯小写复合词，当检测，则短档那次放行、长档那次算令牌——长档只卡长度是写明的取舍，这条用例把边界钉在明面上 | 2026-09-26 |
| detection_prefers_the_more_certain_kind | 检测按确定度排序 | 给定同时可判多类的文本，当检测，则返回更确定的那个类别（确定度自私钥、令牌、卡号至高熵递减；相邻两类各有一对同现的样例） | 2026-09-26 |
| ordinary_text_is_never_flagged | 日常文本一律不拦 | 给定常用句中英文本、中文段落、代码片段、带 `sk-` 的普通英文句与 `let key = "sk-test";` 这类代码，当检测，则全部为 `None` | 2026-09-26 |
| windows_executable_names_block_the_scene_gate | 系统名单按规范化键命中场景闸门 | 给定名单内可执行文件名（裸名与改写大小写）与名单外进程（资源管理器）的前台应用事实，当 `trigger_block`，则名单内两例按命中条目拦截、名单外放行（两平台同跑，系统（macOS）名单行为不变） | 2026-11-07 |
| windows_process_keys_match_regardless_of_exact_spelling | 系统名单按规范化键不依赖确切拼写 | 给定 `1Password7.exe` 与 `1Password 7.exe` 两种拼写及大小写变体、纯名称字段（无 `bundle` `id`）与名单外近似名（`1password-manager.exe`、`explorer7.exe`）的前台应用事实，当 `trigger_block`，则规范化后（小写、去 `.exe` 后缀、去空格）两种拼写都命中同一键、近似名放行 | 2026-11-07 |
| the_self_flag_is_not_consumed_by_the_sensitive_list | `is_self` 与敏感名单无关 | 给定 `is_self` 为真的名单外前台应用，当判闸门，则放行（`is_self` 只归触发决策的防误触分支，不是名单命中） | 2026-09-30 |

### `crates/gloss-core/src/domain/model.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| error_display_names_the_variant_without_the_payload | 错误文案只记变体名不含载荷 | 给定 7 个代表性 `GlossError` 变体（含平台未支持），当 `to_string`，则输出固定类别文案（`EngineResponse` 与 `Config` 不打印载荷） | 2026-11-05 |
| error_debug_output_never_carries_the_payload | 带载荷变体的调试输出不泄露载荷 | 给定携带敏感载荷的 `EngineResponse` 与 `Config`，当调试格式化，则输出只含变体名与载荷长度、不含载荷本身 | 2026-11-11 |
| error_display_never_carries_the_payload | 带载荷变体的 `Display` 不泄露载荷 | 给定携带敏感载荷的 `EngineResponse` 与 `Config`，当 `to_string`，则输出不含载荷的固定文案 | 2026-10-17 |
| error_is_std_error | 错误可作 `std` `Error` | 给定 `GlossError` 装箱为 `std` `Error`，当 `to_string`，则输出正确文案 | 2026-09-19 |

### `crates/gloss-core/src/domain/schema.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| violations_carry_fixed_wording_without_reply_content | 违约措辞固定且不带回复原文 | 给定四类 `Violation`，当取措辞，则非空、固定且不含选区、回复原文、密钥形态（类别进日志与降级产物，永不携带内容） | 2026-10-11 |
| classify_violation_matrix_covers_empty_not_json_and_kind | 分类违约矩阵 | 给定空回复、纯空白、散文、截断 JSON、围栏输出、JSON 标量、缺 `kind/kind` 为 `null`、未知 `kind`、类别外的 `ImageExplain` 与 `General/task` 契约体各类回复，当 `validate_classify`，则分别落 `EmptyReply/NotJson/KindNotAllowed` | 2026-10-20 |
| classify_accepts_the_template_categories_and_maps_fallback | 分类放行模板类别并映射 `Fallback` | 给定模板四个类别（含首尾空白），当 `validate_classify`，则返回对应 `kind` 且携带模型原始类别（`Fallback` 映射 `General`） | 2026-10-20 |
| task_violation_matrix_covers_empty_not_json_and_missing_note | 任务违约矩阵 | 给定五个 `kind` 与空回复、散文、JSON 数组、截断 JSON、缺 `note`、`note` 为 `null`、纯空白 `note`、`note` 非字符串各回复的全组合，当 `validate_task`，则分别落 `EmptyReply`、`NotJson`、`MissingNote`（硬约束只此三条） | 2026-10-12 |
| task_ok_builds_the_three_contract_variants | 任务契约三变体构建 | 给定词卡、代码、句译、图像与通用的嵌套契约（JSON），当 `validate_task`，则 `note` 原样、`structured` 按 `kind` 落 `WordCard/Code/Plain`（坏条目跳过） | 2026-10-12 |
| task_soft_rules_fall_back_field_by_field | 任务软约束逐级回落 | 给定 `structured` 缺失、非对象、变体内字段类型错、`null` 的回复，当 `validate_task`，则按空变体与缺省回落、`note` 保留、清单外疏证键就地忽略（`kind` 决定变体） | 2026-10-11 |
| empty_structured_covers_every_kind | 空疏证变体覆盖全 `kind` | 给定五个 `kind`，当 `empty_structured`，则词卡空 `WordCard`、代码空 `Code`（无语言判定）、句译、图像与通用落空 `Plain` | 2026-10-12 |
| format_detail_maps_fence_truncation_and_non_object_shapes | 格式细节矩阵：围栏、截断与非对象 | 给定围栏包裹、合法非对象（JSON）、中途截断与首字节语法错各原始回复，当 `format_detail`，则分别落 `Fenced`、`NotAnObject`、`Truncated` 与无细节（细节是纯结构事实，围栏内原文不随行） | 2026-10-29 |
| format_detail_maps_missing_note_shapes_to_key_or_string_details | 格式细节矩阵：注键缺失与非字符串 | 给定缺 `note` 键、`note` 为 `null`、非字符串与纯空白、不可解析的各回复，当 `format_detail`，则分别落 `NoteKeyMissing`、`NoteNotString` 与无细节 | 2026-10-29 |
| format_detail_returns_none_for_undetailed_violations_and_legal_input | 无细节违约与合法输入无细节 | 给定 `EmptyReply`、`KindNotAllowed` 与契约合法回复，当 `format_detail`，则恒返回无细节（细分只补格式违约的位置，不改硬约束数量与降级语义） | 2026-10-29 |
| classify_category_names_match_the_validator_exactly | 类别标识集与校验器同源 | 给定类别常量四标识，当逐个过 `validate_classify`，则全部放行且模型原始类别与常量一致（类别段渲染与校验共用同一集合） | 2026-10-29 |

### `crates/gloss-core/src/domain/task.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| task_input_carries_text_only | 输入只携带原文 | 给定文本输入，当构造 `TaskInput`，则只有 `text`——模态提示已废除（源语言、代码语言由 `LLM` 自行判断） | 2026-10-04 |
| task_binds_kind_input_and_options | `Task` 三元正确绑定 | 给定构造参数，当 `Task::new`，则 `kind/input/options` 正确绑定（构造口是唯一入口） | 2026-10-18 |
| task_options_default_carries_the_factory_model | 选项缺省携带出厂模型 | 给定 `TaskOptions::default()`，当检查，则 `model` 为 `DEFAULT_MODEL`、`target_lang/prompt_locale` 为 `None`（缺省只服务反序列化与测试直构） | 2026-10-18 |
| image_input_shares_png_bytes_via_arc | 图像输入 `Arc` 共享 | 给定位图（PNG）字节，当构造 `Image` 输入，则经 `Arc` 共享（`ptr_eq`） | 2026-09-19 |
| outcome_carries_word_card_subprov_and_the_code_verdict | 产物携带词卡疏证与语言判定 | 给定词卡与代码产物，当构造 `TaskOutcome`，则词卡 `structured` 为 `WordCard`（`phonetic/examples`）、代码的语言判定随 `Code` 变体走（`structured.code_language`，`UI` 角标与高亮使用） | 2026-10-11 |
| degraded_defaults_to_false_on_the_wire | 降级标记 `serde` 缺省 `false` | 给定序列化后剥去 `degraded` 字段的产物（JSON），当反序列化，则 `degraded` 为 `false`（旧条目落在正常路径）；`degraded=true` 显式往返 | 2026-10-11 |
| language_violations_default_to_an_empty_list_on_the_wire | 违约清单 `serde` 缺省空清单 | 给定携带两条语言违约的产物，当 `serde` 往返，则清单逐条无损；给定剥去 `lang_violations` 列的旧条目（JSON），当反序列化，则清单为空（旧缓存条目走界面泛化回落，不破缓存） | 2026-10-30 |
| modality_matrix_is_enforced_cell_by_cell | 模态矩阵逐格校验 | 给定五个 `kind` 与两种输入的全矩阵，当逐格 `Task::new`，则文本列（含通用 `General`）与 `Image` 列（唯一合法 `kind` 为 `ImageExplain`）逐格合规 | 2026-10-18 |
| task_deserialization_goes_through_the_modality_gate | `wire` 入口同过模态闸门 | 给定合法与非法（`ImageExplain` 配文本输入）两份 JSON 形态的 `Task`，当反序列化，则前者成功、后者被拒——反序列化不是绕过 `Task::new` 的旁路 | 2026-10-18 |
| task_round_trips_through_serde | `Task` `serde` 往返无损 | 给定文本任务（含目标语言与模型），当经 `Task::new` 构造并 `serde` 往返，则无损 | 2026-10-18 |
| image_input_round_trips_through_serde | 图像输入 `serde` 往返 | 给定 `Image` 输入，当 `serde` 往返，则字节不变且反序列化得到新 `Arc`（不与原值共享） | 2026-09-19 |

### `crates/gloss-core/src/log.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| init_is_idempotent | 初始化幂等 | 给定已初始化的日志，当再次 `init(None)`，则无副作用不 `panic` | 2026-09-19 |
| file_writer_creates_missing_directory | 日志目录自动创建 | 给定不存在的目录，当建 `file` `writer`，则目录被创建且 `active` 路径等于它 | 2026-09-19 |
| file_writer_persists_lines_into_daily_file | 日志按日文件落盘 | 给定写出的一行日志，当 `drop` `guard` 刷盘后检查，则落入按日命名的文件且内容含该行 | 2026-09-19 |
| file_writer_degrades_when_directory_is_unusable | 目录不可用时优雅降级 | 给定路径被普通文件占用，当建 `writer`，则返回 `None` | 2026-09-19 |
| filter_falls_back_to_default_when_unset_or_empty | 过滤器缺省回退 | 给定 `None` 或空 `RUST_LOG`，当 `build_filter`，则得 `info` | 2026-09-19 |
| filter_keeps_default_level_alongside_module_directives | 模块指令与默认级别并存 | 给定模块指令，当解析，则模块级与默认 `info` 并存 | 2026-09-19 |
| filter_lets_global_directives_override_default | 全局指令覆盖默认级别 | 给定 `off/warn/error` 全局指令，当解析，则覆盖默认 `info` | 2026-09-19 |
| filter_drops_invalid_directives_but_keeps_valid_ones | 非法指令丢弃、合法保留 | 给定含非法指令的串，当解析，则非法项丢弃、合法项保留 | 2026-09-19 |
| json_lines_carry_the_structured_contract | 日志行是结构化（JSON） | 给定带 `thread/kind` 字段的一条日志，当解析该行，则 `level/message/thread/kind/target` 均在顶层且行内无 `ANSI` 转义 | 2026-09-23 |
| logs_outside_a_task_carry_no_generation | 流程外的日志不带代数 | 给定任务 `span` 之外记的一条日志，当解析该行，则既无 `generation` 也无 `span` 对象 | 2026-09-24 |
| civil_date_matches_known_anchors | 天数换算公历 | 给定 1970-01-01、2000-01-01、2026-09-24、2026-12-31 对应的天数，当换算，则年月日与已知值一致 | 2026-09-24 |
| log_file_name_is_dated_jsonl | 日志文件名带日期与后缀 | 给定日期 2026-09-24，当取名，则得 `gloss-2026-09-24.jsonl` | 2026-09-24 |
| daily_writer_appends_into_todays_file | 按天文件追加写入 | 给定两个写入器实例写同一份今日日志，当读回，则两行都在今天的文件里 | 2026-09-24 |
| milestone_event_carries_id_and_elapsed_ms | 里程碑事件带 `id` 与累计毫秒 | 给定 `milestone("m_test_probe")` 一条记录，当捕获并解析该行，则 `message/thread/milestone/elapsed_ms` 均在顶层且 `elapsed_ms` 为非负整数 | 2026-10-05 |
| prune_keeps_the_newest_files_only | 旧日志按天数保留 | 给定 9 份日志与一个无关文件，当清理，则只留最近 7 份日志且无关文件不动 | 2026-09-24 |
| task_span_carries_generation_into_events | 任务 `span` 把代数带给范围内的日志 | 给定 `task_span(7)` 并在其中记一条日志，当格式化输出，则事件行是 JSON、顶层 `generation=7` 且无 `span` 对象 | 2026-09-24 |

### `crates/gloss-core/src/pipeline/classify.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| non_text_input_is_rejected | 非文本输入拒绝分类 | 给定 `Image` 输入，当 `classify`，则 `UnsupportedModality` | 2026-09-26 |
| bare_json_reply_selects_the_kind | 裸 JSON 回复解析 `kind` | 给定 `{"kind":"TranslateWord"}` 回复，当 `classify`，则得 `TranslateWord` | 2026-09-26 |
| a_fenced_reply_repairs_then_falls_back | 围栏回复按违约进修复轮再兜底 | 给定模型不守约输出的 `json` 围栏回复（严格裸 `JSON` 契约，无围栏容错），当 `classify`，则按 `NotJson` 违约进修复轮（`1+CLASSIFY_REPAIRS` 次调用）、仍违约落兜底 `General` | 2026-10-12 |
| a_complete_json_settles_before_the_stream_ends | 完整（JSON）先于流结束定型 | 给定“半截 → 闭合 → 流内错误”的增量序列，当 `classify`，则返回首个完整（JSON）的判定（等流结束就会拿到那个错误） | 2026-09-30 |
| the_first_complete_json_wins_over_later_deltas | 首个完整（JSON）胜出 | 给定两段各自完整的 JSON 增量，当 `classify`，则返回前者（拼接后两端都解析不过，只有提前退出才拿得到） | 2026-09-30 |
| partial_json_keeps_waiting_for_the_stream | 半截（JSON）继续等流 | 给定到流结束都补不齐的半截（JSON），当 `classify`，则流内不提前定型（增量校验不过继续等），流走完按违约进修复轮、耗尽落兜底 `General` | 2026-10-12 |
| a_request_failure_is_digested_into_the_fallback_without_retry | 分类请求失败内部消化落兜底 | 给定分类请求整体失败，当 `classify`，则不重试、内部消化后返回 `Ok(General)`（引擎恰 1 次调用） | 2026-10-12 |
| a_violated_reply_repairs_into_a_kind | 分类违约回复修复后判型 | 给定“散文回复 → 裸 JSON `kind`”脚本队列，当 `classify`，则修复轮对话为 `[system][user][assistant][user]`（原文、坏回复、纠错指令四轮），纠错指令把违约固定措辞作为 `reason` 回灌（无契约行）、不含坏回复与选区原文，修复后返回判定的 `kind` | 2026-10-20 |
| classify_traced_carries_rounds_and_repairs_for_the_trajectory | 分类全轨迹携带轮次与修复数 | 给定“散文回复 → 裸 JSON `kind`”与“恒散文”两个脚本队列，当 `classify_traced`，则前者 `decision.kind` 为判定的 `kind`、`repairs` 为 1、`rounds` 按轮序携带每轮完整回复；后者耗尽落兜底 `General`、`repairs` 为 `CLASSIFY_REPAIRS`、`rounds` 为 `1+CLASSIFY_REPAIRS` 轮 | 2026-10-12 |
| an_empty_reply_repairs_by_resending_as_is | 分类空回复原样重发修复 | 给定“空回复 → 裸 JSON `kind`”脚本队列，当 `classify`，则修复轮按 `EmptyReply` 原样重发（消息列表与首轮逐字一致，无 `assistant` 轮无纠错指令）、修复后判出 `kind` | 2026-10-12 |
| an_empty_completion_enters_the_repair_round_and_resends | 分类零增量空完成进修复轮重发 | 给定“`EmptyCompletion` 错误 → 裸 JSON `kind`”脚本队列，当 `classify`，则空完成按 `EmptyReply` 违约进修复轮、原样重发（消息与首轮逐字一致）、修复后判出 `kind`（恰 2 次调用，与任务段同口径） | 2026-10-21 |
| a_repeated_empty_completion_exhausts_into_the_fallback | 分类空完成重演耗尽落兜底 | 给定连续两轮 `EmptyCompletion` 脚本，当 `classify`，则修复预算（`CLASSIFY_REPAIRS`）耗尽、恰 1 加预算次调用后落兜底 `General` | 2026-10-21 |
| exhausted_repairs_fall_back_to_general_with_a_trace | 分类修复耗尽落兜底且有痕 | 给定恒违约的回复脚本，当 `classify`，则恰用尽 `1+CLASSIFY_REPAIRS` 次调用后返回 `Ok(General)`，耗尽 `warn` 只记违约类别（不含回复与选区原文） | 2026-10-12 |
| a_silent_stream_is_cut_at_the_first_item_deadline | 分类流首条目截止快速失败 | 给定建连后恒不出字的流，当 `classify`（暂停时钟），则首条目截止到点按请求异常收卷、内部消化落兜底 `General`（恰 1 次调用，不重试） | 2026-10-16 |
| items_after_the_first_flow_without_the_deadline | 截止只武装到首条目 | 给定首条目 2 秒（截止内）、次条目再等 1000 秒的流，当 `classify`（暂停时钟），则两段增量照常累积定型 `TranslateWord`（截止在首条目交付后退役） | 2026-10-16 |
| replies_outside_the_template_categories_are_rejected | 回复越类别清单、不可识别一律拒绝 | 给定模板类别外的 `ImageExplain` 与 `General`、未知标识、`null`、纯散文、空串与 `gloss` 围栏，当解析，则全部 `EngineResponse`（严格裸 `JSON`：围栏容错已删，围栏内合法 `kind` 同样拒绝） | 2026-10-20 |
| a_fallback_choice_lands_on_the_general_kind | 模板兜底类别映射 `General` | 给定 `{"kind":"Fallback"}` 回复，当解析，则得 `General`（`classify` 模板的兜底类别执行通用任务） | 2026-10-20 |
| the_fallback_kind_is_the_general_template_landing | 兜底常量恒为 `General` | 给定 `CLASSIFY_FALLBACK`，当检查，则恒为 `General`（可渲染可执行的执行落点） | 2026-10-20 |
| a_fallback_reply_lands_on_the_general_task_with_both_kinds_logged | 模型分类与最终分类同行落日志 | 给定 `{"kind":"Fallback"}` 回复，当 `classify`，则落 `General`，且 `info` 日志同行携带模型类别 `model_kind=Fallback` 与最终 `kind=General`、不含选区内容 | 2026-10-20 |

### `crates/gloss-core/src/pipeline/engine.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| engine_failures_propagate_and_earlier_chunks_are_kept | 引擎失败原样上抛 | 给定 `execute` 整体失败与流中 `Err`，当 `run`，则任务段错误原样上抛（分类段失败已由 `classify` 内部消化）且失败前的增量已转发 | 2026-10-12 |
| classified_kind_arrives_before_any_chunk | 分类先于任何增量 | 给定文本输入与两段任务流，当 `run`，则 `on_classified` 恰在首个 `on_chunk` 之前触发一次（含分类修复耗尽落的兜底 `General`） | 2026-10-12 |
| classify_request_failure_falls_back_without_retrying | 分类请求失败内部消化后任务照跑（`engine`） | 给定分类调用失败（一次性错误）与契约任务脚本，当 `service.run`，则分类不重试、内部消化落 `CLASSIFY_FALLBACK(General)`、引擎共 2 次调用（分类加任务）、`fallback` `warn` 不含选区原文 | 2026-10-12 |
| raw_text_is_returned_verbatim | 原始文本原样返回 | 给定两段（JSON）流式脚本，当 `run`，则 `RunOutput.raw` 为未解析的原始拼接文本 | 2026-10-04 |
| run_task_rounds_executes_a_fixed_kind_stream_without_classification | 任务段循环以给定 `kind` 直达执行 | 给定“违约散文 → 契约（JSON）”脚本队列与预渲染的任务底座，当直接调 `run_task_rounds`，则 `kind` 恒为调用方给定值（不分类）、`repairs` 为 1、`raw` 为末轮原文、引擎恰 2 次调用（无分类调用在前） | 2026-10-12 |
| run_output_carries_the_server_finish_reason | `RunOutput` 携带服务端结束原因 | 给定声明 `stop` 与 `length` 结束的两个脚本流，当 `run`，则 `finish_reason` 原文随 `RunOutput` 返回、`raw` 不受影响、契约合回复 `repairs` 为 0（`length` 只在违约修复时进指令，`content_filter` 直接失败卡） | 2026-10-12 |
| run_output_finish_reason_stays_none_without_a_done_marker | 无 `[DONE]` 时结束原因缺省 | 给定只吐增量、不声明结束的脚本流，当 `run`，则 `finish_reason` 为 `None` | 2026-10-11 |
| blank_model_is_a_config_failure_before_the_engine | 空白模型先于引擎拒绝 | 给定 `model` 为空白的选项，当 `run`，则 `Config` 错误且引擎 0 调用 | 2026-10-03 |
| image_input_skips_classification_and_lands_on_image_explain | 图像输入跳分类固定 `ImageExplain` | 给定 `Image` 输入与两段任务流，当 `run`，则 `kind` 恒为 `ImageExplain`、`on_classified` 先于任何 `chunk` 恰一次、引擎共 1 次调用（省去分类往返）、`raw` 原样返回 | 2026-10-10 |
| a_contract_violation_repairs_with_the_bad_reply_in_the_history | 任务违约修复轮带回坏回复 | 给定图像任务“违约散文 → 契约（JSON）”脚本队列，当 `run`，则修复轮对话为 `[system][user][assistant][user]`（图、坏回复、纠错指令四轮），纠错指令把违约固定措辞作为 `reason` 回灌、不带契约示例行、不含坏回复原文，图像部件随对话重发，`on_repair` 恰触发一次、`RunOutput.repairs` 为 1 | 2026-10-12 |
| the_repair_round_warn_carries_the_category_and_round_only | 修复轮违约告警只记类别与轮次 | 给定违约回复触发修复，当捕获日志，则“`task reply violated the contract`”行含固定违约类别与 `attempt=1`、不含回复原文 | 2026-10-12 |
| an_empty_reply_repairs_by_resending_without_an_assistant_turn | 任务空回复原样重发修复 | 给定图像任务“空流 → 契约（JSON）”脚本队列，当 `run`，则修复轮请求与首轮消息逐字一致（无 `assistant` 轮无纠错指令）、`repairs` 为 1 | 2026-10-12 |
| an_oversized_stream_fails_instead_of_accumulating_without_bound | 失控长流按预算失败不无界累积 | 给定 300 段、每段 4 千字节（KB）而总量超 1 兆字节（MB）预算的增量流，当 `run`，则 `EngineResponse` 失败上抛（不进修复轮）、已转发字节不超预算 | 2026-10-16 |
| a_zero_increment_empty_completion_enters_the_repair_path | 零增量完成转 `EmptyReply` 进修复 | 给定“`EmptyCompletion` 变体 → 契约（JSON）”脚本队列，当 `run`，则零增量完成不按失败上抛、转 `EmptyReply` 进修复轮（引擎 2 次调用、`repairs` 为 1） | 2026-10-17 |
| task_repairs_are_budgeted_then_the_raw_reply_is_handed_over | 任务修复预算耗尽上交原始文本 | 给定恒违约的任务回复，当 `run`，则恰用尽 `1+TASK_REPAIRS` 次调用、`on_repair` 触发 `TASK_REPAIRS` 次、原样返回最后一轮违约原文（完成态解析落降级）且不报 `Err` | 2026-10-12 |
| task_stage_engine_errors_are_never_retried | 任务段引擎错误不重试 | 给定 `execute` 整体失败与零增量的流中协议错误，当 `run`，则两者都原样上抛 `Err`（只有 `EmptyCompletion` 变体转 `EmptyReply`）、引擎恰 1 次调用 | 2026-10-17 |
| a_content_filter_finish_fails_without_a_retry | 内容审查结束直接失败卡 | 给定 `finish_reason=content_filter` 的流，当 `run`，则 `EngineResponse`（固定审查诊断） 上抛（不校验正文、不进修复轮）、引擎恰 1 次调用 | 2026-10-12 |
| a_length_finish_repairs_without_a_truncation_hint | `length` 结束按普通违约修复 | 给定“`length` 截断的违约流 → 契约流”脚本队列，当 `run`，则修复指令不含截断提示（修复模板已无该维度）、修复后以新结束原因收口、`repairs` 为 1 | 2026-10-20 |
| a_fenced_reply_repairs_like_any_other_violation | 围栏任务回复同权违约进修复 | 给定 `json` 围栏的任务回复与契约修复流脚本队列，当 `run`，则围栏按 `NotJson` 违约进修复轮（严格裸 `JSON` 契约）、`repairs` 为 1 | 2026-10-12 |
| cancellation_cuts_through_the_repair_round | 取消打断修复轮 | 给定慢流脚本队列（首轮违约、修复轮慢流），当修复轮回调到达即从 `select!` 竞速取消，则取消分支获胜（修复轮是 `run` `future` 内 `await` 点，竞速天然覆盖） | 2026-10-12 |
| language_violations_repair_with_a_row_list_instruction | 语言违约进行式清单修复轮 | 给定语言违约但契约合格的回复与契约修复流脚本队列，当 `run`，则任务段零修复、语言段 1 轮、指令行含每条规则标识与字段位置、不含回复原文、对话形态 `[system][user][assistant][user]`、`lang_degraded` 为假 | 2026-10-28 |
| language_repairs_exhaust_and_deliver_the_reply_degraded | 语言修复预算耗尽带降级交付 | 给定连续两条契约合格但语言违约的回复，当 `run`，则恰 1 轮语言修复（`LANG_REPAIRS`）后 `lang_degraded` 为真、`raw` 为末条契约合格回复、已判得的违约清单（含 `ForeignRun`）随 `RunOutput` 交付、不报 `Err` | 2026-10-30 |
| contract_invalid_direct_degrade_delivers_an_empty_violation_list | 契约无效直降带空清单 | 给定语言段入口处契约已失效的回复（直调 `run_lang_rounds`），当降级交付，则 `lang_degraded` 为真而违约清单为空（契约无效直降路径与预算耗尽路径分开赋值，原文原样上交） | 2026-10-30 |
| a_contract_broken_language_repair_keeps_the_last_valid_reply | 修复轮契约退化保末份合格回复 | 给定契约合格的语言违约回复与修复轮散文（契约坏回复），当 `run`，则坏回复不被采纳、`raw` 保持末份契约合格回复且 `lang_degraded` 为真 | 2026-10-28 |

### `crates/gloss-core/src/pipeline/engine/llm.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| resolves_endpoint_from_config | 端点从配置解析 | 给定出厂配置，当解析端点，则得 `{DEFAULT_BASE_URL}/chat/completions` | 2026-09-19 |
| endpoint_trimming_avoids_double_slash | 端点尾斜杠去重 | 给定尾斜杠 `base_url`，当解析，则无双斜杠 | 2026-09-19 |
| cleartext_endpoints_are_rejected | 明文端点拒绝 | 给定 `http://` 明文端点（含 `localhost/127.0.0.1`），当解析，则报 `Config` 错 | 2026-09-19 |
| endpoints_with_credentials_query_or_fragment_are_rejected | 带凭据、查询、片段的端点拒绝 | 给定带 `userinfo/query/fragment` 的端点，当解析，则拒绝 | 2026-09-19 |
| unusable_endpoints_report_config_error | 不可用端点报配置错误 | 给定空串、空白、`file:///`无 `scheme` 地址，当解析，则报 `Config` 错 | 2026-09-19 |
| resolves_key_from_keychain | 密钥从 `keychain` 解析 | 给定出厂配置加内存密钥，当解析，则取出密钥原文 | 2026-09-19 |
| missing_key_reports_auth_error | 缺密钥报鉴权错误 | 给定未配置密钥，当解析，则 `EngineAuth` | 2026-09-19 |
| blank_key_counts_as_missing | 空白密钥视为缺失 | 给定空白密钥，当解析，则按未配置报 `EngineAuth` | 2026-09-19 |
| empty_provider_list_falls_back_to_the_factory_entry | 空 `provider` 回退出厂条目 | 给定显式空 `provider_keys` 与出厂 `keychain` 条目密钥，当解析，则回退出厂条目 `gloss/deepseek` | 2026-09-19 |
| adapter_streams_deltas_across_chunk_boundaries | 流式事件（SSE）适配器跨块流式 | 给定同一段（SSE）响应按 1、2、5、整包字节分块，当经 `SseStream` 适配，则增量恒为“光泽”、`[DONE]` 终止、余字节不解析 | 2026-09-19 |
| adapter_delivers_the_done_marker_with_its_finish_reason | 适配器交付带结束原因的终结条目 | 给定增量、声明 `stop` 的结束块与 `[DONE]` 的响应，当经 `SseStream` 消费，则先交增量、再交携带 `stop` 的 `TaskChunk::Done` 终结条目 | 2026-10-11 |
| adapter_delivers_deltas_then_terminates_on_protocol_error | 协议错误先交增量再终结 | 给定增量后跟坏（JSON），当流推进，则先交付增量、随后以 `EngineResponse` 错误终结 | 2026-09-19 |
| an_error_chunk_is_final_even_when_bytes_keep_arriving | 流语义契约·错误条目即终结 | 给定增量后跟坏（JSON）行再跟合法数据行的字节流，当经适配器消费，则恰交增量与错误各一，此后再无产出（错误条目之后的字节不解析） | 2026-10-30 |
| adapter_ends_cleanly_without_the_done_marker | 无 `[DONE]` 关流干净收尾 | 给定无 `[DONE]` 直接关流的响应，当解析，则带增量流按正常结束收尾 | 2026-09-19 |
| adapter_reports_empty_completion_as_failure | 零增量完成报失败 | 给定零增量响应（仅 `[DONE]`、`HTML` 劫持页、或声明 `stop` 却零增量），当解析，则报 `EngineResponse` 错且流即止 | 2026-10-11 |
| request_body_has_the_openai_envelope | 请求体 `OpenAI` 信封 | 给定未设限的 `EngineRequest`，当构造请求体，则 `model/messages/stream/response_format` 四键 `wire` 形态精确匹配且无 `max_tokens` 键 | 2026-10-11 |
| json_mode_switch_is_always_on_the_wire | JSON 契约恒在请求体上 | 给定设限、不设限与多模态（图像部件）请求，当构造请求体，则 `response_format` 恒为 `{"type":"json_object"}`（`EngineRequest` 无开关，不做能力协商） | 2026-10-11 |
| text_content_stays_a_json_string_on_the_wire | 纯文本正文 `wire` 上仍是字符串 | 给定纯文本用户消息，当构造请求体，则 `content` 是 JSON 字符串、不升级成 `content` 数组 | 2026-10-10 |
| multimodal_parts_map_to_the_openai_content_array | 多模态部件映成 `OpenAI` `content` 数组 | 给定系统文本消息加仅图像部件的用户消息，夹具为位图（PNG），当构造请求体，则图像部件映 `{"type":"image_url"}` 的 `data:image/png;base64` 数据（URL）且整包形状精确匹配（含 `response_format`） | 2026-10-11 |
| max_tokens_is_carried_only_when_set | `max_tokens` 仅设限时上线 | 给定 `max_tokens=None` 与 `Some(64)` 两个请求，当构造请求体，则 `None` 无该键、`Some` 携带数值 64 | 2026-09-26 |
| maps_http_status_to_error_variants | 超文本传输（HTTP）状态映射错误变体 | 给定 401、403、429、500、`400+JSON`、`400+HTML`，当映射，则分别得 `EngineAuth`、`EngineRateLimited`、`EngineNetwork`、带诊断的 `EngineResponse` | 2026-09-19 |
| json_mode_and_multimodal_rejections_are_recognized_from_diagnostics | 诊断文本识别（JSON）模式与多模态不支持 | 给定 `4xx` 且诊断含 `response_format/json_object` 或 `image/multimodal/vision` 的错误体，当映射，则分别得 `JsonModeUnsupported` 与 `UnsupportedModality`（`JSON` 模式检查在前）；`5xx` 与 401 不做启发式（网络与鉴权优先） | 2026-10-11 |
| retry_delay_covers_only_transient_failures_within_budget | 重试判定只认预算内的瞬态失败 | 给定网络、限流、鉴权、协议、配置五类错误与不同预算余额，当 `retry_delay`，则网络与限流在预算内得固定退避（`Retry-After` 指引封顶生效、低于上限时指引优先），预算耗尽与瞬时之外类别一律 `None` | 2026-10-15 |
| retry_after_header_parsing_accepts_seconds_only | `Retry-After` 只解析秒数形态 | 给定缺失、整数、带空白、超文本传输（HTTP）日期、非数字五种头，当 `parse_retry_after`，则仅整数（含首尾空白）得秒数时长，其余 `None` 回落固定退避 | 2026-10-15 |
| connect_retries_transient_failures_within_the_budget | 连接阶段瞬态失败退避重试并交还余额 | 给定“网络错误后成功”“两次瞬态失败”“鉴权失败”三条尝试脚本，当 `connect_with_retry`，则首条两次尝试后成功、余额清零；次条交还末次错误 `RateLimited`、恰 2 次调用；末条不重试原样上抛 `Auth`、余额不减 | 2026-10-15 |
| first_byte_failures_retry_within_the_shared_budget | 首条目之前的瞬态失败对消费方无感重试 | 给定“立即网络错误的空流 → 增量加`Done` 流”尝试脚本与同形首轮空流，当消费包装流，则失败条目不上交、增量与 `Done` 原序到达、恰 1 次重连调用 | 2026-10-15 |
| first_byte_budget_exhaustion_passes_the_error_through | 重试预算耗尽错误原样交付 | 给定首轮与重试流都以网络错误开场，当消费包装流，则末次错误原样上交后流即止、重连恰 1 次 | 2026-10-15 |
| non_retryable_first_byte_errors_pass_through_without_a_retry | 非瞬态失败不重试直接交付 | 给定首条目即 `EngineAuth` 的流，当消费包装流，则错误原样上交、零重连调用 | 2026-10-15 |
| errors_after_the_first_item_are_never_retried | 首条目之后错误一律不再重试 | 给定“增量后网络错误”的流，当消费包装流，则增量先到、错误原样上交、零重连调用（半截产物不重头重播） | 2026-10-15 |
| first_byte_retry_honors_a_zero_leftover_budget | 首条目重试尊重连接阶段耗尽的余额 | 给定余额为 0 与网络错误开场的首轮流，当消费包装流，则错误原样上交、零重连调用（两阶段共用同一份预算） | 2026-10-15 |
| an_idle_stream_fails_after_the_silence_budget | 静默超时终结悬死流 | 给定交付一个增量后恒 `Pending` 的字节流（虚拟时钟），当静默超过空闲预算后消费，则流以 `EngineNetwork` 终结且不再产出 | 2026-10-15 |
| arriving_bytes_postpone_the_idle_deadline | 字节到达即复位空闲看门狗 | 给定每 30 秒（虚拟时钟）到达一个增量的字节流，当连续消费三个（虚拟 90 秒超出 60 秒预算），则每个增量如期到达、看门狗不触发 | 2026-10-15 |

### `crates/gloss-core/src/pipeline/engine/sse.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| decodes_a_complete_stream | 完整流解码 | 给定完整 `OpenAI` 兼容流，当一次喂完，则两段增量加`Done`（`role` 块不产出） | 2026-09-19 |
| done_carries_the_server_finish_reason | `Done` 携带服务端结束原因 | 给定声明 `stop`、声明 `length` 与从未声明结束的三种流，当解码，则 `[DONE]` 条目携带最后一个内容块声明的 `finish_reason`（从未声明为 `None`，交出后清空） | 2026-10-11 |
| decodes_identically_for_every_chunk_split | 任意切分解码一致 | 给定同一段流按 `1..n` 每种字节切法，当逐块喂入，则产出完全一致 | 2026-09-19 |
| byte_by_byte_input_keeps_multibyte_characters_intact | 逐字节输入多字节字符完好 | 给定逐字节喂入，当解析，则多字节字符“光泽”完好 | 2026-09-19 |
| ignores_crlf_heartbeats_and_other_fields | 忽略 `CRLF`、心跳、其它字段 | 给定 `CRLF`、心跳注释、其它（SSE）字段，当解析，则不产出、`data` 行照常出增量 | 2026-09-19 |
| maps_server_errors_by_code | 服务端错误按码分类 | 给定流中错误对象（`rate_limit/invalid_api_key`、其它），当解析，则分类为 `RateLimited/Auth`、带诊断 `EngineResponse` | 2026-09-19 |
| unparsable_payload_reports_bounded_diagnostic | 解析失败诊断有上限 | 给定解析不出的载荷与超长错误消息，当解析，则报 `EngineResponse` 且诊断文本有上限 | 2026-09-19 |
| ignores_a_data_line_without_payload | 空载荷 `data` 行忽略 | 给定空载荷 `data:` 行，当解析，则按心跳忽略 | 2026-09-19 |
| ignores_bytes_after_done | `[DONE]` 后字节忽略 | 给定 `[DONE]` 后的字节，当解析，则一律忽略 | 2026-09-19 |
| done_is_the_last_item_even_with_trailing_lines | 流语义契约·完成条目为末项 | 给定完成哨兵后仍带数据行的完整流，当解码，则条目止于完成条目，其后喂入与收尾都无产出 | 2026-10-30 |
| a_terminal_failure_leaves_no_items_after_it | 流语义契约·失败条目终结流 | 给定超长行洪泛，当解码出失败条目，则其后喂入合法数据行与收尾都无产出 | 2026-10-30 |
| a_failed_chunk_terminates_the_decoder_too | 失败数据块同终结解码器 | 给定不可解析数据块或服务端错误对象，当解码出失败条目，则其后喂入数据行、完成哨兵与收尾都无产出（解码器与端口流语义同构） | 2026-10-30 |
| finish_drops_a_truncated_tail | 残缺尾行丢弃 | 给定残缺半行，当 `finish`，则丢弃且此后字节不再解析 | 2026-09-19 |
| finish_keeps_a_complete_unterminated_line | 完整无换行末行保留 | 给定完整但无换行的最后一行，当 `finish`，则作为增量交出 | 2026-09-19 |
| rejects_a_line_beyond_the_limit | 超长行拒绝终结流 | 给定超 `MAX_LINE_BYTES` 的无换行洪泛，当解析，则协议错误终结流、后续不再解析 | 2026-09-19 |
| error_without_code_reports_the_message_only | 无码错误只报消息 | 给定只有 `message` 的错误对象，当解析，则文案不带前导冒号 | 2026-09-19 |
| error_classification_checks_code_and_type | 错误分类查码与类型 | 给定 `code` 通用而 `type` 细分类（或反之），当解析，则按细分类识别 | 2026-09-19 |

### `crates/gloss-core/src/pipeline/lang_gate.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| foreign_runs_flag_bare_latin_and_kana_but_parenthesized_terms_pass | 外文段抓裸用放括注 | 给定中文目标与裸用外文词、`API`、`iPhone`、日语残段的注，当判定，则逐条 `ForeignRun`；括号内的外文术语（容器内容）不判 | 2026-10-28 |
| registered_bare_terms_pass_bare_and_flag_missing_space_without_spaces | 登记词裸用放行缺空格走盘古 | 给定登记词 `JSON` 带盘古空格与汉字直连两种注，当判定，则前者放行、后者只判 `MissingSpace` 不出 `ForeignRun`（登记面整词精确匹配放行，两侧同源，盘古空格随行） | 2026-11-02 |
| registry_match_is_case_sensitive_and_whole_word | 登记匹配大小写敏感整词 | 给定小写 `json`、带后缀 `JSON5` 与多字母 `JSONS` 三种注，当判定，则逐条 `ForeignRun`（表外零豁免，拼错与带后缀的词形不落登记词） | 2026-11-02 |
| unregistered_terms_keep_the_foreign_run_flag | 表外词维持外文判罚 | 给定未登记字母段的注，当判定，则 `ForeignRun`（逐词登记，不设全大写类形态放行规则） | 2026-11-02 |
| han_ranges_admit_extension_and_compat_forms_in_chinese | 汉字区段三段口径收扩展与兼容形 | 给定中文目标含扩展甲区与兼容区汉字的注，当判定，则无 `ForeignRun` 与 `ContainerMixing`（区段住数据源三段，两侧同判，`R1`） | 2026-10-30 |
| single_latin_letter_passes_as_variable_and_n_dot_flags | 单字母变量豁免缩写判罚 | 给定“变量 `x` 与 `y`”式单字母，当判定，则放行；词性缩写 `n.`（单字母紧跟句点）判 `ForeignRun` | 2026-10-28 |
| punctuation_set_catches_wrong_script_and_corner_brackets | 标点集抓串味与角引号 | 给定中文正文里的半角逗号与直角引号对，当判定，则逐条 `PunctuationMismatch`（直角引号中文场景全禁，`D3`）；英文正文合法标点全放行 | 2026-10-30 |
| slash_separator_passes_in_chinese_prose | 分隔号入中文集 | 给定中文正文含分隔号的注，当判定，则无 `PunctuationMismatch`（`D1`） | 2026-10-30 |
| math_symbols_pass_in_prose_without_delimiters | 数学记号直排放行 | 给定中文与英文正文含裸数学记号的注，当判定，则无 `PunctuationMismatch`（中性面并入允许表，`R4`） | 2026-10-30 |
| attached_measures_pass_after_digits_only | 计量族随数字放行 | 给定前邻数字的计量符号注与孤立计量符号注，当判定，则前者放行、后者 `PunctuationMismatch`（计量族归附随面，`R3`） | 2026-10-30 |
| attached_faces_follow_the_target_scene_and_two_dot_ellipsis_flags_both | 附随面随目标场景取面 | 给定英文区的摄氏符与全角人民币符随数字注，当判定，则 `PunctuationMismatch`；中文区同两符与万分率注放行；两点省略号两场景都判 `PunctuationMismatch`（分场拆分，`R3`） | 2026-10-31 |
| dash_and_ellipsis_families_stay_style_items_not_violations | 破折号省略号与数字附随不判 | 给定破折号、省略号、双连字符、小数、区间、百分与货币金额各形态的注，当判定，则全部不判标点（风格项留给评测） | 2026-10-28 |
| traditional_chars_flag_outside_and_pass_inside_quotes | 繁体正文判罚引文豁免 | 给定正文繁体字形与引文容器内的古籍繁体，当判定，则前者 `TraditionalChar`、后者放行 | 2026-10-28 |
| pangu_space_flags_adjacency_only_for_chinese_target | 盘古之白仅中文目标判定 | 给定中文目标“中文`word`”直连与标点隔开、英文目标直连三种注，当判定，则字母段直连转由 `ForeignRun` 判罚（管辖收窄后不出 `MissingSpace`）、标点隔开与英文目标放行 | 2026-11-01 |
| pangu_flags_digit_and_single_letter_adjacency_only | 盘古之白出罚面收窄到豁免对象 | 给定数字直连、单字母直连、多字母段直连与 `n.` 缩写直连的注，当判定，则前两者判 `MissingSpace` 且不出 `ForeignRun`，后两者只判 `ForeignRun`（补空格改法对豁免对象自收敛） | 2026-11-01 |
| markdown_forbidden_keeps_accumulating_other_categories | 结构节点判罚后其余类别照常累加 | 给定夹外文段与夹残缺括号的禁 `markdown` 注，当判定，则 `MarkdownForbidden` 与剥除记号后文本上的 `ForeignRun`、`UnclosedContainer` 同轮上报（早退屏蔽拆除） | 2026-11-01 |
| emoji_and_variation_selectors_flag_in_body_only | 表情符号与变体选择符判罚 | 给定正文含表情符号与变体选择符的注，当判定，则 `EmojiForbidden` | 2026-10-28 |
| markdown_structure_flags_on_forbidden_kinds_and_passes_on_full_kinds | 结构节点按任务能力判定 | 给定禁 `markdown` 三任务（词卡、句译、图像）的着重号、行内码、链接注，当判定，则 `MarkdownForbidden`；代码与通用任务全量放行 | 2026-10-28 |
| unclosed_containers_flag_stray_parens_quotes_fences_and_backticks | 残缺容器判罚 | 给定未闭合括号、未闭合弯引号、未闭合围栏与落单反引号，当判定，则 `UnclosedContainer`；成对围栏与行内码放行 | 2026-10-28 |
| isolated_dollar_and_currency_amounts_stay_legal | 落单美元符号双义放行 | 给定孤立 `$` 与 `$5` 的注，当判定，则既不判标点也不判容器（货币符号双义，宁漏勿误） | 2026-10-28 |
| math_containers_exempt_their_content | 数学容器内容豁免 | 给定成对 `$...$` 内的外文与记号、落单 `$`，当判定，则容器内容不判语言标点、落单不判残缺 | 2026-10-28 |
| container_mixing_flags_mixed_groups_only | 组内混语判罚 | 给定括号组内同时出现目标文字段与非目标文字段，当判定，则 `ContainerMixing` | 2026-10-28 |
| container_mixing_passes_single_language_and_excluded_tokens | 组内单一语言放行 | 给定“引文（译）”串行、嵌套内层括号、数字与单字母括注、代码任务无引文容器四种形态，当判定，则全部放行 | 2026-10-28 |
| sentence_parens_flag_final_punctuation_only | 句译括注末位句末标点判罚 | 给定句译任务括号组末字符为句末标点与组中部缩略语句点，当判定，则前者 `SentenceInParens`、后者放行；短语级括注放行 | 2026-10-28 |
| source_overlap_flags_copied_passages_and_folds_whitespace | 原文重合抓整段照搬 | 给定整段照抄与空白差异折叠后仍比中的产物，当判定，则 `SourceOverlap`；低于阈值（12 字符或 6 西文词）的术语回标放行 | 2026-10-28 |
| phonetic_charset_limits_word_card_phonetics | 音标字符集限定 | 给定国际音标与拼音声调、越界汉字的音标字段，当判定，则前者放行、后者 `PhoneticCharset`；非词卡任务不判 | 2026-10-28 |
| image_input_skips_overlap_but_runs_the_rest | 图像输入跳重合照跑其余 | 给定图像输入与照抄式注，当判定，则无 `SourceOverlap`（无文本输入）；表情符号照常判罚 | 2026-10-28 |
| violation_lines_carry_position_and_rule_id_only | 违约行只带位置与规则标识 | 给定违约条目，当渲染行式与显示形态并摘要，则行为字段键、半角冒号与规则标识三段相连（语言中立，动作措辞住措辞注册表）、显示形态只含规则标识与字段键、摘要只含去重类别，都不带回复原文 | 2026-10-29 |
| math_inside_containers_stays_exempt | 容器内公式豁免 | 给定括注与引文内含成对公式的注，当判定，则无 `ContainerMixing`——数学容器内容不判语言（`R8`） | 2026-10-28 |
| line_wrapped_formulas_stay_math_containers | 换行公式仍成数学容器 | 给定公式跨段内软换行的注，当判定，则无 `PunctuationMismatch`（软换行不拆数学配对，段界仍拆） | 2026-10-28 |

### `crates/gloss-core/src/pipeline/langspec.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| bare_term_registry_matches_the_migrated_table | 登记面金样锁定初始清单 | 当解析嵌入数据源，则登记表恰为 `JSON` 一词（登记即评审，每收一词两侧金样同步扩行） | 2026-11-02 |
| bare_term_registry_rejects_non_word_shapes_and_empty_reasons | 登记面良构校验失败关闭 | 给定数字开头、含空白、横线开头、下划线开头与逗号缀尾的词形及空白缘由条目，当装入，则逐条按数据源损坏拒绝；合法词形 `Node.js` 通过 | 2026-11-02 |
| punctuation_faces_match_the_expected_tables | 允许表与登记基准对版 | 给定嵌入数据源装入的允许表（中西允许）与弯引号定界对，当与登记基准（场景标点面并中性面）双向比对、定界对与登记常量对比对，则集合相等且定界对序相等 | 2026-11-11 |
| empty_spec_carries_a_constant_curly_pair | 降级规范携带常量弯引号对 | 给定装入失败的降级规范，当读弯引号定界，则恒为常量兜底对且与登记基准定界对相等——降级路径容器定界不落空，扫描无下标可越（架构重构 `C1`） | 2026-11-11 |
| curly_quote_face_rejects_counts_other_than_two | 弯引号定界面定型为二元组 | 给定空、单字符与三字符的弯引号定界面，当装入，则逐条按数据源损坏拒绝（解析失败走有痕降级）；恰好两字符的嵌入源照常解析 | 2026-11-11 |
| attached_and_family_faces_match_the_expected_tables | 附随面与家族面同基准对版 | 给定装入的按场景附随面（中文取共用并场景合面、英文取共用面）与破折号省略号家族面，当与现表 `golden` 比对，则集合相等 | 2026-10-31 |
| emoji_faces_match_the_migrated_tables | 表情事实同 `golden` 对版 | 给定装入的表情区段、裸表情与变体选择符，当与判定器原常量 `golden` 比对，则区段逐对相等、字符面集合相等 | 2026-10-30 |
| policy_brackets_and_ranges_match_the_migrated_tables | 策略开关括号对与区段同 `golden` 对版 | 给定装入的盘古之白与简繁开关、括号对与六类文字系统区段，当与现表 `golden` 比对，则开关布尔、括号对序与区段逐对相等 | 2026-10-30 |
| simplified_pairs_match_the_migrated_table | 简繁对串整段对版 | 给定数据文件嵌入的对串（尾随换行裁去），当与迁出前常量整串比对，则逐字相等 | 2026-10-30 |
| char_faces_have_no_duplicate_symbols | 数据面内无重 | 给定嵌入数据源的十二个字符面（含中文场景附随两面），当逐面去重对账，则无重复符号 | 2026-10-31 |
| symbol_layers_stay_disjoint | 符号归位互斥 | 给定场景标点、中文场景附随（前邻、后邻）、中性、共用附随（前邻、后邻）与家族各面，当两两求交，则交集为空；数字夹住面只容与西文标点集重叠（句点逗号短横按位置双角色，`R3` 既定） | 2026-10-31 |
| ranges_are_wellformed_and_pairwise_disjoint | 区段良构且互斥 | 给定表情区段与六类文字系统区段，当逐段核对与跨类求交，则起止有序、段内不叠、类间不相交 | 2026-10-30 |
| simplified_pairs_stay_wellformed | 简繁对串良构 | 给定嵌入对串，当逐对核对，则长度为偶、繁简字形有别、按繁体码位升序且繁体不重 | 2026-10-30 |
| zh_and_en_allow_tables_share_only_the_neutral_face | 中西允许表互斥对账 | 给定派生允许表（场景标点面并中性面两份），当双向求交，则交集恰为中性面成员——场景标点两面无共同成员（`R2`） | 2026-10-30 |
| every_admitted_symbol_lands_on_exactly_one_layer | 现网符号恰属一面 | 给定收录面全体（场景标点、中文场景附随、中性、共用附随三向、家族、弯引号定界、句末标点与括号对），当逐符号查归位层，则恰属一面，句点逗号短横的西文标点与数字夹住双角色是唯一例外，表情事实不落任何层 | 2026-10-31 |

### `crates/gloss-core/src/pipeline/prompt.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| classify_prompt_carries_the_template_categories_and_the_text | 分类提示词携带模板类别与原文 | 给定原文，当 `render_classify`（双语），则用户消息为原文、系统指令含模板四个类别标识（含 `Fallback`）与 `kind` 契约、无残留占位符 | 2026-10-20 |
| text_kinds_render_system_and_user_with_kind_content | 文本 `kind` 渲染两段消息 | 给定四个文本 `kind`（词卡、句译、代码、通用），当渲染，则得 `[System, User]` 两段，系统指令含各自关键词（通用落“研究专家”，不带传统名号）与纯（JSON）输出契约，用户消息为原文 | 2026-10-29 |
| the_general_kind_is_never_named_by_the_classify_template | 分类模板不含 `General` 标识 | 给定双语分类模板渲染，当检查，则系统指令不含 `General` 标识——模板以 `Fallback` 类别指认兜底，到 `General` 的映射在校验层 | 2026-10-20 |
| output_example_carries_the_kind_fields | 输出示例携带 `kind` 字段 | 给定词卡、句译、代码解释模板（自包含任务书，含输出示例），当渲染并检查系统指令，则系统指令声明 `structured` 与 `note`，词卡示例另带 `phonetic/examples`、代码示例另带 `examples/code_language` | 2026-10-11 |
| missing_target_lang_defaults_to_chinese | 目标语言缺省中文 | 给定未设 `target_lang`，当渲染句译，则系统指令含“中文” | 2026-09-19 |
| explicit_target_lang_is_rendered | 显式目标语言渲染 | 给定 `Lang::En`，当渲染，则系统指令含“英语” | 2026-10-30 |
| image_explain_renders_system_and_image_parts | 图像解读渲染系统指令与图像部件 | 给定 `ImageExplain` 图像任务，当渲染，则得 `[System, User]` 两段，用户消息是 `Parts([ImagePng])`（字节经 `Arc` 零拷贝共享），系统指令要求按（JSON）格式输出且携带 `note/structured.examples` 契约 | 2026-10-29 |
| modality_mismatch_is_rejected_at_construction | 模态错配在构造时拒绝 | 给定图像 `kind` 配文本输入，当 `Task::new` 构造，则返回 `UnsupportedModality`（拒绝点在构造口，渲染路径不再兜底） | 2026-10-18 |
| render_template_substitutes_placeholders | 占位符显式替换 | 给定含 `{{`占位符`}}` 的模板，当渲染，则按值替换、同名占位符可重复；未声明与未闭合的占位符原样保留（留待完整性测试抓） | 2026-09-22 |
| render_template_drops_lines_whose_placeholders_are_empty | 占位符全空的行整行消失 | 给定提示行与指令行模板，当某行占位符值全为空，则该行连同行内静态文字与换行一并删除，其余行不受影响 | 2026-09-22 |
| render_template_keeps_blank_lines_without_placeholders | 无占位符空行是结构 | 给定模板里的空行（不含占位符），当渲染，则原样保留 | 2026-09-22 |
| every_locale_renders_without_leftover_placeholders | 全 `locale` 无残留占位符 | 给定 `locale` 中英两份与五个 `kind` 的全组合（四个文本任务加 `ImageExplain`），当渲染，则文本消息不含 `{{`、系统指令携带 `note` 纯 JSON 契约且不含旧围栏 | 2026-10-12 |
| english_locale_renders_english_prompts | 英文 `locale` 出英文指令 | 给定 `En` `locale` 的句译任务，当渲染，则系统指令为英文（含 `JSON` 契约）、用户消息携带原文，且不含中文指令词 | 2026-10-03 |
| prompt_locale_is_independent_of_target_language | 模板语言与目标语言解耦 | 给定 `En` `locale` 未设目标语言、`Zh` `locale` 设 `Lang::En`，当渲染，则前者英文模板下默认目标仍是中文、后者中文模板含“英语” | 2026-09-22 |
| prompt_locale_defaults_to_chinese | 模板语言缺省中文 | 给定未设 `prompt_locale` 的任务，当渲染，则结果与显式 `Zh` 逐字一致且含“词典专家” | 2026-09-22 |
| every_task_system_prompt_ends_with_the_discipline_after_the_shared_clause | 纪律块垫底且共享条款在其前 | 给定五任务两语言的渲染，当比对系统提示尾部，则与共享输出纪律块逐字一致、其前紧接共享目标语言条款（两段单点维护，拼接自渲染入口） | 2026-10-29 |
| classify_prompt_composes_the_skeleton_and_the_registry_category_section | 分类提示词骨架加类别段两段拼装 | 给定双 `locale`，当 `render_classify`，则系统指令以骨架（引言与输出形状）逐字起头、其后为注册表渲染的类别段（四类别逐条带定义），无残留占位符 | 2026-10-29 |

### `crates/gloss-core/src/pipeline/prompt/repair.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| repair_instruction_renders_without_leftover_placeholders | `repair` 指令渲染无残留占位符（完整性门禁） | 给定 `locale` 双语、四类违约与无细节加五个细节的全组合，当 `render_repair`，则渲染产物不含 `{{`、非空且含 `json` 字样（注册表骨架的键归注册表完整性测试管，不进本白名单） | 2026-10-29 |
| repair_conversation_replays_the_history_with_the_bad_reply | 修复对话以历史形态重拼 | 给定首轮 `[system][user]` 与坏回复，当 `repair_conversation`，则拼成 `[system][user][assistant][user]`（首两条逐字保留；`EmptyReply` 不经本函数，原样重发归 `next_round`） | 2026-10-20 |
| language_repair_instruction_carries_the_row_list_only | 语言修复指令只帧行式清单 | 给定两条违约条目与中英两语言，当 `render_language_repair`，则指令以回指句起头（只指称上一条回复的身份）、无残留占位符、行含规则标识与字段位置、不含回复原文、提及 `JSON`（端点 `JSON` 模式门读取） | 2026-10-29 |
| repair_instruction_composes_the_detail_wording_onto_the_skeleton | 细节措辞上修复指令 | 给定双 `locale` 与五个细节的全组合，当 `render_repair` 带细节，则指令行含细节位置与动作措辞、带违约规则标识、骨架（头、行、尾）齐备，且只携带结构事实（不含回复内容标记） | 2026-10-29 |
| repair_instruction_without_detail_rides_the_violation_level_row | 无细节回落违约级行 | 给定散文类 `NotJson` 与无细节违约（`EmptyReply`、`KindNotAllowed`），当 `render_repair` 不带细节，则指令行为违约级措辞行（位置、规则标识、动作），无残留占位符、不带契约行 | 2026-10-29 |
| repair_row_falls_back_to_the_display_wording_when_the_registry_is_missing | 注册表缺席回落固定措辞 | 给定骨架缺席或查键未中的组合，当渲染违约行，则整行回落违约固定措辞（`Display`），警告已在查找层留痕 | 2026-10-29 |
| language_repair_aggregates_same_field_and_category_and_sorts_by_field_order | 语言清单聚合并按字段序排序 | 给定乱序跨字段同类违约（注、音标、例句），当 `render_language_repair`，则同字段同类并一行（例句序号升序合并，如“第 2、5、7 条”），行序按产物字段序（注 → 音标 → 例句），中英位置单复数形态各自成立 | 2026-10-29 |
| language_repair_caps_rows_and_appends_the_truncated_note | 语言清单封顶并附截断注 | 给定聚合后 13 组违约，当渲染，则清单恰 12 行、截断注一行在场、第 13 组丢弃不溢出 | 2026-10-29 |
| language_repair_within_the_cap_carries_no_truncated_note | 清单未超限无截断注 | 给定聚合后 12 组违约，当渲染，则无截断注（清单不挤占修复轮上下文的口径只在超限时生效） | 2026-10-29 |
| language_repair_degrades_to_position_and_rule_id_lines_without_the_registry | 注册表缺席降级为位置与规则标识行 | 给定骨架缺席的语言清单，当聚合渲染，则每组回落为字段键、半角冒号加规则标识的行（语言中立，警告留痕） | 2026-10-29 |
| violation_briefs_render_human_position_and_rule_names_on_the_shared_row | 降级批注行人读词同构行式 | 给定两条违约（注缺空格、例句夹外文两条聚合）与注入的人读位置词，当 `violation_briefs`（双语），则行式与修复轮同构（位置、规则名、动作三段），例句占位符按聚合组序号填充、动作措辞逐字复用注册表、同字段同类并一行、未超限无截断注 | 2026-10-30 |
| violation_briefs_carry_structural_facts_only_without_leftover_placeholders | 降级批注行只载结构事实 | 给定注、音标、例句三字段违约与双语全组合，当渲染批注行，则无残留占位符、不含回复内容标记（批注只描述字段位置与规则，不引回复原文） | 2026-10-30 |
| violation_briefs_cap_rows_and_append_the_truncated_note | 降级批注行封顶附截断注 | 给定聚合后十二组违约与上限五，当渲染，则恰五行加注册表截断注一行（“其余为同类违约”）、被封顶组丢弃不溢出；上限十二时逐行齐出且无截断注 | 2026-10-30 |
| violation_briefs_of_an_empty_list_stay_empty_without_a_truncated_note | 空清单批注行恒空 | 给定空违约清单，当渲染批注行，则行清单为空且无截断注（界面据此回落泛化文案） | 2026-10-30 |

### `crates/gloss-core/src/pipeline/prompt/templates.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| output_example_carries_the_note_field | 输出示例携带 `note` 字段 | 给定双 `locale` 五份任务模板，当整行解析示例（JSON），则示例可解析且恒带 `note`（义，恒为首字段），流式渐进提取不受 `structured` 嵌套影响 | 2026-10-12 |
| output_example_values_are_neutral_placeholders | 输出示例值为中性占位 | 给定双 `locale` 五份任务模板，当检查示例值，则恒为`<string>` 类占位、不含具体词条——同分类契约的少样本偏置取舍（照抄示例的比例随示例显著性上升） | 2026-10-12 |
| every_task_template_example_passes_validate_task | 模板示例过生产校验器（完整性门禁） | 给定 `locale` 双语与五份任务模板的全组合，当取出示例对象行，则示例对象过对应 `validate_task`、`structured` 内键齐每个 `kind`（词卡 `phonetic` 加 `examples`、句译、图像、通用 `examples`、代码 `examples` 加 `code_language`） | 2026-10-12 |
| every_template_mentions_json | 所有模板必含 `json` 字样（完整性门禁） | 给定两个 `locale` 的七份模板（五份任务模板加 `classify.md` 与 `discipline.md`），当检查原文，则都含 `json` 字样（端点 `JSON` 模式的提示读取它）；修复指令的 `json` 字样由组合产物测试逐违约锁定 | 2026-10-29 |
| image_explain_example_is_a_single_json_object_with_note_first | 图像模板示例为单（JSON）且 `note` 在前 | 给定双 `locale` 的 `image_explain` 模板，当整行解析示例（JSON），则示例可解析、键恰为 `note` 与 `structured`（`note` 在前），`structured` 内只有 `examples`（`interpretation` 旧名退役）——流式与完成态解析都按这份契约 | 2026-10-11 |
| every_template_placeholder_is_declared | 模板原文占位符白名单 | 给定两个 `locale` 的八份模板原文（五份任务模板加 `target_lang.md`、`discipline.md` 与 `classify.md`；修复骨架已迁入措辞注册表），当扫占位符，则每个 `{{` 都有闭合且名字都在已声明集合内（`target`；嵌套契约的示例 `JSON` 以 `}}` 收尾，闭合计数因此可多于开启——按配对判）；未声明者若与已声明占位符同行会被整行删掉，产物断言看不见 | 2026-10-29 |
| task_templates_avoid_forbidden_legacy_markers | 任务模板不回传统框架措辞 | 给定中英两份禁用标记清单（中文十二条为框架模式加训诂词簇，英文十二条为传统框架短语），当扫描双 `locale` 五份任务模板正文，则零命中（界面印章与字段名是产品语言，不在受检面） | 2026-10-29 |

### `crates/gloss-core/src/pipeline/wordings.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| registry_frame_wording_is_present_for_both_locales | 注册表骨架词两语言齐备 | 给定中英两份注册表，当装骨架区与新增四区，则头、行、尾、截断注与回指句全部非空，连接符非空、例句位置两键携带 {{joined}} 且可渲染、分类段头与分隔符非空、目标语言名两变体非空（嵌入数据可解析由各测试的装载入口前置锁定） | 2026-10-30 |
| registry_covers_every_format_key_with_worded_rows | 注册表格式区键全覆盖 | 给定四类违约级键与五个细节键的双 `locale` 全组合，当查格式区，则键齐且位置与动作措辞非空 | 2026-10-29 |
| registry_covers_every_language_action_and_classify_definition | 注册表语言动作与分类定义全覆盖 | 给定十一个语言违约类别与四个分类类别的双 `locale` 全组合，当查对应区，则动作、规则名（界面语短名）与定义全部在场且非空 | 2026-10-30 |
| registry_keys_match_the_enums_exactly_in_both_directions | 注册表键与枚举双向集合相等 | 给定注册表三个枚举键区加规则名区，当把每键反解回枚举变体并对账键数，则无多键无缺键（键空间锁死，扩枚举须同步两份表数据） | 2026-10-30 |
| registry_carries_identical_key_sets_across_locales | 注册表键集中英对齐 | 给定中英两份注册表，当对比各区键集合（含规则名区），则逐键一致（措辞随语言分表，键空间不分语言） | 2026-10-30 |
| missing_keys_and_broken_sources_degrade_to_none_with_a_trace | 注册表缺键与坏源有痕降级 | 给定未登记键（含规则名区）与非法（TOML）源，当查键与装载，则返回空且警告留痕（无 `panic` 路径，组合层据此回落固定措辞或泛化文案） | 2026-10-30 |

### `crates/gloss-core/tests/stubs_behavior.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| config_store_mock_round_trips_secrets | 密钥存取桩往返 | 给定密钥未设置时读为 `None`，当 `set_secret` 后再读，则读回写入值 | 2026-09-19 |
| config_store_mock_round_trips_document | 配置文档桩往返 | 给定带非默认 `language` 的配置文档，当 `save` 与 `load`，则往返无损且与出厂默认可区分 | 2026-09-22 |
| chunk_delay_paces_the_stream | `chunk` 延迟为流定速 | 给定 30 毫秒 `chunk` 间延迟的三段脚本，当消费流，则内容按序且总耗时下界为两段延迟 | 2026-09-19 |
| failures_are_injectable | 失败位置可注入 | 给定注入的各类 `GlossError` 与流中 `Err`，当 `execute`，则失败在注入位置原样发生、流继续按脚本 | 2026-09-19 |
| execute_failure_once_fails_exactly_once | 一次性失败只生效一次 | 给定注入一次性失败与恢复脚本的引擎，当连续两次 `execute`，则首次返回注入错误、第二次照常产流 | 2026-09-19 |
| execute_panic_fires_on_first_poll | `panic` 注入在首次 `poll` 触发 | 给定注入一次 `panic` 的引擎，当在 `tokio` 任务里驱动 `execute`，则任务以 `panic` 收场 | 2026-09-19 |
| call_count_tracks_execute_invocations | 调用计数如实增长 | 给定多次 `execute`（含克隆体），当读 `call_count`，则共享计数如实增长 | 2026-09-19 |
| empty_script_yields_empty_stream | 空脚本产出空流 | 给定空脚本，当 `execute` 并消费，则立即结束 | 2026-09-19 |
