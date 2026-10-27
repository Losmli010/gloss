# task.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关符号时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），文件内不留指向本文件的指针。

## TaskOptions::prompt_locale

- 为什么挂在 `TaskOptions` 而不是 `Task` 顶层或通道消息上：`model_override` 已经确立了这个位置的口径——「触发时按配置解析、随任务冻结、执行途中不回读配置」的东西都放这里。放通道消息上要新开一条贯穿通道①②③④的字段（locale 不参与取材，走那么远没意义）。
- 代价（有意接受）：`None` 与 `Some(Zh)` 渲染结果相同、缓存 key 却不同。App 路径恒填 `Some(...)`，因此这只影响直接构造任务的测试；反过来若是给 `cache_key` 做归一化，就得在 key 派生里引入「配置默认值」这一层，代价更大。
- 为什么参与缓存 key 是**必须**的（任务卡点名的欠账）：locale 是运行时可变的模板维度（设置页切换语言），同输入不同语言的产物不能互相命中。实现上靠 options 的 serde 序列化在 `cache_key` 里自动全量参与（2026-10-10 起输入按变体直接哈希、不再整份序列化 task，options 仍走 serde，口径不变），另有专门测试钉住这条不易被察觉的耦合。

## on_accessibility_granted（2026-10-09）

- 2026-10-09 授权落定事件的处理：置位 `permissions_ready` 并复用 `send_secret_prewarm` 补发——门控语义统一为「没有系统权限引导弹窗在途」（启动预检通过，或运行中授权落定），keychain 授权框在授权落定后出现，保持「辅助功能 → 密钥」顺序；重复事件按已就绪静默忽略（观察器一次性，理论不可达，防御性兜底）。
- 补充（注释迁出）：仅启动预检未就绪的会话会收到本事件（授权观察器由组装点按预检结论武装，落定只产出一次）；就绪即无系统权限引导弹窗在途，keychain 授权框此刻出现符合引导顺序（辅助功能 → 密钥），用户无须重启或撞上首次任务。

## send_secret_prewarm（密钥预热命令）

- 发密钥预热命令（通道③，不带任务 span——预热不属于任何任务）。仅在预热门控就绪时发送——就绪即没有系统权限引导弹窗在途（启动预检通过，或运行中授权落定后），keychain 授权框（仅旧签名条目存在时出现）独占出现且晚于一切权限引导，读到的值进存储的进程内缓存，首次划词不再弹；未就绪时系统的授权引导弹窗异步在途且无落定回调，预热读的同步授权框会抢到它们之前，故跳过启动期预热（由授权落定事件补发），否则首次任务自然重读——两种路径下授权框都必然晚于权限引导。
- 推迟路径下任务自身的读密是同步阻塞（撞上未回填首读时卡在闸门/授权框上，取消不生效，见消费桥与 keychain 读的文档）。发送失败（通道已关，应用正在退出）只留痕。

## on_secret_prewarmed（预热回执）

- 只留痕——预热失败不拦主流程，首次任务会自然重读并按既有失败路径兜底。

## GlossApp::drain_events（消费通道④）

- 回传事件按代数/探测编号采纳——连续快速触发时旧代的产物被丢弃，浮层只显示最后一次请求的结果。状态决策在 machine，壳只做通道发送、浮层展示与日志。
- 浮层「什么时候露面」抽在 `machine::should_reveal`（内含 `machine::policy::auto_show_after` 的按批判定）：挂起显形请求由划词提交（内容到达才显形）置位，批处理后在同一帧消费；失败即弹（权限卡等）走批次判定。
- 先按批推进状态机、收集每条的采纳结果，再一次性决定这一批要不要露面：逐条 `|=` 等价于按批取或，抽出来是为了这条语义可测。
- 策略只关心「哪一类回传」，事件本身在下一行被消费掉。
- 预热回执与任务代数无关（不占代数，与浮层无关）：旁路处理后不进露面批次。
- 挂起显形在批处理**之后**消费：划词提交的置位点就在本批的 commit_probe 里——取前会把它拖到下一帧，取后同帧即显。
- 显形决策（守卫与「显形或失败即弹」的取舍）在 should_reveal：这里只递交挂起请求、机器当前视图与本批回传。
- 划词触发的浮层跟随选区（代数对得上时），否则居中；屏幕边缘钳制后显示，并按摆放意图记录——后续内容撑高窗口时才能按同一意图重定位。

## GlossApp::accept_input

- 采纳取材产物：探测命中走提交段（内容到达才显形——出的是**待确认卡**，
  2026-10-27 前直接下发推理），其余按陈旧产物丢弃（收起后的取材、被顶掉
  的旧探测）。返回是否进入了需要展示浮层的新任务。
- 探测编号在提交时才提升为代数，陈旧过滤按编号而不是代数。

## GlossApp::commit_probe（编排侧）

- 取材产物到达，划词与剪贴板图片共用：状态机接管（旧会话让位、代数提升、视图整卡换待确认卡），置挂起显形——出窗与重定位由 drain_events 在同帧统一执行（那里才有 ActiveEventLoop）。内容闸门命中时探测作废但**当前显示保留**（它属于上一个会话），只记一行 warn。
- 提交只落卡不下发（2026-10-27，发送前确认方案）：`InputOutcome::Pending` 分支记一条 info（「待确认」是默认日志级别下可见的进度痕迹）并置挂起显形，不 send_run——通道三零流量，推理等用户点「注疏」（`handle_overlay_action(Confirm)` → `send_run`）。
- 提交日志只记字节量不记内容：图像字节是用户敏感内容，不落日志。
- 内容到达才显形：从 Idle 出窗、从已显示改锚点重定位，误滑（探测失败）永远走不到这里；用户看到的显形是待确认卡而非流式骨架。

## GlossApp::send_run

- 通道③发送；接收端不可用（tokio 桥死亡）时状态机降级落 Error。调用方只有确认动作（`OverlayAction::Confirm`）与重试动作（`OverlayAction::Retry`）两处——取材路径不再直达这里。

## 编排侧的采纳包装（accept_chunk / accept_repairing / accept_classified / accept_done）

- `accept_chunk`：采纳流式增量。返回是否有新内容需要重绘。
- `accept_repairing`：采纳修复轮开始：清上一轮流式正文并切「重新注解」页脚。返回是否需要重绘。
- `accept_classified`：采纳自动分类结果（头部任务标签的更新点）。返回是否需要重绘。
- `accept_done`：采纳任务产物：定格正文并进入 `Show`。返回是否需要重绘。

## GlossApp::accept_failed

- 采纳任务失败：划词探测命中走探测失败处置（误滑静默丢弃 / 权限卡显式反馈），其余按推理失败处置（落 `Error` 态弹失败卡，文案与动作出口由 machine 按错误类别给出，见 `machine::error_action`）。返回是否需要展示浮层。
- `SilentlyDropped` 臂不可达：静默丢弃只发生在划词探测段（上方已分流）。防御性兜底——照陈旧语义丢弃，不碰窗口。
- `Ignored` 臂：陈旧失败的丢弃是竞速语义（新划词覆盖旧划词），但这条线正是「划了没弹窗」的排查盲区——升为 info 保证默认日志可见。

## GlossApp::fail_probe（探测失败）

- 划词的空选区/读不到按误滑静默丢弃——不留窗口动作、不留弹窗（若浮层正在显示，当前内容原样保留），只留一条带前台应用的排查痕迹，那是「划了没反应」的唯一日志线索；其余失败（划词权限缺失、剪贴板图片超上限等）落失败卡，经「失败即弹」显形（从 Idle 出窗或顶替已显示内容——真实故障不该被吞掉）。

## TaskInput::Image.region（2026-10-10，剪贴板图片方案 §5.1）

- 为什么 `ScreenRect` → `Option<ScreenRect>`：截图区域坐标只有框选来源才有（区域在屏幕上），剪贴板图片来源的内容在粘贴板里、没有屏幕锚点。改动时点零生产者（模态矩阵早已放行 Image 列，但取材端口尚未落地），类型改动免费；未来框选来源带 `Some(rect)`，剪贴板来源 `None`。
- 消费端口径（已随方案步骤 2 落地）：缓存 key 把 region 一并哈希、`None` 也参与（app 侧 `cache_key` 按变体直接哈希，`ScreenRect` 为此补了 `Hash` derive，见 `cache_comment.md`）；UI 尺寸角标仅 `Some` 时显示。

## TaskKind::ImageOcr 与 OutcomeStructured::Extracted 删除（2026-10-10，契约校验与重试方案 §10 第一条 / 步骤 1a）

- 为什么删：`ImageOcr` 从未有自己的模板（prompt 侧对它恒拒 `UnsupportedModality`，从未发出过请求），域内不存在可执行路径；图像输入唯一合法 kind 已是 `ImageExplain`（恒定映射，跳过分类）。留着它，模态矩阵、分类允许清单描述、eval 必需键表都要为一张「永远走不通的格子」付穷尽匹配税。
- `Extracted` 随之无消费者：解析侧（finalize 两处）、UI 渲染分支（popup `outcome_body`）、eval 镜像（`mirror_complete`）里所有只为它存在的分支一并删除；「凡 N 言」提取小记（`extract_note` + `gloss_popup_seal_note` 词条）是它的唯一 UI 伴生，同删。
- 保留 `RegionCapture` 端口与框选手势管线：那是**图像输入的取材路径**，与任务类型解耦（取材产出 `TaskInput::Image`，kind 由编排层定）；删 OCR 任务类型不动取材能力。
- 连带：`TaskInput::Image.region` 等图像输入设施、`ImageExplain` 契约、剪贴板图片管线全部不动；`modality_matrix_is_enforced_cell_by_cell` 的矩阵收敛为 4 kind × 3 输入。

## OutcomeStructured 三变体与 TaskOutcome.degraded（契约校验步骤 3，2026-10-11）

- 三变体收敛（WordCard / Plain / Code）：`ImageCommentary` 与 Plain 同形
  （interpretation 改名 examples 后字段完全一致），保留两个同构变体只会让
  每个 match 多一臂；`Extracted` 已随 OCR 删除（步骤 1a）。图像解读、句译
  （未来还有步骤 4 的 General）都落 Plain——「变体数 = 渲染形态数」，不是
  「变体数 = 任务数」。
- `code_language` 自 TaskOutcome 顶层移入 Code 变体：顶层字段暗示所有 kind
  都有语言判定；只有代码任务有，字段住进变体后「拿错变体取语言」在类型层
  不可能。
- `degraded` 挂 `#[serde(default)]`：缓存/事件里旧形态的反序列化落
  false（正常路径）——default 的语义是「没有标记 = 没降级」，不是「缺字段
  = 降级」。

## TaskKind::General 变体（2026-10-12，契约校验与重试方案 §8 / 步骤 4）

- 变体放枚举末位：`TaskKind` 派生 `Ord`（eval 混淆矩阵的 BTreeMap 键），尾部插入让既有键的迭代序不动，报告列序与既有夹具不漂移。
- 「不进 CLASSIFY_KINDS」是本变体的定义性约束：分类器只在三个文本 kind 中选择，General 是清单外的执行落点（分类清单渲染不为它产生行，`the_general_kind_is_never_offered_to_the_classifier` 钉住）。`CLASSIFY_FALLBACK` 本步仍指 TranslateWord——换值归步骤 5（与分类重试同 PR），本步只备好可执行、可渲染的落点。
- 模态矩阵 General × Text 合法（与三个文本 kind 同格）；`empty_structured`/`structured_of` 落 Plain——「变体数 = 渲染形态数」，句译/图像/通用同形。

## TaskOptions.model 改 Arc<str>（P1-1 clone 削减，2026-10-13）

- 选项在状态机「在途副本」与「下发请求」之间共享同一份模型 id：此前每次
  下发深拷贝一个 String；改 `Arc<str>` 后 `options.clone()` 只递引用计数，
  machine/lifecycle 基准少一次分配。serde 已开 `rc` 特性，wire 形状不变
  （仍是纯字符串），缓存 key 不受影响。
- **Config.model 维持 String 不动**：设置页草稿把 `draft.model` 当文本
  缓冲直接 `&mut String` 编辑（TextEdit 原地改写），Arc<str> 不可变，
  改它就得给草稿另配影子缓冲——复杂度大于收益。代价是状态机侧
  `task_options` 用 `Arc::from(config.model.as_str())` 每探测仍分配一次
  （与原 String clone 同价），只有选项复制那一跳变零分配。
- 备选「TaskInput::Text 的 text 也改 Arc<str>」被否：原文在视图源、在途
  副本、通道载荷三处各有所有权，Arc 化能把三次分配压到一次，但
  `TaskInput` 是跨四 crate 的公共枚举，全部 `assert_eq!(text, "...")` 形
  测试与取材构造都要跟着改，P1-1 不扩这么大。

## General 的落点语义更新（2026-10-20，PR #218）

- 上文「不进 CLASSIFY_KINDS 是本变体的定义性约束」的表述失效：常量已删，
  类别集收进 classify 模板。现行口径——分类模板以 `Fallback` 类别指认
  兜底，`validate_classify` 把 Fallback 映射 `TaskKind::General`；分类
  失效（修复耗尽/请求异常）仍落常量 `CLASSIFY_FALLBACK`（同一 kind）。

## TaskKind

- 任务类型。新增场景 = 加变体 + prompt 模板 + 结构化结果变体 + UI 模板；管道只搬运载荷，不按类型分支。
- `TranslateWord`：单词或短语，落词卡产物（音标与例句在 `structured`，释义在 `note`）。
- `ImageExplain`：图像解读，唯一接受 `TaskInput::Image` 的 kind（见 `Task::new`）。
- `General`：通用任务，不在分类模板的类别清单里，是分类 `Fallback` 类别与分类失效时的执行落点。

## TaskInput

- 输入模态：取材产物的统一形态，随消息 move 所有权。
- 不携带模态提示：源语言与代码语言都由模型从原文判断，代码语言经 `OutcomeStructured::Code` 回传 UI。
- `Image.png`：PNG 字节；编码成 data URL 归传输层。

## TaskOptions

- 任务选项：全部字段都是**单次快照冻结**的——App 在触发时按配置快照解析填入，执行途中不再回读配置。
- `target_lang`：目标语言；`None` 走缺省中文（`crate::prompt` 的 `DEFAULT_TARGET`）。
- `model`：本任务使用的模型 id；`Arc<str>` 让在途副本与下发请求共享同一份，复制只是引用计数。缺省值只服务反序列化兜底与测试直构，App 路径恒按配置快照填入。

## TaskOptions::prompt_locale（字段注释补记）

- prompt 模板语言；缺省中文。它参与缓存 key——换模板语言后同输入不得命中旧语言的产物。

## TaskOptions 的 Default 实现

- 只服务反序列化兜底与测试直构。生产路径必须全量显式构造（见 `gloss-app` 的 `task_options`）——`..Default::default()` 会静默带上工厂模型，把调用方本应自己决定的参数掩盖掉。

## Task::new

- 任务的唯一构造口：模态约束在这里判定，`kind` 与 `input` 的非法组合不可表示。`TaskOutcome` 的 kind 由执行路径决定，构造只认输入形状。

## Task::options_mut

- 选项可在渲染前调整（目标语言、模板语言）；模态不变量只认 kind 与 input，与选项无关。

## TaskParts

- 反序列化的中转形态：`Task` 的字段私有，wire 入口也必须过 `Task::new` 的同一道模态闸门，否则夹具与持久化路径会成为绕过不变量的旁路。

## validate_modality

- 模态约束表：任务类型与输入模态的合法组合。唯一被拒的错误是 `GlossError::UnsupportedModality`。

## TaskOutcome

- 任务产物：`note` 是流式拼接的 markdown 正文，`structured` 是 UI 精排字段。
- `note`：markdown 正文；降级产物恒为空串（原文垃圾不展示不留存）。
- `structured`：按 kind 的附加字段，见 `OutcomeStructured`。
- `degraded`：契约校验失败后的兜底产物标记（此时 `note` 为空串）：不进缓存、UI 出降级卡。`serde(default)` 是为兼容缺该字段的旧工件——没有标记即视为正常产物。

## OutcomeStructured

- 按 `TaskKind` 分派的附加字段：释义在 `TaskOutcome::note`，这里只带音、例与语言判定。三个变体与 `crate::schema` 的 wire 契约同构。
- `WordCard`：词卡，音标（模型给出时才有）与例句。
- `Plain`：句译、讲解、图像解读与通用任务共用，展开条目列表。
- `Code`：代码讲解，展开条目 + 模型判定的语言（不确定为 `None`）。
