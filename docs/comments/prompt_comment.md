# prompt.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关符号时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），文件内不留指向本文件的指针。

## 模板文件化（R1，2026-09-22）

- 为什么把三个模板从 `format!` 里的内嵌字符串挪到文件：老形态改一句提示词就要改 Rust 并重编译，且模板与代码挤在同一个 match 里（三份散文 + 一份契约散文），改一处得读整段函数。任务卡 R1 的原话是「模板文件改动不需要碰 Rust 代码」。
- 为什么目录落在 `crates/gloss-core/prompts/{zh,en}/` 而不是仓库根：`include_str!` 的路径相对**当前文件**解析（不是 crate 根），放根目录要写 `../../../prompts/...`，路径越长越脆；放消费它的 crate 里最短、也最容易被改模板的人找到。
- 为什么每个 locale 是 5 份文件（三个 kind 指令 + `contract.md` + `hints.md`）：契约散文与提示行是三个 kind 共用的，抄成三份就是三处漂移源（改契约漏一处）。文件数从 3 变 5，换来的是每类文本各有一份单点。
- 备选与取舍：
  - `rust-embed`：能在 debug 期读盘（改模板免重编译），代价是一个新依赖 + 一套 folder/prefix 配置。本项要的是「编译期嵌入」这个已定决策，std 的 `include_str!` 就够（还顺带在编译期校验 UTF-8）。真要热重载，那是配置化模板的独立话题。
  - mustache / handlebars 模板引擎：为 5 个占位符引入引擎（section、lambda、HTML 转义语义）不划算；且它们的「只对 section/comment 生效」的 standalone 行规则**不表达**本项要的「可选行」语义——照搬会得到一个看似规范、实则不匹配的模型。
  - `format!` + 转义：占位符自身就是花括号，`format!` 的 `{}` 插值语法要求模板把每个 `{{target}}` 写成 `{{{{target}}}}`，模板里满是转义、改一句提示词就得数括号，直接否掉。
- 为什么 `contract.md` 里用 `{{fence}}` 而不是直接写围栏字面量：围栏标记是代码与解析器共用的事实源（`STRUCTURED_FENCE`），写死在模板文件里就等于开了第二处，改一处忘一处会让正文剥离静默失效。

## render_template / substitute_line

- 为什么自写逐行替换而不是 `str::replace` 循环：`replace` 无法表达「可选行」，也无法在渲染后发现自己漏了占位符；逐行扫描顺带拿到「这行有没有占位符、值填了没有」的判断依据，还给完整性测试留下「未声明的占位符原样保留」这个可观测行为（否则打错字会静默少一段提示词）。
- **可选行语义**（关键取舍，2026-09-22）：判定是「一行里的**已声明占位符全部为空**⇒ 整行删除（含行内静态文字）」，而不是「替换后该行只剩空白」。前者的直接动机是提示行：`代码语言：{{code_lang}}` 的标签是行内静态文字，值为空时这行永远不可能「只剩空白」，用后者的规则提示行根本删不掉（实现时先按后者写，两个测试当场红出来）。
- **行划分规则**（上一条的代价与用法，评审补充 2026-09-22）：一行里若既有必填值又有可选值，必填值为空时整行消失、可选值一起没了。因此模板的行划分是**同一行只放一个语义角色**：必填值独占一行、可选值独占一行。必填值独占一行是允许的（指令行的 `{{target}}` 就是），但代码必须保证它不为空——这是 `target_display` 的职责（见 lang_display 节）。
- 为什么空值判定是 `is_empty()` 而不是 `trim().is_empty()`：值都由代码生成（语言名、代码语言标识），不含需要宽容的空白；用严格判据让行为可预测。
- 保留行为：不含占位符的空行是模板结构（段落分隔），一律不动；未闭合的 `{{` 之后的内容原样跟随，不吞掉。
- `all_empty` 用结构体 `RenderedLine` 返回而不是三元组/`Option`：这个布尔承载的是一条要写进文档的语义（该行是可选行），起个名字比在调用点拼元组可读。
- 完整性测试要**两层**（评审补充 2026-09-22）：`every_locale_renders_without_leftover_placeholders` 查渲染产物，`every_template_placeholder_is_declared` 直接查模板原文的占位符白名单。后者不可省——模板打错字（`{{taget}}`）若与某个已声明占位符同行，整行会被按可选行删掉，渲染产物里也就看不到那个残留的 `{{`，只靠产物断言是测不到的（评审指出的盲区）。

## Locale（R1 时名 PromptLocale，任务 09 更名并上移 model.rs）

- 为什么是二态（`Zh`/`En`）而不是复用 `config::Language` 三态：`Language::System` 是**用户的偏好**，`Locale` 是**渲染输入**——偏好要持久化，渲染输入必须具体。落定动作（读系统语言）在壳侧做一次，core 不认识系统。
- 为什么叫 `Locale` 而不是 `PromptLocale`（任务 09）：它已是**两条**选表依据——prompt 模板（产物给模型）与界面文案表（`i18n::Text`，界面给用户）。名字里带 prompt 会把第二条用途读成越界；`Lang`（产物语言）与 `Locale`（界面语言）两者解耦，各有测试钉住。
- 为什么定义在 model.rs 而不是 prompt.rs（任务 09 落实 R1 的预案）：R1 记录过「若 `TaskOptions` 之外的类型也要携带它，就挪到 model.rs，双向依赖即消散」。任务 09 的 UI 文案表正是那个「之外的类型」，于是按预案上移。`prompt.rs` 与 `task.rs` 的**双向模块依赖就此消散**（`task` 与 `prompt` 都只依赖 `model`），`config.rs` 也不再为 `Language::resolve` 依赖 `prompt`。
- 为什么 `Locale` 落进 `TaskOptions` 而不是 `AiTaskService::execute` 的参数：`execute` 已经用「任务自带模型」（`options.model_override`）的口径拿模型，locale 与模型同性质——触发时定、随任务走、进缓存 key。放进 options 还自动让 `cache_key`（整份 task 序列化）把它算进去，不需要单独给 `cache_key` 加参数。
- `None` 与 `Some(Zh)` 渲染结果逐字一致，但 `cache_key` 不同（整份 task 序列化）。这条冗余是接受的：`None` 只出现在「不经过触发路径」的构造里（测试、将来的批量工具），真实触发路径一律显式落定 locale（`machine::task_options` 用 `Some`），所以线上不会出现两者并存而互不命中的情形。

## lang_display

- 为什么语言显示名随 locale 变（`中文` / `Chinese`）：模板语言换了、目标语言名还是中文，模型拿到的是半汉半英的指令。语言名是固定词表（`Lang` 六态），留在代码里而不进模板文件：它随枚举增删而变，与散文的维护节奏不同。
- 目标语言的**缺省**不随 locale 变：缺省仍是中文（`TaskOptions::target_lang` 的文档口径），英文模板下写 "Chinese"。模板语言是「用哪种语言跟模型说话」，目标语言是「产物给谁看」，两者解耦（有测试钉住）。
- 为什么 `target_display` 对空显示名回落 `DEFAULT_TARGET`（评审整改，2026-09-22）：指令模板里 `{{target}}` 那一行**全是静态文字 + 这一个占位符**，是「必填值独占一行」。若用户把手填语言名清空（`target_lang = { Other = "" }`，配置文件手改即可构造），按可选行语义整条指令会被删掉，系统指令只剩契约段——旧实现只是语言名为空、指令保留，这是会被用户看见的功能回退。回落比「配置边界拒空」便宜：只此一行，且不引入新的校验失败面（与 `target_lang` 既有文档的「缺省中文」口径一致）。
  - 连带订正一处记录措辞：本仓库**确实有**「必填值独占一行」的写法（指令行的 `{{target}}`）——早先记的「本仓库不用这种写法」不成立。准确口径是：**不能把必填值与可选值混在同一行**（混了则必填值为空时可选值一起消失）；必填值独占一行是允许的，但要保证它不为空。
  - 空名判据用 `trim().is_empty()`，与 `substitute_line` 的 `is_empty()` **不同口径且各自正确**：`substitute_line` 判的是「占位符值是否为空」（值全由代码生成，严格判据让行删除可预测），这里判的是「用户手填的语言名是否有效」（配置文件手改得来，可能带空白，宽容判据才拦得住）。
  - 为什么不给这次回落留日志痕迹（复审提出，2026-09-22）：它不是运行时故障降级，而是**无效输入归一化**——与 `Config` 的 `#[serde(default)]` 缺字段回落出厂默认同性质（`missing_fields_fall_back_to_defaults_on_deserialize` 也无日志）。AGENTS 的「降级＝有痕迹」针对 keychain/网络/剪贴板这类**故障**；把配置里的空名当故障报，得先有配置边界的逐字段校验（任务 07 那套），那属独立任务。另：`prompt.rs` 的渲染是纯函数式路径（无 IO、无生命周期状态），日志在 core 里都挂在 `config_handle` / `engine` 这类有 IO 与生命周期的路径上，不该为一个显示名归一化把副作用引进渲染。（订正：先前的记录写成「gloss-core 至今零日志调用」，与 `config_handle.rs` 的 `error!` 与 `engine.rs` 的缓存日志不符。）
- `Lang::Other(name)` 原样透传：那是用户自填的名字，没有可翻译的语言名。

## schema_json

- 为什么契约的 schema 不进模板文件：它与 `engine::parse_structured` 的解析器是同一份字段契约（字段名改了要两边一起改），抽成文件后「改 schema 忘改解析器」会静默坏。契约的**散文**（怎么输出这个块）随文件本地化，**字段定义**留在代码里。
- 为什么 schema 的示例值（`词条原文` 等）不随 locale 变：字段名是给解析器的，示例值是给模型的提示；让示例值随 locale 变意味着同一份字段结构在代码里出现两份措辞，改 schema 的人要同时改两处——这正是上面那条要避免的。代价是英文模板里会出现中文示例值，属可接受（模型多语言，且字段名本身是英文）。
- kind 与 schema 的对应关系没变（词卡给 senses/phonetic，句译与代码解释给 title，OCR 给 text），只是从 `format!` 的插值变成 match 返回 `&'static str`，少一次拼接。

## render

- 步骤顺序保持原样（模态校验 → 取模板 → 渲染）并在渲染前落定 locale：模态非法时不产生任何模板工作，与旧实现的失败面一致。
- 用户消息的提示行与系统指令共用同一次 `hint_block` 结果：两处标签必须一致，分别渲染两份就会漂移（旧实现也是同一个 `hint_lines` 调两次）。
- 提示行片段末尾换行由 `hint_block` 负责去掉（`trim_end_matches('\n')`），两个注入点各自决定换行：系统指令里占位符独占一行、用户消息里另起一段。模板文件以换行结尾是文本文件的常规写法，不该由模板作者去猜这件事。
- 与旧实现的**有意差异**（评审核对后订正清单与数量，2026-09-22）：
  1. 系统指令末尾多一个空行：模板文件以换行结尾，渲染结果因此以**两条** `\n` 收尾（旧实现无尾随换行）。系统指令的尾随换行对模型无影响，为它做归一化反而要引入「渲染结果尾行特殊处理」这类隐藏规则，不值当。
  2. 空值提示：`InputHint::CodeLanguage("")` 这类空值下旧实现会渲染出孤零零的标签（`代码语言：`），新实现整行删除。这是可选行语义的必然结果，方向是对的（不该向模型发半截提示），但属可观察的行为变化，记录在此。

## 分类提示词的少样本偏置与判别规则（2026-09-30）

- schema_json(Auto) 原写死 `{"kind":"TranslateSentence"}`：输出契约里的示例值
  对模型就是少样本示例，照抄示例类别的比例随示例显著性上升——代码解释
  （尤其无围栏的命令/配置片段）被系统性误判进句子翻译。改为中性占位
  `{"kind":"…"}`：契约只示范形状，具体值由 {{allowed}} 清单给。教训与
  accessibility_comment.md 的 FFI 常量条目同类：契约位不放具体值。
- 判别规则（classify_rules_block）只补清单判据一句话说不清的边界（命令、
  报错堆栈、配置与数据格式片段没有代码围栏也属代码解释；完整句子夹杂
  术语仍属句子翻译），**不引入少样本示例**——规则约束类别边界而不示范
  输入，不会复制偏置问题。
- 规则按 kind 条件渲染而不是写死在模板：设置页停用某个 kind 后，指向它
  的规则等于推着模型选一个必然被解析拒绝的项。复用模板引擎的「可选行」
  语义（标签并进规则段、清单里没有带规则的 kind 时整段消失），与
  hint_block 同一实现习惯。
- 措辞刻意克制：每条规则一句话、不带输入样例；描述增强（ExplainCode 补
  「命令行、报错堆栈、配置与数据格式」，TranslateSentence 补「自然语言
  …可夹杂术语」）与规则行措辞对齐，双语同构。
- 验证口径：just eval-replay 全绿且与改前同分布（夹具驱动、不触 prompt，
  防回归靠它钉住解析器行为）；live 轨（just eval，需 GLOSS_LIVE_*）实测
  code-* 混淆簇改善属 opt-in 步骤，待有凭据环境执行（分类缓存进程内
  1h TTL，重启即失效，无需迁移）。

## MessageContent 与 ContentPart（2026-10-10，剪贴板图片方案 §5.4）

- 为什么 `ChatMessage` 不再 `Serialize`：此前域形态与 wire 形态恰好重合（content 就是 JSON 字符串），`messages_serialize_to_openai_shape` 把这个巧合钉在了序列化层。多模态引入后两形态分叉（content 字符串 vs OpenAI content 数组），wire 组装整体搬到 `engine::llm::chat_request_body`（`message_to_value`/`content_to_value`/`part_to_value`），该序列化测试随之删除、由请求体快照测试接管。
- 为什么域类型不携带 base64：编码是纯传输细节且体积膨胀 4/3 倍；域部件 `ContentPart::ImagePng(Arc<[u8]>)` 只搬原始字节，保持 `PartialEq` 按字节可比、Arc 跨线程零拷贝共享，也不存在「编码态与原始态并存」的双形态漂移。data URL 前缀与 base64 编码只在 `part_to_value`（请求体构造处）发生。
- 备选与落选：让 `ChatMessage.content` 直接存 `serde_json::Value`——域层被 wire 词汇（`image_url`、data URL）侵蚀，prompt 层就得 import 传输知识，分层反向。

## image_explain 模板（2026-10-10）

- 为什么单请求 JSON 双字段（`note` 在前、`interpretation` 数组在后）：描述与解读一次往返拿到；字段次序固定 note 在前，与流式提取器「note 值闭合即可渐进上屏、interpretation 完成态落」的顺序对齐。两请求链式（描述 → 解读）双倍延迟与成本，落选。
- 示例值恒为占位（`"…"`）：既有少样本偏置取舍——写死具体示例，模型照抄示例内容的比例随显著性上升。
- `PromptRegistry::render` 的 Image 分支：ImageExplain 走 image_explain 模板 + 用户消息 `Parts([ImagePng])`；ImageOcr 无模板仍报 `UnsupportedModality`（OCR 随其独立特性落地）；文本分支行为零改动。

## render 图像分支的 kind 复核删除（2026-10-10，契约校验与重试方案步骤 1a）

- ImageOcr 删除后，`validate_modality` 已保证 `Image` 输入 × 任何 kind 只有 `ImageExplain` 合法，render 里 `task.kind != TaskKind::ImageExplain` 的二次判断成为不可达分支——删掉，模态约束表保持唯一闸门；图像分支的注释改为说明这一点。

## 任务模板整体嵌套化（契约校验步骤 3，2026-10-11）

- 步骤条目只点名「image_explain.md 改嵌套改词」，但 §3 的三行同构表对
  **全部 kind** 生效：词卡/句译/代码模板若留在扁平示例，模型照模板输出
  扁平 JSON，`structured` 恒缺失 → 软回落空变体 → 词卡永远没有音标/例句
  ——功能回归，不是风格问题。因此四份任务模板 × 2 locale 全部改为
  `{"note":"…","structured":{…}}`，image_explain.md 另做 interpretation →
  examples 改词。
- 模板示例行与 `schema::validate_task` 的唯一性关系（「示例就是解析侧所吃
  形状的唯一描述」）由此获得门禁：`every_task_template_example_passes_
  validate_task` 锁三件事——模板含 json 字样（端点 JSON 模式读它）、示例
  对象过 validate_task、structured 内键齐 per kind。扁平示例也能过
  validate_task（硬约束只有 note），所以「structured 在场且键齐」的断言
  不能省。
- `every_template_placeholder_is_declared` 的括号检查从「{{ 计数 == }}
  计数」改为「每个 {{ 有闭合」：嵌套示例 JSON 以 `}}` 收尾，闭合计数天然
  多一——配对成立即可，计数相等从此是错误判据。

## general.md 任务书（2026-10-12，契约校验与重试方案 §8 / 步骤 4）

- 双 locale 各一份三行模板，与句译/代码同构（指令行 + 纯 JSON 契约行 + 中性占位示例行）；`{{target}}` 是唯一占位符——通用任务同样以目标语言书写回应。
- 任务书刻意不设判别性引导（不偏向「翻译」或「解释」）：它是分类失败后的兜底落点，任务书越中性，兜底产物越能如实回应用户给的任意内容；示例行与句译/图像同形（`{"note":"…","structured":{"examples":["…","…"]}}`），值恒占位（少样本偏置取舍同全仓）。
- `classify_allowed_block` 补 General 臂只为穷尽匹配（该 match 无通配臂，编译器强制）；生产路径 `allowed` 恒传 `CLASSIFY_KINDS`，General 的描述行永远渲染不出来。`classify_rules_block` 不加显式臂：`_ => None` 通配已表达「清单外 kind 不产生规则行」，再写一条 `(General, _) => None` 是重复信息。
- 方案 §8 改动面的「任务标签 i18n 各加一条」在当前仓库**无落点**：按 kind 的任务标签（页头药丸 + `ui::kind_label` + 对应词条）已随经注疏落地有意删除（见 popup.rs 决策记录 2026-09-30「任务药丸与头部 spinner 的删除」——「任务由内容层（经注疏印章）自明，头部只剩图标 + 动作区」），i18n 表里不存在可扩展的 per-kind 词条集；新增无消费者的 `Text` 字段会被 dead_code 门禁拦下。故本步不加词条，任务类型经产物内容自明。若后续恢复任务标签设计，应随新设计一并补全全部 per-kind 词条，而不是只补 General 一条。

## repair.md 与修复轮指令（2026-10-12，契约校验与重试方案 §6）

- 为什么契约示例行运行时从任务模板提取（`task_contract_line`）而不落成 Rust 常量：模板是输出契约的唯一事实源（完整性门禁锁定「示例行过 validate_task」），zh/en 示例行还存在方言差（「…或 null」vs「…or null」）——运行时提取让任务书与修复指令引用同一行，措辞漂移在结构上不可能；常量方案则需要第三处同源测试锁两份拷贝。
- 为什么截断提示（`{{hint}}`）放代码不放模板：hint 是条件出现的内容，`render_template` 的可选行语义（占位符全空整行删除）正好承载「length 才出现」；模板里写死文字就无法条件化。
- 为什么 `repair_conversation` 收 `Option<&str>`：EmptyReply 原样重发 = 首轮消息逐字返回（不带 assistant 轮也不带纠错指令——无内容可纠，指令没有指向对象，且部分端点拒空 assistant content）；`Option` 让「带不带历史」在类型上显式，两个消费方（classify/engine）共享同一形态。
- `Role::Assistant` 的首个落地：多轮对话预留点的首次真实使用，wire 序列化（小写 `assistant`）此前已就绪，零传输层改动。

## STRUCTURED_FENCE 删除与模板完整性门禁收口（2026-10-12，契约校验与重试方案步骤 6 / §10）

- `STRUCTURED_FENCE` 常量整体删除：它的两个消费者（完成态围栏 fallback、分类围栏容错提取）分属步骤 3 与步骤 6 删除，常量随之失去存在理由——R1 时代「`{{fence}}` 单一事实源」的取舍针对的是「正文剥离解析器与模板共用标记」，那套围栏契约退役后，`every_locale_renders_without_leftover_placeholders` 里的防回归断言改用字面量 `"```gloss"`（断言保留：模板正文永远不该出现旧围栏标记）。
- 「所有模板必含 json 字样」从 `every_task_template_example_passes_validate_task` 拆出为独立的 `every_template_mentions_json`，并从五份任务模板扩到全部七份（+ classify.md + repair.md）：§3 的口径是「所有模板」——端点的 JSON 模式读的是**当次请求**的完整提示，分类请求与修复指令同样是 JSON 契约请求，同样需要 json 字样；塞在 validate_task 测试里名实不符也盖不住后两者。一个测试一个行为，bdd 各自登记。
- `repair_instruction_renders_without_leftover_placeholders` 补 repair.md 的渲染门禁：全组合扫（locale × 四类违约 × 有无截断提示 × 三种契约行，含空契约行的可选行删除路径）。此前 repair.md 的无残留断言只散在措辞测试的四个组合里；general.md 与任务模板的同一门禁由 `every_locale_renders_without_leftover_placeholders`（全量渲染）承担，两者合起来覆盖 §11「repair.md/general.md 渲染无残留占位符」。

## 语言显示名改 Cow（P1-1 clone 削减，2026-10-13）

- `lang_display`/`target_display` 改返回 `Cow<'_, str>`：常用语种借静态
  字面量、`Lang::Other` 借配置原文，渲染 `{{target}}` 不再复制字符串
  （原实现常用语种也 `"中文".into()` 分配一次）。`render_template` 收
  `&str`，Cow 解引用即用。

## DEFAULT_TARGET 与 Templates（源码注释迁入）

- `DEFAULT_TARGET`：目标语言缺省值（`TaskOptions::target_lang` 文档：缺省中文）。
- `Templates`：一个 locale 的模板文件集——各 kind 的自包含任务书（说明 + 输出契约与示例都在文件内）+ 共享目标语言条款 + 共享输出纪律块 + 分类指令 + 修复轮纠错指令。
- `Locale::templates`：本 locale 的模板文件集（编译期嵌入，无 IO）。

## Role / MessageContent / ContentPart / ChatMessage（源码注释迁入）

- `Role`：OpenAI 兼容消息角色。`System` 系统指令、`User` 用户输入、`Assistant` 模型回复（修复轮带回上一次的坏回复）。
- `MessageContent`：消息正文的域形态，纯文本或多模态部件数组。**不携带 base64**——部件数组映成 OpenAI 兼容 content、图像字节编码成 data URL 都归传输层（`engine::llm` 的请求体组装），本枚举只搬原始载荷。`Text` 纯文本正文；`Parts` 多模态部件数组（图像任务的用户消息）。
- `ContentPart`：多模态内容部件，域形态只携带原始载荷（PNG 保持字节，不预编码）。`ImagePng` PNG 图像字节。
- `ChatMessage`：OpenAI 兼容消息的域形态，`role` + `content`（wire 形状由传输层组装）。构造子：`system`/`user`/`assistant` 纯文本正文，`user_parts` 多模态部件数组（如图像任务）；`assistant` 修复轮用它带回上一次的坏回复。

## PromptRegistry（源码注释迁入）

- Prompt 模板注册表：内置文本任务模板，无状态可按需构造；模板来自编译期嵌入的文件资源（见模块文档）。

## PromptRegistry::render（源码注释迁入）

- 渲染任务的完整 messages。模态约束已在 `Task::new` 判定，这里只按输入形状分派。模板语言取任务自带的 `options.prompt_locale`（缺省中文）——与模型、目标语言一样，一次任务只认触发时定下的那一份。
- 系统指令是模板渲染结果；用户消息按输入模态分派：文本任务只有原文（取材不携带模态提示，语言线索由模型从原文判断），图像任务是图像部件（编码归传输层）。
- 图像分支：模板只有 image_explain（模态闸门在构造时已保证图像输入只配 ImageExplain），这里直接按唯一合法的图像 kind 取模板。

## PromptRegistry::render_classify

- 渲染**分类请求**的 messages：系统指令来自 classify 模板（类别清单、判别规则与输出契约都在模板内——模板是类别集的唯一事实源），用户消息只有待分类原文。与任务渲染的分工：分类不走 `render`（那是任务的路径），输出契约见 `crate::schema::validate_classify`。

## PromptRegistry::render_repair

- 渲染**修复轮**的纠错指令（修复对话里最后一条 user 消息的正文）：违约固定措辞（`crate::schema::Violation`，只记类别不携带回复原文）作为 `{{reason}}` 回灌模板。

## repair_conversation（源码注释迁入）

- 修复轮对话：`[system][user 原文/图][assistant 上一次坏回复][user 纠错指令]`。首轮消息原样在前：图像任务的图像部件（`Arc` 字节）随 base 零拷贝重发，修复轮历史里首轮图片随完整对话走。（EmptyReply 无内容可纠，不经本函数——原样重发的决策在 `crate::repair::next_round`。）

## instruction_template（源码注释迁入）

- 指令模板：按 kind 取本 locale 对应文件（全 kind 全覆盖；`render` 只对文本输入调本函数——图像分支直接取 image_explain，非法模态组合在渲染前已被约束表拦截）。兜底 kind `TaskKind::General` 的落点就是 general 模板——分类判 `Fallback` 时任务走这一份。

## target_display（源码注释迁入）

- 目标语言显示名：缺省中文，且**显示名不得为空**——指令模板里 `{{target}}` 独占一行语义、周围全是静态文字，空值会让整条指令被当作可选行删掉，只剩契约。空名或纯空白名（`Lang::Other` 由配置文件手填得来，可能带空白）回落缺省。
- 语言名随 prompt locale——英文模板里写 "Chinese" 而不是「中文」，否则模型拿到的是半汉半英的指令。`Cow` 承载：常用语种与自填名都借自静态表或原配置，渲染不复制字符串。

## lang_display（源码注释迁入）

- 语言显示名：模板面向模型，用各 locale 的语言书写；`Lang::Other` 是用户自填的名字，原样透传。

## render_template / substitute_line / RenderedLine（源码注释迁入）

- `render_template`：`{{占位符}}` 渲染，逐行替换全部已声明占位符；**一行里的占位符全部替换为空**时整行（含行内的静态文字与换行）删除——「可选行」因此在模板里就是一行，代码不必为可选片段做条件拼接（提示行就是这么写的：标签在模板里，值为空时整行消失）。
- 未声明的占位符与未闭合的 `{{` 原样保留：模板打错字不会静默吞内容，由完整性测试（`just test`）抓出来。
- `RenderedLine`：单行渲染结果。`text` 替换后的文本；`all_empty` 该行有已声明占位符、且它们的值全是空串（该行是可选行），没有已声明占位符的行恒为 `false`——不带占位符的空行是模板的结构。
- `substitute_line`：单行替换，声明的占位符按值替换，未声明的原样保留（连同一处未闭合的 `{{`，其后内容照原样跟在后面）。

### 共享目标语言条款与语言修复模板（2026-10-28，输出语言体例门禁设计 §7）

- 五份任务模板的「目标语言与体例」节提为共享片段（设计建议的实现路径）：`prompts/{zh,en}/target_lang.md` 编译期嵌入，`render_task_system` 在任务书渲染后拼接——四条共同条款单点维护，五任务逐字一致由 `every_task_system_prompt_ends_with_the_shared_clause` 锁定。任务模板文件里不再有该节。
- 新模板 `repair_language.md`（中英两份，§7.6 原文 + 英译）：`{{violations}}` 单值注入行式清单（判定器渲染行，模板不管类别到动作的映射——映射在 violation.rs 单点维护）。
- 词卡字段规则例句与设计 §7.1 的偏差（protected 例句自相矛盾）取规范句「受保护的（protected）」，来龙去脉见 lang_gate_comment.md。
- 五份任务书按设计 §7 v2 全文替换：书证四段式（出处、双引号原文、括号译、按语）、句子短语级括注、代码围栏闭合与裸代码禁令、通用 markdown 全量、音标字符集约束。

## 模板重设计期一（2026-10-29，prompt-template-redesign-plan §3.1–§3.8、§9 期一）

- **五段骨架归一**：五份任务模板统一为「角色与任务 → 输出形状 → 字段规则 → 任务约定」四段（输出纪律块在拼装层垫底），标题层级统一为三级；general.md 原以一级标题开头、JSON Schema 用二级的层级漂移一并归一。代码的评价维度与图像的观察方法按 §3.1 归入任务约定段；词卡例证的槽位骨架按 §3.2 的定稿示例留在 `examples` 的形态要素里——§3.2 给的是近乎逐字的核心改写示例，槽位骨架与四要素在那里是一个整体，§3.1 的括注按概览读。
- **纪律块**：新增 `{zh,en}/discipline.md`，全部任务模板逐字共享（除 `{{target}}`）。§3.4 示例文本落盘时按 §3.8 与决策 12 落了两处合规点：zh 的裸 JSON 改全角括注（JSON）（JSON 子串仍在，端点要求不受影响）、`null`/`[]` 加反引号（代码字面量走既有反引号豁免，为期二收编铺路）。
- **拼装顺序**：`render_task_system` 在目标语言条款之后接纪律块（任务书 → 目标语言条款 → 纪律块），锁点测试随更名与改写（见 prompt_tests_comment.md）。
- **体例现代转写**：核心体例段整体删除，结构要求以直陈功能语汇并入字段规则——分层释义、举证与简评、异说辨于释义列于例证、「用现代语言写作」进任务约定；语源（词义的时间轴）进词卡与通用，典源（典故的时间轴）进词卡、通用与句译同语例证（决策 14 的适用面）。禁用标记清单（中英各十二条）落成单元测试，英文清单按前置试跑定稿采用（决策 13）。
- **翻译决策规则**（§3.7，决策 15 的落位）：译名判定表（含禁则）落词卡与句译 `note` 的语义要素；留种条款落词卡 `examples`——句译异语时例证恒空、无落点，不硬塞；译文尺度三行落句译 `note` 字段规则。
- **出口句式统一**：「无 X 时用 Y，不要猜测」——phonetic 用 `null`、examples 用 `[]`、code_language 用 `"unknown"`；词卡 `note` 补防杜撰出口（如实写未确证，不留空，`note` 仍是硬约束）。
- **参数定稿**（§3.5）：note 长度 120 字（英文 240 字符）落词卡、句译、代码、图像；通用 note 不设上限（模板不陈述上限即无上限）。examples「无把握时 1 条；至多 5 条」只落在原「一般 1–3 条」所在的词卡、句译、通用——代码与图像的例证数量本无上限，§3.5 该行的缘由列写明是那条弱措辞的判定化，不外溢。
- **措辞合规**（§3.8）：zh 区专有名词全角括注（JSON）（markdown），占位符 `{{target}}` 不加反引号，JSON 字面量与字段名走反引号豁免；「疏证」「书证」「按语」等训诂词簇换为现代编辑语汇（例证、引文、出处、简评）。词卡 note 的括注例句「受保护的（protected）」保留——它是形态示例，前置试跑已把它登记为期二收编时的待判点。
- **范围边界**：模板目录本期仍豁免语言规范门禁（收编在期二）；`repair.md`、`repair_language.md` 与格式修复轮措辞未动（期三）；classify.md 未动（期三注册表渲染）。en 词卡形态要素里的全角括注示例（original word）保留原文——它是英文区路由的已知残余，期二定路由时处置。
