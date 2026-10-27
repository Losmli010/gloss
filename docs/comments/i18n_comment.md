# i18n.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关符号时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），代码内不留指向本文件的指针。
> 任务 09（docs/12 R7 UI 文案 i18n）的取舍集中在此。

## Text / 文件资源形态（与 prompt 同构）

- 为什么是「文件资源 + 编译期嵌入 + 类型化解析」，而不是代码里的常量表或
  `match locale`：与 R1 的 prompt 模板同构（`include_str!` + 解析），两处
  「文本资产」的维护形态一致；文案改动落在数据文件，与语言无关的渲染代码不用动。
- 为什么每个字段显式列出、且整体 `#[serde(deny_unknown_fields)]`：`#[serde(default)]`
  会把「键拼错」变成静默回落空串——界面出现空白而没有任何报错，正是 R7 卡要消掉的
  失败模式。显式字段让「少键」当场解析失败，`deny_unknown_fields` 拦「多键/拼错键」；
  两份文件的键集合一致性另由 `both_locale_files_declare_the_same_keys` 兜底。

## 键名 gloss_<模块>_<词条>（2026-09-22 owner 两轮定稿）

- 词条一律写成全限定名 `gloss_<模块>_<词条>`，**前缀直接落在每一行**，文件里不再有
  节头（`[gloss_<模块>]`）；用下划线而不是点号连接。整张表因此是一份扁平的键值列表，
  每行自描述：导给外部翻译平台不必再拼节名，diff 里也能直接看出词条归属。
- Rust 侧与文件逐字同名：字段就叫 `gloss_popup_brand` 这样的全名，**既没有 rename 层
  也没有嵌套结构体**，取用点同样是单层（`text.gloss_popup_brand`）。两侧是同一个名字的
  两种写法，改词条名要连着改 TOML 那一行；名字对不上即解析失败
  （`deny_unknown_fields` + 字段无缺省）。
- 定稿经两轮（同一日内）：先定为「节名带前缀 + `#[serde(rename)]` 对齐 + 嵌套结构体、
  取用 `text.settings.save`」，随后 owner 改为「两侧都用全名」，再改为「下划线连接」。
  被否方案的取舍记此备查：嵌套取用读起来短，代价是多一层 rename 对照与一处隐性耦合
  （改节名要同时改两处）；owner 取「文件与代码同名、零映射层」。
- 键名形态由 `entries_are_written_fully_qualified` 钉住（每行须以 `gloss_` 开头、
  键名不含 `.`），键集合的两侧一致性另由 `both_locale_files_declare_the_same_keys` 兜底。

## Text::get / 解析与常驻

- `OnceLock<[Text; 2]>` 首次调用解析两份文件并常驻：文件是编译期常量，之后零锁读取。
- 解析失败回落 `Text::default()` 并记 `error`：属「有痕迹的降级」。那条分支在正常构建里
  不可达（解析测试 + 键一致性测试钉住），留着是为了将来若改成运行时加载，不静默变空白。
- `pub(crate)` 而非 `pub`：`missing_docs = deny` 会要求公开面逐条写文档，而文案表的字段
  是数据不是 API；壳外（`tests/overlay.rs`）需要语言时传 `Locale`，不暴露表本身。
  （连带的可见性后果：`settings::draw` 因此收成 `pub(crate)`。）
  - **2026-10-05 修订**：`Text` 类型与 `get` / `for_error` / `for_error_detail` 放开为
    `pub`（字段仍 `pub(crate)`，数据面不外露），供 `benches/clone.rs` 无头驱动文案
    映射与设置/浮层帧的分配测量；`missing_docs` 的成本按约定补齐文档。原「不暴露
    表本身」的关切仍在——公开的只有三个取用方法，字段与解析仍内部。

## fill

- 占位符沿用 prompt 模板的 `{{name}}` 形态：两处模板的书写习惯一致，改文案的人只需记一套。
- 无对应实参的占位符**原样保留**：宁可让人在界面上看见「{{other}}」，也不要静默吞掉
  ——文案表里写错名字时立刻暴露，而不是变成空串。

## Text::for_error / for_error_detail

- 失败卡文案按 `GlossError` **变体**映射，不按英文 `Display` 反查（R7 卡明令）：`Display`
  是日志用的诊断文本（`config error: …`），措辞与界面无关，改它不该改界面；按它反查还会
  把英文整句变成界面契约。
- `for_error_detail` 专供**复合**提示（设置页的「保存失败：{{detail}}」）：前缀已交代场合，
  再拼一遍本地化整句会读成「保存失败：配置有误：…」——而「落盘失败」与「配置本身非法」
  是两回事。带诊断的变体（`Config` / `EngineResponse`）只出诊断原文，其余变体回落整句。
- 词条分两组覆盖这两类起因：`gloss_errors_*` 覆盖 `GlossError` 的全部 10 个变体，
  另加 `gloss_errors_acquire_channel` / `gloss_errors_inference_channel` 两条给**壳层通道类
  起因**（`FailureCause::AcquireChannel` / `TransportChannel`，不属 `GlossError`）。新增变体时
  `for_error`（无兜底臂）与 `Text` 的字段（TOML 少键即解析失败）**都编译/解析不过**，比测试
  更早拦住；`each_error_variant_maps_to_its_own_entry`
  另按变体比对词条身份，防的是两臂对调这类「值都对、落错了变体」的错。
- 两张表的**取值**层面另有三条兜底：`every_entry_is_translated_in_the_english_catalog`
  （除 `gloss_ui_language_en` 外逐条中英不同）、`placeholders_match_across_locales`
  （同名占位符两语言一一对应，防单边拼错）、`both_locale_files_declare_the_same_keys`
  （叶子键集合逐条一致，条数钉死为 76，防遍历退化）。

## 敏感信息防护**没有**词条（任务 14 修订，2026-09-24）

- 初版加了 12 条（确认卡 4 + 因由 4 + 隐私区 4），条数 76 → 88；修订时全数删除，
  条数回到 76。两道闸门现在都不出浮层、也都不可配置，因此没有任何面向用户的
  措辞：命中只进日志（英文诊断行，与界面文案表分属两套），设置页也没有这一
  区。占位符清单里同时去掉 `gloss_popup_guard_notice`——那份清单逐条列名，正是
  为了这种「删一条模板就没人在意」的时刻能被人看见。
- 之所以值得记一笔：这张表的条数是被 `both_locale_files_declare_the_same_keys`
  钉住的常量，删词条必须同步改它，否则测试会指向一个误导性的失败（「键集合
  不一致」而其实是条数写错）。

## 新键 gloss_errors_json_mode_unsupported（2026-10-11，契约校验与重试方案 §7）

- zh 用方案 §2 定稿短句「当前模型服务不支持 JSON 输出模式」：行动指引由失败卡的「打开设置」按钮承担（同因由同出口）；en 附一句换服务的指引，保持失败卡文案「说清现状 + 给出路」的分工。
- 条数钉住值随新键增至 83（`both_locale_files_declare_the_same_keys`，本步骤时点 82 → 83）。

## gloss_popup_output_abnormal（契约校验步骤 3，2026-10-11）

- 降级产物卡的标题词条。不复用失败卡文案（gloss_errors_*）：契约违约不是
  错误变体（没有 GlossError 携带它），语义是「输出异常」——模型话到了但
  不可用；措辞刻意与失败卡区分，用户重试的心智也不同（失败卡=没到，
  降级卡=到了但烂）。
- 只加了这一个键：gloss_popup_reannotating / gloss_errors_json_mode_
  unsupported 等 schema 方案的其余键分属步骤 2/5，不提前占位。

## gloss_popup_reannotating（2026-10-12，契约校验与重试方案 §6/§7）

- 为什么只加一个词条：方案定稿「加载动画复用、不加横幅不加计数」——修复轮的全部用户可见差异是页脚一个词；条数钉死测试（85）随之 +1，防遍历退化的机制照旧。
- zh「重新注解」/ en「Re-annotating」：与「正在注解/Annotating」同词根，语义是同一件事的第二轮，不引入新概念词（「重试」「修复」都会与失败卡的重试按钮混淆）。

## 发送前确认与复制钮四键（2026-10-27，方案落地）

- 新键四条：`gloss_popup_confirm_send`（注疏/Annotate）、
  `gloss_popup_confirm_cancel`（取消/Cancel）、`gloss_popup_copy`
  （复制注疏/Copy annotation）、`gloss_popup_copied`（已复制/Copied）。
  条数钉死测试随增至 88（`both_locale_files_declare_the_same_keys`）。
- 主按钮词条「注疏」：待确认卡把「送出去做什么」写进按钮——按钮就是
  动作文案，与卡片标题解耦（卡片没有标题，原文即主题）；en 用
  `Annotate` 与页脚生成指示（`Annotating`）同词根，不引入新概念词。
- `gloss_popup_copy` 带宾语（复制**注疏**）而 `gloss_popup_copied` 不带
  （已复制）：按钮文案要交代动作对象，反馈文案贴着图标出现、对象自明。

## 注疏改名（同日）

- 「正在注解/Annotating」→「正在注疏/Annotating」、「重新注解」→
  「重新注疏」（zh 侧措辞统一为经注疏语汇；en 不动）。缓存键是
  `'static` 标签本体，改名自动换键重排；含页脚的七份快照基线随录
  （沿革见 bdd.md）。

## fill 单参数一趟拼接（本地门禁第 2 轮，2026-10-13）

- `fill` 原实现对模板先整份复制、再用 `format!` 造占位符模式串、最后
  `replace` 出结果——每次报错文案填充 3-4 次分配。改为单参数（现有全部
  调用点）一趟字面量拼接：模式按**字面**匹配（命中失败只前进一个字节，
  与 `str::replace` 逐字节语义一致，相邻花括号等畸形模板不歧义），替换
  值不参与再扫描（两版一致）；多参数走原逐参数替换，行为不变。
- 等价性口径：可达输入 = 文案表模板（静态、经完整性测试钉住）× 任意
  detail 值（只作替换体）——两版逐字节一致；i18n 既有 fill 测试
  （多次出现/无占位符/未知占位符）不改一字全绿。

## fill 多参回退分支补测试（评审第 1 轮，2026-10-13）

- 补两个 L1 钉子：多参去重替换（`{{a}} 和 {{b}}`）与多参顺序替换的
  嵌套值语义（`[("a","{{b}}"),("b","乙")]` → `乙`——回退分支保持旧的
  逐参数 `str::replace` 语义，后参**重扫**前参插入值；单参快路则不重
  扫，两分支各自的契约均有测试钉住）。

## 符号注记（迁自代码内注释）

- `ZH`：中文文案（出厂 locale）。`EN`：英文文案。
- `Text`：一个 locale 的全部界面文案——一张扁平表，字段名就是 TOML 里的键。
  键一律是全限定名 `gloss_<模块>_<词条>`（下划线连接，见 `i18n/*.toml`）；
  字段与文件逐字同名，因此既没有 rename 层也没有嵌套结构体——文件与代码
  读到的是同一个名字。改词条名要连着改 TOML 里的那一行，名字对不上即解析
  失败。
- `Text::for_error`：失败卡文案——按 `GlossError` 变体映射，不按英文
  `Display` 反查——后者是日志用的诊断文本，措辞与它无关（改 `Display` 不该
  改界面）。静态词条借自常驻文案表（`Text::get` 返回 `&'static`，表与进程
  同寿），只有带占位符的变体才拼新串。
- `Text::for_error_detail`：错因细节——变体自带诊断文本的取它，其余整句
  照搬。供**复合**提示用（如设置页的「保存失败：{{detail}}」）：复合提示的
  前缀已经交代了场合，再拼一遍本地化整句会读成
  「保存失败：配置有误：…」——落盘失败与配置本身非法是两回事。
- `Text::get`：取某 locale 的文案表。首次调用时解析两份文件并常驻，之后
  零锁读取。
- `parse`：解析一份文案文件；失败回落空表并记 error（有痕迹的降级）。
- `fill`：填充 `{{name}}` 占位符——表里出现几次就替换几次。占位符模式按
  字面匹配，替换值不参与再扫描（与逐参数 `str::replace` 的语义一致）——
  单参数（当前全部调用点）一趟字面量拼接直接出串，模板与占位符模式
  不再各自复制一份；多参数维持逐参数替换（参数值互不嵌套）。
  - 非占位符的花括号：只前进一个字节，逐字面量比对不漏配。
  - 字面量 `{{name}}`：四花括号两两成对。
- 2026-10-28 新词条 `gloss_popup_output_unverified`（语言降级产物卡的泛化提示）：zh「结果未通过校对」/ en "Result not fully verified"，设计 §6 定稿文案。条目总数钉值 88 → 89。
