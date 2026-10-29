# config.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关符号时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），文件内不留指向本文件的指针。

## Language::resolve

- 为什么是三态偏好 → 二态渲染输入的纯映射，而不是让 `Locale` 也带 `System`：偏好要持久化（用户选了「跟随系统」，下次启动还得跟随），渲染输入必须具体（模板路径、界面文案表、缓存 key 都要求唯一答案）。三态到二态的落定发生一次，就在取值时。
- 为什么入参是系统语言而不是在 core 里探测：core 不得依赖平台栈（AGENTS.md 的依赖方向红线），系统语言只能由适配器读。入参形态还让这条映射可用纯逻辑测试覆盖（不需要真机语言设置）。
- 为什么不做「进程内缓存系统语言」：读一次的成本是启动期一次调用（见 gloss-platform 的 locale 模块），壳把结果存进 `GlossApp` 的字段后，这里每次触发只是传值，缓存没有收益。
- 方法名（任务 09 由 `prompt_locale` 改为 `resolve`）：返回值同时供 prompt 模板与 UI 文案表选表，叫 `prompt_locale` 已经名不副实。

## 防护字段的移除（任务 14 修订，2026-09-24）

- 初版曾有 `guard_enabled`（总开关）与 `guard_blocked_apps`（敏感应用名单）
  两个字段，连同设置页第五区「隐私」一起删除：判据改为 `gloss_core::guard`
  的内建常量（`SENSITIVE_APPS`）。理由是**安全不交由用户控制**——字段只要
  还在，即使设置页不画，手改配置文件仍是一条旁路出口；而这道闸门防的正是
  「用户没意识到自己选中的是凭据」，一个可被关掉的防护等于没有防护。
- 代价写在明处：误报没有出口（命中就是发不出去，只能改文本再划），因此
  判据只收高置信度模式，宁可漏报（见 guard_comment.md 的误报/漏报权衡）。
- 落盘兼容：旧配置里的这两个键成为未知字段，serde 默认忽略、不报错；下一次
  保存自然丢弃，不需要迁移代码（`#[serde(default)]` 只补缺失字段，不拒绝多余
  字段，与「手改配置缺字段按出厂补齐」是同一条宽容口径）。

## DEFAULT_MODEL（2026-10-10，剪贴板图片方案 §5.4）

- `DEFAULT_TEXT_MODEL` → `DEFAULT_MODEL = "deepseek-flash"`：单一多模态模型承接全部任务，文本/图像不分模型——`TaskOptions.model` 本就单值冻结、引擎按不透明 id 透传，无需 `image_model` 配置与回退链；常量名去掉 TEXT 以免与「文本专用」旧语义冲突。
- 已落盘用户配置不受影响：`#[serde(default)]` 只补缺失字段，已写 `model = "deepseek-chat"` 的配置文件保持原值（升级不悄悄替用户换模型）；仅缺字段的配置回落新出厂值。

## watch_clipboard_images（2026-10-10，剪贴板图片方案 §5.2/§5.8）

- 默认 false：自动触发（复制图片即弹卡）是固有打扰面，opt-in 才能把「行为
  变化」的选择权交给用户；`#[serde(default)]` 使已落盘的旧配置补齐该键即
  关，升级后剪贴板行为与旧版本完全一致。
- 运行时不经配置快照逐次回读，而是组装点启动时取一次快照初始化共享
  `AtomicBool` 开关位（事件线程的哨兵源持加载侧，设置页保存时置位热切换）：
  哨兵每 250ms 读一次开关位，走配置句柄快照则要么高频分配快照、要么引入
  「改配置必须同步通知哨兵」的第三条通知路径。代价是「手改配置文件 + 不经
  设置页」的热更新对哨兵不生效（重启后生效）——配置文件本就不是热更新面。
- **默认翻转为 true（2026-10-12，owner 决定）**。升级影响面：已显式保存过
  `false` 的配置保持关（serde 只补缺失字段，不覆盖已落盘值）；从关变开的
  只有从未落盘过该键的机器。触发面事实：纯文本复制不触发（哨兵复查 types
  才产出事件），能自动弹卡的只有图片复制与 ⌘⌃⇧4 截图。

## DEFAULT_BASE_URL

- 出厂默认 OpenAI 兼容端点：DeepSeek。客户端按 `{base_url}/chat/completions` 拼接，设置页可改。

## DEFAULT_MODEL（枚举注释补记）

- 出厂默认模型 id：`Config::model` 的出厂值——单一模型承接全部任务（分类与执行同一模型），文本与图像模态不分（引擎按不透明 id 透传）。设置页可改；App 在触发时冻结进任务选项。

## Theme

- 界面主题：跟随系统 / 固定浅色 / 固定深色。`System` 跟随系统外观（出厂默认），`Light` 固定浅色，`Dark` 固定深色。

## Language

- 界面语言：出厂跟随系统。落定成 `Locale`，供 prompt 模板（触发时解析、随任务冻结）与界面文案（渲染帧逐帧取表）共用。
- 变体：`System` 跟随系统语言（出厂默认）、`Zh` 简体中文、`En` English。

## CACHE_TTL_MAX_SECS

- 缓存有效期的合法上限（秒）：30 天。0 = 永不失效（合法，进程内缓存随退出清空）；上限只为拦手滑输入的天文数字。

## BaseUrlError

- Base URL 的结构性问题类别（`validate_base_url` 的失败面）。文案由展示层映射；`Display` 是引擎侧的英文消息，与既有引擎检查逐字一致。
- 变体：`Empty` trim 后为空；`Invalid` 缺 scheme/host 等结构性残缺，或含空白与控制字符；`NotHttps` scheme 不是 https；`EmbeddedCredentials` authority 段内嵌账号密码（会被 reqwest 抽成 Basic 认证）；`QueryOrFragment` 带 query 或 fragment（路径拼接会落错位置）。

## validate_base_url

- Base URL 的结构性校验（纯逻辑）：设置页逐字段校验与引擎请求前检查共用这一份规则，避免两套口径漂移。规则与既有引擎检查一致：非空、https、无内嵌凭据、无 query/fragment、结构可解析；尾斜杠宽容。
- 结构解析交给 `reqwest::Url`（引擎发出的就是同一个解析器）：authority、IPv6 字面量、端口范围与非法字符的判定与线上请求同源，本函数只补产品规则（https、拒绝凭据与 query/fragment，且把失败归到具体类别）。
- 空白/控制字符与 `<`/`>` 先行拒绝：URL 解析器会静默剥离制表/换行，并把尖括号百分号转义——这些输入不该被「归一化」成合法端点。
- scheme 先于解析判定：非 https 的合法 URL 也要落 `NotHttps`（设置页的文案按类别给指引），不能被解析错误吞成「结构非法」；scheme 大小写不敏感（`Url` 解析时归一化为小写）。
- authority 不能为空：`https:///v1` 会被解析器读成主机 `v1`（多余的斜杠被当成空 authority 跳过），这不是用户想要的端点。query/fragment 不是 authority 的一部分：`https://host?token=a@b` 里的 `@` 属于 query，不能按内嵌凭据归类（两类都拒绝，但设置页文案不同）。
- authority 里出现 `@` 一律拒绝（含空 userinfo）：端点不带账号段，否则 reqwest 会把它抽成 Basic 认证或留下歧义主机。
- 其余结构问题（缺 host、非法 IPv6、端口越界/非数字、非法字符）由与线上请求同一个解析器判定。

## ProviderKey

- 一个 provider 的密钥条目：`keychain_id` 是密钥在系统 keychain 里的条目标识，读取走 `ConfigStore::secret`，配置文件中永不出现密钥本体。
- `provider` 是 provider 名称（如 "deepseek"）；`keychain_id` 是 keychain 条目标识。

## factory_provider / FACTORY_PROVIDER / default_provider_keys

- `factory_provider`：出厂 provider 条目，密钥本体永远只在 keychain，这里只给条目标识。
- `FACTORY_PROVIDER`：出厂 provider 条目的静态承载，`resolved_provider` 要返回引用，兜底时给它。
- `default_provider_keys`：出厂默认 provider 表（目前只有一条）。

## Config

- 应用配置：持久化为 TOML（`FileConfigStore`），运行时以整份快照在各线程间共享（共享形态不携带密钥，见模块文档红线）。反序列化带 `#[serde(default)]`：手改配置缺字段时按出厂默认补齐，不允许半份配置带病运行；退役字段（历史版本写下的多余键）静默忽略，测试有钉子。
- 落点（改动本节时同步更新）：`target_lang` / `model` / `language` 已接线（触发时由状态机按快照冻结进任务选项）；`base_url` / `provider_keys` 已接线（引擎每请求解析端点、按条目直查 keychain）；`theme` 已接线（主题施加到两个 egui 上下文）；`cache_ttl_secs` 归 gloss-app 的缓存构造接线；`watch_clipboard_images` 已接线（组装点按启动快照初始化哨兵开关位，运行中经共享位热切换，不在触发时冻结）。都不在触发时冻结的只有 `theme` 与 `watch_clipboard_images`（渲染帧/事件线程读取）。
- 敏感信息防护不在此列：它不是配置项，判据内建在 `gloss_core::guard`。
- 字段：`base_url` OpenAI 兼容端点的基地址，客户端按 `{base_url}/chat/completions` 拼接（尾斜杠会被去掉），MVP 单端点，多 provider 路由出现时再把它移进 provider 条目，空串视为未配置（引擎明确报错，不猜端点）；`provider_keys` 各 provider 的 keychain 条目标识，密钥本体只在 keychain；`model` 全部任务共用的模型 id，分类与执行同一模型，触发时随任务选项冻结；`target_lang` 翻译类任务的默认目标语言；`cache_ttl_secs` 缓存条目存活时长（秒），0 = 永不失效，上限 `CACHE_TTL_MAX_SECS`；`theme` 界面主题；`language` 界面语言；`watch_clipboard_images` 剪贴板图片触发开关，开时事件线程的哨兵源观察系统剪贴板的新图片并自动弹出解读卡片，关时剪贴板行为与既有版本完全一致（哨兵不查询），默认开，运行中经共享开关位热切换，不要求重启。

## Config 的 Default 实现

- `cache_ttl_secs` 出厂 1 小时：与 gloss-app::cache 的出厂 TTL（1 小时）一致（同源校验测试在 app 侧，随 DEFAULT_TTL 钉住）。

## Config::active_provider

- 本任务实际使用的 provider 条目：MVP 单端点，取最后一条；`provider_keys` 为空时返回 `None`（纯查表）。

## Config::resolved_provider

- 引擎实际使用的 provider 条目：空 `provider_keys` 时回退出厂条目（只带 keychain 条目标识，不带密钥），因此**恒有值**——真正的失败面是「条目指向的密钥没设」（`EngineAuth`，UI 引导去设置页）。
- 兜底不只是「方便」：旧版本落盘的出厂值是空数组，而 `#[serde(default)]` 只补缺失字段——那批机器上落盘的 `provider_keys = []` 会一直留着，设置页落地前又没有改它的 UI。

## deserialize_target_lang（2026-10-30，语言规范统一阶段四 D5）

- `target_lang` 的字段级自定义反序列化：语言收敛后 `Lang` 只余 `Zh`/`En`，历史配置里的 `Ja`/`Ko`/`Fr`/自定义名会让整份解析失败——沿用部分解析先例（`#[serde(default)]` 补缺失字段），把「单字段值失效」也收进宽容口径：解析失败时警告留痕、回落缺省中文，其余字段不受牵连。不走隔离降级（那是整份文档结构性损坏的处置）。
- 日志不引值原文：与配置加载错误同口径（错误文本不回显配置内容），消息只说「退役或不支持的语言回落缺省」。线程角色记 `ui`——配置装载发生在启动期主线程。
- 为什么不做「读时迁移落盘」：装载路径保持只读（「先落盘再换快照」的不变量只在保存时启动），下一次设置页保存自然把回落值写回磁盘。
