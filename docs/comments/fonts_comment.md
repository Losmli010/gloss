# fonts.rs 决策记录

> 按符号锚定（不写行号），改到相关代码时同步更新。本文件是「为什么」的落点：
> 行为与不变量写在代码注释里，决策、备选方案与实测数据记在这里。

## CJK_BYTES：为什么用进程级静态缓存 + 借用登记

- **问题**：两个 `egui::Context`（浮层、设置窗）各调一次 `install`，旧实现里每次都要
  重新向系统取一份 PingFang（77,871,956 字节）并各自持有。
- **epaint 的拷贝语义**（`epaint-0.36.2/src/text/fonts.rs` 的 `blob_from_font_data`）：
  `Cow::Owned` 走 `Arc::new(bytes)`——每个 `FontsImpl::new` 深拷贝整包；`Cow::Borrowed`
  只 `Arc::new(&'static [u8])`（存指针）。因此「借用」不是风格选择，而是两个上下文
  共用同一份字体内存的唯一形态：`FontData::from_owned` 会让每个上下文各自持一份。
- **为什么字节必须是 `'static`**：`FontData::from_static` 要求 `&'static [u8]`，所以字节
  由 static 的 `OnceLock<Vec<u8>>` 拥有（进程生命周期内不释放），不引入 `Box::leak`、
  也不触发 `mem::forget` 禁令。
- **失败也缓存**：`Some(None)` 同样进 `OnceLock`，第二个上下文不再重复查系统。代价是
  首调失败即整进程禁用 CJK 兜底（旧实现会重试第二次）；本机与 CI 上未观察到，若将来
  需要自愈，再加一个重置入口。
- **与 `docs/comments/app_mod_comment.md` 里「静态可变状态是隐藏依赖」的取舍相反**：此处
  static 只增不减、只读消费、无注入需求，且它是「进程内唯一一份」这一不变量的载体，
  换用参数传递反而要求调用方持有 `'static` 字节，得引入泄漏。

## copy_font_data 的 drop-再-try_unwrap

- font-kit 的 `Font` 持有 `FontData::Memory(Arc<Vec<u8>>)`（loaders/core_text.rs），
  `copy_font_data()` 返回该 Arc 的克隆。旧实现在 `font` 仍存活时 `try_unwrap`，refcount
  恒 ≥2，于是**每次都走** `(*arc).clone()`（旧注释「引用唯一时移出 Vec」不成立）。
- 现在先让 `font` 出作用域析构，再 `try_unwrap`，直接 move 出 Vec。`unwrap_or_else` 分支
  保留为降级路径并加 `debug!`：万一 font-kit 将来把该 Arc 存在别处，日志里能看到
  「CJK font bytes copied out」，避免 77.9MB 的静默回退。

## 面（face）选择：face index 必须恒为 0

- 实测结论：font-kit 在 `load()` 时把 `.ttc` **就地解包**成单面数据（blob 头是 `OTTO`
  而非 `ttcf`，18 张表、偏移仍指向原文件），因此交给 egui 的数据是单面字体，`index`
  必须是 0。
- **被实测否掉的尝试（2026-09-23）**：曾把 handle 的 `font_index` 一并交给 egui，想让
  CoreText 选中的面生效。真机一跑即崩：
  `Error parsing "gloss-cjk" TTF/OTF font file: Invalid index 5 for font collection`
  （`FontsImpl::new` → `skrifa::FontRef::from_index`）。已回退。
- **权重**：本机 CoreText 按家族名解析出的是 `PingFangSC-Medium`（usWeightClass 500），
  不是 `Properties` 里请求的 `Weight::NORMAL`（400）；旧注释「固定常规体（Regular）」
  因此不准确。是否要显式拿 Regular 面属于需要真机核对的视觉项，未纳入任务 10。

## 内存实测（2026-09-23，本机 dev 构建）

| 指标 | 改动前（main） | 改动后 |
| --- | --- | --- |
| 启动期 `located system CJK font` 行数 | 2 | 1 |
| 两次 `installed into egui` 间隔 | 3.3s | 10ms |
| 启动后 `vmmap --summary` footprint | 171.4M | 89.0M |
| 空闲约 10s 后（内核回收空闲页） | — | 23.2M |

- 单次加载本机耗时数秒（日志：`gpu ready` → `located` 5.7s）；加载期瞬时峰值（约
  237M）由 font-kit 的 `slurp_file` 与就地解包两份缓冲决定，与本次改动无关。
- 实测依据：`vmmap --summary <pid>` 的 Physical footprint（Activity Monitor 的「内存」列
  口径）；`ps -o rss` 会把共享库常驻页算进来（同进程 276M vs 23M），不适合做本项对照。

## 调研：运行时子集化与懒加载（任务 T7 做法②③）

- **运行时子集化：不采用。** 字形集合运行时不确定（选区文本与模型输出任意），重子集化
  还要重排与图集失效，成本高于整份字体。
- **懒加载：无收益。** 浮层首帧就要中文，延后只是把同样的阻塞挪到首次交互。
- **后续候选（表紧致化）**：单面数据里真正被引用的表只有 13.08MB（Python 解析 PingFang
  表目录实测，集合 77.87MB/36 面），把引用的表拷进新缓冲并重写目录可把这份内存再降约
  6×；纯 Rust、不涉字形语义，但需真机核对渲染一致后才好合入，建议单列任务。

## 宋楷命名字体族（2026-10-01，经注疏 demo 定稿）

- **新增两个命名字体族**：`gloss-songti`（经/疏/印章，候选 Songti SC →
  STSong → Noto Serif SC → SimSun）与 `gloss-kaiti`（注，候选 Kaiti SC →
  STKaiti → Kaiti TC → KaiTi）。命名字体族**不进后备链**——只有显式点名
  的 RichText/文本样式才用，其余文本的字形解析完全不受影响。
- **族必须恒注册**：epaint 在布局期对未绑定的字体族直接 panic
  （`fonts.rs:1025` "is not bound to any fonts"）。因此两条兜底链的末端
  都是 CJK 后备字节：系统无宋体 → 宋体族绑 PingFang；无楷体 → 先落宋体
  字节（与 demo `--kai` 栈以 Songti SC 收尾的回退链一致），再无则同上。
  排版降级为默认字体而不是崩。
- **本机实测（macOS 13.6）**：Kaiti.ttc 不随系统附带（旧版 macOS 才有），
  本机注区实际落宋体；Songti.ttc 64MB（黑/粗/常规/细四面 TTC），font-kit
  按家族名选面、就地解包单面——内存增量即整包 64MB（`select_best_match`
  请求 Weight::NORMAL，CoreText 实际交出的面未核对，与 PingFang 的
  Medium 现象同类）。
- **快照不受宿主字体影响**：kittest 的自建上下文把两族绑到内置
  Ubuntu-Light（见 popup.rs 测试模块 `bind_named_families`），字形保持
  tofu 约定，字号/布局照常锁进基线。
- **内存对照**：CJK 后备（PingFang 77.9MB）+ 宋体 64MB 为现状基线；若
  将来做「表紧致化」（见上文调研节），两份一起做收益更大。

## CJK 全缺时的降级契约补全（2026-10-01，PR #91 评审 H-1）

- **评审发现**：`definitions()` 曾对 `cjk_bytes()` 用 `?` 早退——宿主连
  PingFang/Hiragino/STHeiti 都没有时返回 `None`，`install` 跳过
  `set_fonts`，上下文停在出厂字体表，而宋楷命名字体族未被注册；本 PR 起
  浮层无条件按命名字族排版，画首帧即触发 epaint 的 "FontFamily is not
  bound to any fonts" panic。同一 PR 给「宋/楷缺席」补了恒注册兜底，却
  漏了兜底链更上游的「CJK 全缺」分支——修复不对称。
- **修法**：`definitions()` 改返回 `(FontDefinitions, bool)`（bool = CJK
  后备是否接上），不再提前返回 None；命名字族恒注册，兜底链延长为
  宋/楷系统字体 → 宋体字节 → CJK 后备字节 → **内置 Ubuntu-Light**，
  `register_named_or_builtin` 在无字节时把族绑到内置字形。排版逐级降级，
  永不因缺字体崩。
- **口径拆分**：`install` 的 cjk_fallback 布尔自此只描述「比例/等宽族
  是否接上系统 CJK」（日志与 `new_context` 测试的语义不变）；命名字族的
  绑定状态不再经它表达。
- **kittest 绑定与本机制的合流**：kittest 测试此前手工把宋楷族绑到
  Ubuntu-Light，与本兜底的最终一级完全同构（`install_kittest_fonts` 的
  语义因此与生产兜底一致，仅字节源不同）。

## 等宽命名字体族 gloss-mono（2026-10-02，音标豆腐修复）

- **新增第三个命名字体族** `gloss-mono`：词卡音标（T3 起代码面板同用）
  的排版字体。与宋楷同款模式——不进后备链，只有显式点名的文字才用；
  `FontFamily::Monospace` 全局链不动（设置窗输入框、注区 markdown 代码
  块零影响）。
- **候选顺序 = demo ui-monospace 栈的系统落地**：SF Mono → Menlo →
  Monaco → DejaVu Sans Mono → Consolas。macOS 三代系统等宽在前（SF Mono
  是新系统自带、Menlo 兜底、Monaco 兜旧），后两项覆盖 Linux/Windows
  常见等宽；命中第一个可用者即止（`load_family_bytes` 既有语义）。
- **绑定规则与宋楷的不同点**：族绑定 = 系统等宽 + CJK 后备字节——等宽
  系统字体不收 CJK 字形，链条尾部接上 `gloss-cjk` 才有「代码内中文注释
  可读」；宋楷是单字节绑定。**系统等宽全缺席时不从 CJK 降级**（宋楷会
  落到 CJK 字节）——比例字形的等宽族让代码失去对齐，宁可直接绑内置
  Ubuntu-Light；族仍恒注册（epaint 对未绑定族 panic）。
- **音标豆腐的根因与实测**（CoreText 逐字体查 cmap，18 个常用 IPA
  字符）：音标原走 `FontFamily::Monospace`，解析链 Hack（内置）→
  Ubuntu-Light（内置）→ 两个 emoji 字体 → gloss-cjk（PingFang SC）。
  内置 Hack 缺 14/18（ɡ ɒ ə ɛ ɔ æ ɑ ʃ ʒ ʌ ʊ ɪ ˈ ˌ ː 等）、内置
  Ubuntu-Light 缺 13/18、PingFang SC 缺最常用的 6 个（ɒ ʒ ʌ ˈ ˌ ː）——
  链上无一命中即豆腐，`/ɡlɒs/` 必然中招。系统 Menlo 实测 0 缺失（demo
  的 `.w-phon`/`pre.code` 用 ui-monospace 栈，macOS 上即落 Menlo），换
  族即修。
- **快照约定**：kittest 自建上下文把 gloss-mono 与宋楷一并绑到内置
  Ubuntu-Light（`install_kittest_fonts`），基线锁排版不锁字形；真机
  字形由 `just run` 走查。

## CoreText 直取替代 font-kit（2026-10-09，PR #146，启动 memmove 塔）

- **为什么绕开 font-kit 字节路径**：font-kit 的 CoreText source 在家族解析里对 ttc 按候选
  序号逐面 `Font::from_bytes`——每面一次整包 clone+unpack（就地解包成单面 sfnt），苹方
  78MB/宋体 67MB 的集合在匹配循环与 `handle.load()` 里产生 GB 级 memmove；启动火焰图
  （run 37856233794，1506 样本）fonts 线程 64.8%、`OnceLock::initialize` 下 1444 个
  memmove 样本即此。
- **为什么 CoreText 家族集合 + 整文件 `fs::read`**：`create_for_family` 只做描述符查询
  不碰字节；选中常规面后按 `font_path` 读一次文件，ttc 整体登记（`FontData.index`），
  egui 侧 skrifa 按序号取面——每族只付一次文件读的成本。
- **为什么按 CoreText 文件枚举定位面序号并用 name 表复核**：
  `CTFontManagerCreateFontDescriptorsFromURL` 的位次与文件内 face 序号的对齐是经验事实
  （本机 PingFang/Songti/Menlo 实测一致），不是契约；复核（该序号面的 name 表交出同一
  postscript 名）失败即放弃该候选，宁可降级也不错登记面。
- **为什么只认文件绝对偏移（同日评审修正）**：OpenType 规范规定 TTC 表偏移从文件头量起，
  Apple 系统 ttc 全部如此（本机 20+ 个实测），egui 侧 skrifa 也只支持绝对。初版两种约定
  都试，评审指出面相对文件即便复核通过 skrifa 也解不出表（epaint 侧 `FontCell::try_new`
  失败即 panic），且相对优先在绝对文件上有实测误拒例（Songti 目录 1 的 face+12000 弱
  通过头部检查但解析出垃圾名）——收敛为单约定。
- **常规面选择**：家族描述符里先比风格修正位（粗/斜/宽/窄）计数、再比权重离 0 的距离，
  与 font-kit 时代 Weight NORMAL + Style NORMAL 的取向一致；实测各候选族均选中 Regular，
  无 Regular 的家族按最近权重取。
- **取舍**：旧路径常驻单面 sfnt 字节，新路径整包 ttc 常驻（苹方 78MB + 宋体 67MB +
  Menlo 2MB）——启动 memmove 塔消除优先；日后若要收敛 RSS 可评估 mmap（注意「长驻进程
  不泄漏内存」门禁的落点）——已于 2026-10-13 落地，见下节。
- **依赖**：font-kit 移除；core-text 20.1 + core-foundation 0.9 本是 font-kit 的传递
  依赖，转正为直接依赖（特性按仓规收窄，core-foundation 留 link）。

## 两处环境性回归与收口（2026-10-09 晚，跟进 PR #146）

- **CI runner 上字体测试挂十几分钟的根因**：`CTFontManagerCreateFontDescriptorsFromURL`
  在无窗口服务的 CI runner（fontd 病态）上单次要 10-16 分钟（Menlo 16 分钟、
  PingFang+Songti 再 33 分钟），#146 的 Test job 因此 2 分钟变 51 分钟。修复：面序号改为
  纯文件解析（ttc 头部面数 + 逐面 name 表找 postscript 名），序号完全由文件自身回答，
  CoreText 枚举与「枚举位次=文件序号」的经验假设一并删除；`file_face_descriptors` 与
  unsafe 块随之退场，face_index 复杂度微秒级。
- **启动弹「确认下载字体」框的根因**：家族查询用了
  `CTFontDescriptorCreateMatchingFontDescriptors`（core-text 的 `create_for_family`），
  匹配会命中系统的「可下载字体」目录——查询未随机型的楷体（Kaiti SC 等）时 macOS 主动
  弹字体下载确认框。font-kit 时代走 `CTFontCollectionCreateWithFontDescriptors`（只扫
  已安装字体）从不弹框。修复：家族查询改回集合形状（字体族描述符 →
  `new_from_descriptors` → `get_descriptors`，与 font-kit 同款），缺席家族自然耗尽候选
  走既有降级，不触碰下载目录。
- **教训**：CoreText 的两条查询路径语义差异极大（installed-only 集合 vs 含可下载目录的
  匹配），换 API 不只是换调用方式；且任何 CoreText 新调用都要在无窗口服务环境下验证。

## 字节源改 mmap 文件背页（2026-10-13，P0-1）

- **动机（性能计划 P0 内存卡）**：本机实测空闲进程 footprint 167 MB 里 **147 MB 是
  MALLOC_LARGE 脏页**，来源是 `fs::read` 把字体文件整包拷进 `Vec<u8>` 常驻（宋体
  66.9 MB + 苹方/冬青 26-80 MB + 楷体/等宽）；非字体部分仅 ~20 MB。改 mmap 文件背页
  后只有实际光栅化触碰的字形表页驻留、可被内核随时回收，footprint 只计脏页。
- **背衬形态 = `FontBacking` 枚举（Mapped/Copied）而非 `Box::leak`**：`FamilyFont`
  本就常驻 static `OnceLock`，`&'static FamilyFont` 已满足 egui `FontData` 的 `'static`
  借用（`Cow::Borrowed` 指向背衬切片的链条不变）——`Box::leak` 语义重复，且主动泄漏
  贴近「长驻进程禁 `mem::forget`」门禁的落点，能不碰就不碰；枚举实现
  `Deref<Target = [u8]>` 把「切片指向映射页还是回退拷贝」收敛到一处，登记端
  （`insert_face`）与既有断言（指针比对）不感知形态。
- **unsafe 落点**：memmap2 0.9 的 `Mmap::map` 是 unsafe（被映射文件被并发修改/截断时
  读切片可越界），SAFETY 不变量写死「系统字体文件由系统管理、本进程从不写入、进程
  生命期内不被截断」；映射只读，无写路径。
- **失败降级**：映射失败（空文件 mmap 直接 EINVAL、打开/系统调用失败）留 warn 后回退
  `fs::read` 整包拷贝——拷贝是兼容路径，保启动不阻塞；两条路都失败沿用既有「跳过该
  候选继续探下一个」降级（带 family/path 痕迹）。
- **渲染不变**：登记切片与整包拷贝逐字节相同，字形解析（skrifa 按序号取面）不受字节
  背衬影响；L2 快照走 `install_kittest_fonts` 绑内置 Ubuntu-Light，宿主字体字节源
  本就不进快照，基线零变更。
- **验证口径**：验收以流水线趋势点 `app.footprint_idle_kb` ≤ 50 MB 为准（计划 P0-1）；
  本卡改动只换字节来源，不改查找/选面/序号解析逻辑。

## 符号注记（迁自各符号文档注释）

- `FONT_NAME`：CJK 字体在 egui 字体表里登记的名字。
- `FONT_SERIF_NAME`：宋体命名字体族——经/疏正文的排版字体（demo 的 --serif 栈）。
- `FONT_KAITI_NAME`：楷体命名字体族——注正文的排版字体（demo 的 --kai 栈）。
- `FONT_MONO_NAME`：等宽命名字体族——词卡音标的排版字体（demo 的 ui-monospace
  栈的落地，覆盖内置等宽缺的 IPA 音标字形）。
- `zhu_family`：注正文的字体族。`serif_family`：经/疏正文的字体族。
  `mono_family`：音标的等宽字体族。
- `FontBacking`：字体文件字节的进程内背衬——首选 mmap 文件背页，egui 登记的
  借用切片直接指回文件页，字节不进进程私有堆，只有字形表被实际触碰的页驻留
  物理内存；映射失败回退 `fs::read` 整包拷贝。两种形态对登记等价，消费面只有
  只读切片（`Deref` 到 `[u8]`）。
- `FamilyFont`：一个系统字族的解析结果——整份字体文件的字节背衬与目标面在
  文件内的序号。字节首选 mmap 文件背页、映射失败回退整包拷贝，进程内常驻
  （缓存于各 `OnceLock`）；ttc 集合整体交给 egui，按序号取面。
  `FamilyFont::from_bytes`：整包字节构造——无文件来源的合成字体（测试）用的
  等价入口；生产路径的字节背衬由 `imp::load_backing` 直接给出。
- `insert_face`：把一张系统字面登记进字体表，字节以借用形态挂到 `name` 上。
  签名要求 `'static` 字体——登记的字节背衬必须进程内常驻（生产路径出自
  `OnceLock` 缓存），借用的 `Cow` 才不悬垂；切片指向映射页还是回退拷贝由背衬
  收敛。
- `CJK_FONT`：系统 CJK 字体，进程内唯一一份；取用失败同样缓存，不重复查找
  系统。`SERIF_FONT`/`KAITI_FONT`/`MONO_FONT`：宋体/楷体/等宽，各自进程内
  唯一一份（形态与 `CJK_FONT` 相同）。
- `CJK_FAMILIES`：苹方随系统自带，历代候选按可用性排序。
- `SERIF_FAMILIES`：宋体候选（demo --serif 栈的前四项，serif 泛型族不参与
  精确匹配）。`KAITI_FAMILIES`：楷体候选（demo --kai 栈的实名项）。
- `MONO_FAMILIES`：等宽候选（demo ui-monospace 栈的系统落地：macOS 的三代
  系统等宽在前，后两项覆盖 Linux/Windows 常见等宽）。
- `BUILTIN_FALLBACK_FONT`：egui 内置字形的登记名（default_fonts 特性自带）——
  宋楷两族在系统与 CJK 后备双双缺席时的族内兜底，排版降级为默认字形而不是
  panic。
- `definitions`：带 CJK 后备与宋楷字族的字体定义；bool 表示 CJK 后备是否接上
  （缺席时此处出 warn，比例/等宽族中文不可读）。宋楷两族**无条件注册**——
  epaint 对未绑定的字体族直接 panic——兜底链为 宋/楷系统字体 → 宋体
  字节 → CJK 后备字节 → 内置字形，排版逐级降级而不是崩。交给哪个上下文
  由 `super::context` 的统一装入点决定。
- `builtin_definitions`：仅内置字形的字体定义——命名字体族恒绑定（绑到内置
  字形，epaint 对未绑定族直接 panic），系统字体一个不装。启动首帧的快路径：
  系统字体装载是秒级的，由 `context::apply_system_fonts` 延迟补装。
- `system_fonts_ready`：系统字体是否已装载过（字节级缓存就位）。统一装入点
  据此选择完整定义与内置快路径：就位前任何上下文施加都走快路径，系统字体
  只由延迟装载一次性补齐。
- `cjk_font` / `mono_font`：系统 CJK 字体/系统等宽字体；首次调用向系统取，
  之后命中缓存。
- `append_fallback`：把 `FONT_NAME` 以后备（最低优先级）追加进比例与等宽两个
  字体族——内置字体先挑，挑不到的字形才轮到系统字体。字节已由 `cjk_font`
  登记进字体表，这里只补族链。调用前提：`definitions` 里 `FONT_NAME`
  已注册（`definitions` 的装入顺序保证）。
- `register_named_or_builtin`：把一个系统字面注册成命名字体族；缺失时把族绑
  到内置字形上——族必须恒存在（epaint 对未绑定的族直接 panic），字形逐级
  降级。
- `register_mono_family`：等宽命名字体族——族绑定 = 系统等宽 + CJK 后备字节
  （等宽系统字体不收 CJK 字形，链条尾部接上才有代码内中文注释）；系统等宽
  缺席时绑内置字形，不从 CJK 降级——比例字形的等宽族会让代码失去对齐，
  宁可用内置等宽。`cjk` 为 `Some` 时 `FONT_NAME` 必已注册（`definitions` 的
  装入顺序保证：后备追加先于本调用）。
