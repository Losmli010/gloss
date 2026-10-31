# ui/code_hl/mod.rs（adopt）决策记录

## adopt(verdict)：采纳门槛 + 归一化收敛于渲染（契约校验步骤 3，2026-10-11）

- `machine::accept_done` 原先在产物到达时算一次语言并缓存进视图字段
  （`OverlayView::Outcome.code_lang`）；本步改为渲染时调用 `adopt` 现算。
  纯函数成本可忽略（一次 trim + 一次字符扫描 + 一次查表），换来“视图不再
  缓存派生状态”——机器采纳结果与渲染所用语言在结构上不可能漂移（方案
  §1.8）。`machine::plausible_code_language` 随之删除，门槛逻辑原样搬入
  `adopt`（单个 ASCII 词元；`+`/`#`/`-`/`.` 是 c++/c#/objective-c 的合法
  形态）。
- 签名取 `Option<&str>`（调用点对 Code 变体的 `code_language.as_deref()`），
  返回 `Option<String>`（规范名）。非 Code 变体在调用点直接落 None，不进
  adopt——adopt 只回答“这个判定能不能上角标”，不做变体分发。
- 门槛的“为什么”随函数搬迁：散文与`…或 null`占位串（模型照抄示例）不可
  上角标；首尾空白先修剪再过门槛（不因空白误判成占位串）。单测三件套
  （采纳归一 / 散文拒绝 / 空白修剪）从原 machine 测试迁来并补齐别名面。

## 着色后端换 egui_extras + syntect（2026-10-19）

- 旧实现是单趟 regex + 六类规则集，30 余门语言的 keyword 表与规则特化
  占 `tokenize.rs` 近 1600 行（大半是数据与测试）。换 syntect 的 Sublime
  语法集，语言覆盖与嵌套字符串/注释的准确性都上来了；egui_extras 的
  `highlight_with` 直接产出 `LayoutJob`，与 `code_job` 的接口同形。
- 语法集用 two-face 的 `extra_newlines()`：syntect 默认集缺 Kotlin /
  Swift / Dart / Elixir / Julia / TOML / TypeScript；`syntect-fancy` 与
  syntect 的 fancy-regex 后端对齐，不引 onig。
- 色板仍是六类（`palette.rs`）。egui_extras 的 `CodeTheme` 字段私有、
  只暴露 7 个内置主题，没法注入自定义 tmTheme；唯一注入点是
  `SyntectSettings.ts`——它按 `CodeTheme` 的键（base16-mocha.dark /
  Solarized (light)）索引主题，所以把这两个键替换成 gloss 主题。
- 流式期 `code_language` 尚未到达、以及表外语言仍走通用兜底
  （`fallback.rs`，regex 只用于这一条）；否则 syntect 会整段灰掉，而旧
  实现至少给字符串/注释上色。
- 0.36.2 的 egui_extras 把 `FontStyle::ITALIC` 误当 UNDERLINE 判据，
  斜体注释会被加下划线；`code_job` 统一清 `underline` 绕开，颜色与斜体
  仍由主题决定。
- 未做启动预热：`SyntaxSet` 在首个代码卡首次调用时构造并缓存进
  `OnceLock`；若首帧延迟成为问题，再考虑后台构造 `SyntectSettings` +
  `highlight_with` 的稳定引用。

## 审查后修订（2026-10-19，PR #215）

- 语法集预热挪到启动路径的后台线程（`code_hl::warm_up`，由
  `app/handler.rs` 在 `06_ready` 前调用）：首次构造 debug 实测约 0.1s，
  不能落在首个代码卡的首帧。
- 语言别名从 match 拆成 `LANGUAGE_ALIASES` 表，与 `SYNTAX_NAMES` 由单测
  锁一致（每条别名都要落到可着色的规范名）；加语言只改一处、漏改会报。
- 补三条 scope：`keyword.operator.word`（and/or/not）与
  `variable.language`（self/this）→ Keyword，`entity.name.tag.yaml` → Type。
  这三类在旧的关键字表与数据格式特化里都有色，不补会掉成平文或换类。
- 斜体下划线的绕行从 `code_job` 上收到 `code_hl::highlight`：这是对
  egui_extras 缺陷的适配，归适配层，也让单测能直接断言。

## 第二轮审查修订（2026-10-19，PR #215）

- `keyword.operator.word` 只覆盖 JS 的 `new` 一类；Python/Ruby/Lua 的
  and/or/not 是 `keyword.operator.logical.<lang>`，已按语言点名补上。不泛化
  到 `keyword.operator.logical`——JS 的 `&&`/`||`/`!` 同族，旧实现不着色。
- `variable.language` 全体太宽（会把 Python `self` 与 shell 的 `$@/$#` 也
  染上），改为按语言/词点名：`rust`、`ruby`、`this`、`super`。Rust 的
  `&self` 参数位是 `variable.parameter.rust`，与普通参数同 scope，只能保持
  平文。
- 补回旧特化的三类：`constant.other.color` → Number（CSS `#fff`）、
  `entity.name.table` → Function（TOML 节头）、
  `meta.mapping.key.json string.quoted.double.json` → Type（JSON 键）。
- egui_extras 的主题匹配按“更具体的 scope 得分更高”取胜，与规则书写顺序
  无关；注释已按此改写。
- 测试按字节区间断言（syntect 会把一个 token 按 scope 拆成多段，如字符串的
  引号与内容），并覆盖 YAML/TOML/CSS/JSON/Python/Ruby/Rust 的具体类别。

## 第三轮审查收尾（2026-10-19，PR #215）

- `entity.name.table` 收窄为 `entity.name.table.toml`：当前只在 TOML 命中，
  但按语言限定后，将来加语法集不会误伤 SQL/其它数据格式。
- 补 `entity.name.tag.toml` → Type：TOML 的键与 YAML/JSON 对齐（旧实现
  TOML 键本就不着色，这里取一致性而非逐像素复刻）。
- 测试模块里的 `///` 已删：注释纪律对 `#[cfg(test)]` 与 `tests/` 一视同仁。
- 用例扩到明暗两档，并补 ruby/lua 的 and、js 的 this、json 字符串值、
  toml 键与三处平文负例（python self / bash `$@` / rust `&self`）。

## 符号注记（迁自代码内注释）

- `LANGUAGE_ALIASES`：别名 → 规范名。别名只影响角标与归一化，能否着色看
  `SYNTAX_NAMES`；两表的规范名由测试锁住一致。
- `normalize_language`：把语言名归一化为规范名——别名映射收拢，未知输入
  原样小写返回（角标如实显示），空白视为无语言。
- `adopt`：采纳产物的代码语言判定（渲染时现算）——修剪首尾空白后过词元
  门槛：语言标识应是单个 ASCII 词元（"rust"、"c++"、"objective-c" 这个
  形状，内部含空白或非 ASCII 字符的判定是散文或示例占位串）——再经归一化
  （别名/大小写）落规范名。判定缺失或不可采纳时为 `None`：无角标、按
  通用启发集着色，自由文本不上角标。
- `SYNTAX_NAMES`：规范语言名 → Sublime 语法名。syntect 按名字精确匹配
  （大小写敏感），这里用语法集的实名而不是扩展名，避免 `.m` 之类多语言
  共用扩展名的歧义。
- `syntect_token`：规范语言名对应的 Sublime 语法名；表外语言返回 `None`，
  由调用方落 `fallback_job` 的通用启发集——不猜语言。
- `highlight`：代码正文着色——syntect 分词 + egui_extras 出 `LayoutJob`；
  语言未定或表外语言返回 `None`，由绘制侧落 `fallback_job` 的通用启发集
  （不猜具体语言，只认跨语言稳定的字符串/注释/数字/类型/函数形）。
  语法名可解析但逐行解析失败时（fancy-regex 的回溯上限，概率极低），
  egui_extras 自己退回灰底纯文本且不返回错误——这里无从探测，接受该
  降级。逐段清 `underline`：egui_extras 0.36 把 `FontStyle::ITALIC` 误当
  UNDERLINE 判据，斜体注释会多出一条下划线；统一清掉，颜色与斜体仍由
  主题决定。样式统一阶段 2 起改收界面单元（`Ui`）：明暗判别经语义调色板
  的解析点取（`Palette.is_dark`），本模块不再触碰界面库的明暗态；
  测试经 `run_ui` 驱动（主题偏好由统一装入点的上下文携带）。
- 语法主题键（原 `DARK_THEME_KEY` / `LIGHT_THEME_KEY`）：样式统一阶段 2
  迁往 `style/palette.rs`（`SYNTAX_THEME_DARK`/`SYNTAX_THEME_LIGHT`）——
  配色真相（语法色板）与索引键同住一处；egui_extras 的 `CodeTheme::dark/light`
  仍按这两个键在 `SyntectSettings.ts` 里取主题，自定义色板只能顶替这两个内置
  条目（`CodeTheme` 字段私有，没有注入自定义 tmTheme 的公开入口）。
- `settings`：进程内唯一的语法集与主题集。语法集用 two-face 的 bat 补集
  （syntect 默认集缺 Kotlin/Swift/Dart/Elixir/Julia/TOML/TypeScript）。
  `highlight_with` 的缓存按这个引用的地址作键，必须取 `OnceLock` 里的
  同一份，不能逐帧新建。
- `warm_up`：后台预热语法集——首次构造要反序列化整套语法定义（debug 实测
  约 0.1s），落在首个代码卡的首帧会让弹窗出现卡顿。启动时调一次，首个
  代码卡通常已命中 `OnceLock`。
- `theme_set`：只保留 egui_extras 会取用的两个键，其余内置主题不装——省掉
  默认主题集的解析，也让“自定义色板顶替内置条目”这件事在类型上只有
  一处。
- `SCOPE_CLASSES`：scope 选择器 → 语法角色类别——只做“语法角色 → 类别”
  的映射，颜色仍由 `palette::Class` 单点决定。syntect 按匹配得分取最具体
  的一条，与规则的书写顺序无关。表内特化条目的缘由：
  - CSS 的 #fff 落 color.rgb-value，旧实现按 Number 着色。
  - and/or/not 在 Python/Ruby/Lua 里是 keyword.operator.logical.*，不能
    泛化到 logical（JS 的 &&/||/! 同族，旧实现不着色）。
  - .word 类是词形运算符（JS 的 new 等）。
  - self/this/super 在旧关键字表里是 Keyword。按语言点名，不收
    variable.language 全体：Python 的 self 与 shell 的 $@/$# 在旧实现里
    都不着色。Rust 的 &self 落在 variable.parameter，与普通参数同 scope，
    只能保持平文。
  - YAML 键旧实现算 Type；TOML 键旧实现不着色，这里一并对齐为 Type。
  - JSON 的键与值同为 string scope，只能靠父级 mapping 区分。
  - TOML 的 [section] 是 table 名，旧实现按 Function 着色。
- `gloss_theme`：把六类色板包成 syntect 主题（scope 映射表见
  `SCOPE_CLASSES`）。
- `theme_item`：一条主题规则。scope 全是静态字面量：解析失败只在开发期
  断言，release 落空选择器（该条规则不生效），不让静态表的笔误把绘制
  路径打崩。
- `syntect_color`：egui 的 Color32 是预乘 alpha；色板里的类别色都是不透明
  色，逐通道搬运即可。
