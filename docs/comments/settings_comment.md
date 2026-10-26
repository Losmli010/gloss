# ui/settings.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），代码内不留指向本文件的指针。
> 任务07（docs/14 设置窗 UI 设计，2026-09-22 实现）的取舍集中在此。

## 校验规则下沉 gloss-core（共源）

- Base URL 的结构性规则（https/无凭据/无 query·fragment）原本是引擎
  `resolve_endpoint` 的请求前检查；热键语法原本在 platform `parse_trigger`。
  设置页逐字段校验若另写一套就是两套口径（T6 卡明令避免）。两者都是纯
  逻辑，下沉 `gloss_core::config::validate_base_url` 与
  `gloss_core::hotkey::parse_trigger`，引擎/注册侧改为调用同一份；
  platform 退为「core 语法 → global-hotkey 类型」薄映射。
- 错误文案分层：core 错误类型 `Display` 是英文（进日志/引擎 GlossError，
  与既有引擎消息逐字一致），设置页就地提示是本地化文案（展示层映射
  `FieldError`，见下节）——「日志英文 / UI 文案随 locale」两条红线同时成立。

## 文案与校验分家：FieldError / SettingsNotice（任务 09，2026-09-22）

- 校验产出**类型化错误**（`FieldError`）、壳回写产出**类型化提示**
  （`SettingsNotice`），两者都在渲染帧才按当前 locale 取措辞。为什么不让
  `validate_draft` 直接吃 `&Text`：校验是纯逻辑（模块文档的第一句），把文案表
  塞进去会让「哪里错了」与「怎么措辞」在同一个函数里绞在一起；更要紧的是，
  设置窗自己就有语言下拉，文案若是校验的产物，语言切换就只能在保存后才生效。
- 连带的可测性收益：`invalid_draft_blocks_save_and_enters_the_error_state` 从
  「断言提示含 https」变成「断言 `Some(&FieldError::BaseUrlNotHttps)`」——断言
  从此与语言无关，换文案不会让测试红。
- 变体与文案表的 `gloss_settings_error_*` 键一一对应（`DuplicateHotkey{line}` 对
  `gloss_settings_error_duplicate_hotkey`），填入的 `{{line}}` / `{{trigger}}` 由 `BaseUrlError` →
  `FieldError` 与热键语法解析两处给出；`base_url_hint` 随之改名 `base_url_error`
  并改为返回类型（原返回 `&'static str`，是上一版「文案在码里」的残留）。
- 汇总行与就地提示共用同一张错表，只是取用方式不同（`len()` 填 `invalid_summary`、
  逐 key 取 `message()`）；`SettingsState::errors()` 因此不再需要文案表。

## validate_draft 与校验时机

- 三态：`validated=false`（初始，编辑中不标红——半成品 URL 不该被追着
  报错）→ 首次点保存且有错：置位 + 不落盘 + 汇总行 → 每帧实时复检、
  改对即清。`errors()` 每帧从草稿重算，不存陈旧错误。
- 「不可保存」用阻断而非禁用按钮：禁用掩盖原因（哪里错了看不见），且
  egui 禁用态灰化整行；可点击 + 全量反馈信息量更大。
- 汇总行与壳回写提示（保存失败等）共用 notice 位，语义可区分：汇总只
  属于校验；壳回写是运行时错误。两者同帧可并存。

## 跨字段不变量：默认任务必须处于启用状态（2026-09-23）

- 现象（owner 真机走查）：界面切英文、默认任务改成「代码解释」（Code
  explanation），随后划词与划代码都没有任何反应——没有浮层、没有失败卡，
  日志里也没有一行（拦下它的那条只在 debug 级别）。根因是那个 kind 同时
  被任务开关停用：`trigger_decision` 按开关放行，停用的 kind 对一切
  触发路径无响应（docs/06 的配置表、docs/14 的开关行提示都写明了这条
  语义），于是「默认任务」这个唯一的划词入口被永久静音。
- 为什么在设置页校验、而不是运行时兜底：运行时语义（停用 = 无响应）是
  设计定的，不该被静默改写；默认任务与开关是否自洽，是**配置层面**的
  问题，能表达用户意图的只有设置页。`validate_draft` 因此多一条跨字段
  规则：`!draft.is_kind_enabled(draft.selection_task_kind())` → 阻断保存。
- 为什么不是「选作默认任务就自动打开开关」：开关有独立语义（关掉某任务
  本身就是用户能做的决定，比如不想让它的热键抢组合键），自动翻转是隐藏
  副作用；阻断 + 就地提示把选择权交回用户——改默认任务，或启用该任务。
- 为什么错误键落在 `FieldKey::DefaultKind`（不是开关行）：用户改得动的
  是这一行；文案里点名「任务开关」指路（开关就在同一张卡片下方）。提示
  与说明提示同位（出错时替换），见 docs/14 §六。
- 三个用例钉住这条规则：`a_disabled_default_task_blocks_save` 走纯逻辑（错误键、
  改对即清、恢复上交 Save），`a_disabled_default_task_is_blocked_with_an_in_place_hint`
  走渲染（点保存不动作、错误文案进树且替换该行说明提示、启用后恢复），
  `switching_off_the_default_task_is_blocked_until_it_comes_back` 走**用户那条路**
  （扳掉默认任务所在开关 → 保存被拦 → 扳回 → 可保存），
  `the_default_task_rule_follows_the_selection_kind_fold` 钉手改的图像默认。
- 判定口径是 `draft.selection_task_kind()`（与运行时同一处收口），不是
  `draft.default_text_kind`：手改配置把默认任务写成图像 kind 时，划词实际跑的是
  收口后的 `TranslateWord`。**已知措辞粗糙（记录项）**：这种配置下可见的默认任务
  是「提取结果」，而错误提示说「默认任务已停用」——指路的开关确实是对的（收口后
  的 kind），但没点名是哪一个；要精确就得把 kind 带进 `FieldError` 并加占位符，
  目前判断不值得为手改场景加一条占位符词条。
- 代价：想「彻底关掉划词手势」的用户没法再用「停用默认任务」表达这件事
  （今天也没有这个开关），保存会被拦下并说明原因；真要「关掉手势」是
  独立设置项，未落地。

## Base URL 的占位符 vs 说明提示（owner 两轮定稿）

- 首轮设计给 Base URL 常显说明「OpenAI 兼容服务地址，须使用 https」；
  owner 二轮改为：说明前半句移入 `hint_text`（清空才见），https 约束的
  解释完全交给校验错误文案；API Key 不设说明（占位符已够）。

## 任务开关钮（switch_row）

- egui 无原生 switch：自绘 40×24 胶囊（rect_filled 半径 12）+ 20px 圆形
  滑钮，`Sense::click` 的 allocate_exact_size 驱动；ON=`accent(ui)`（朱砂
  明暗双档，2026-10 换装）、OFF= widgets.inactive.bg_fill。
- 可访问标签用 `widget_info` 设为「启用{任务名}」：与勾选框时代的树标签
  一致（kittest/读屏按它定位），也避免与默认任务下拉、模型行标签的裸
  任务名重名（get_by_label 唯一性）。

## kittest 点击用 click_accesskit（2026-09-22）

- 新卡片布局里，`get_by_label(..).click()`（按 accesskit 包围盒中心回放
  指针事件）对滚动区内控件的点击不生效，`click_accesskit()`（派发
  accesskit Click 动作）稳定。后者语义上正是读屏用户的点击路径，且不受
  滚动/可见性影响——测试一律用它。真实指针路径走 egui 命中测试，与本
  回放机制无关，未受影响。

## bottom_up 里直接 with_layout(RTL) 的布局陷阱

- 动作行最初直接 `with_layout(right_to_left)`：bottom_up 下该子区域会
  撑满剩余整高——按钮被垂直居中到窗口中部、滚动区被挤剩 64px（快照
  实证，根 ui 只剩上半）。修法：`allocate_ui_with_layout` 先分配固定
  行高（ACTION_ROW_HEIGHT 30）再做 RTL。老代码用 `ui.horizontal`（子高
  = 内容高）所以没踩过；卡片 Frame + RTL 的组合才暴露。

## Base URL 校验与引擎的口径对齐（2026-09-22 评审整改）

- scheme 大小写不敏感：url crate 会把 scheme 归一化为小写，main 上
  `HTTPS://host` 一直合法——共源校验用 `eq_ignore_ascii_case` 保持等价。
- 三处刻意收紧（原引擎放行、现校验拒绝，均为更安全方向）：空 userinfo
  （`https://@host` 的 `@`）、内嵌 tab/LF/CR（url 静默剥离）、任意位置的
  `<`/`>`；另补端口 16 位范围与 IPv6 括号成对检查（原 validator 是
  Url::parse 的真子集，这些输入曾「设置页放行、请求期才报错」）。
- 后果声明：手改配置里依赖上述宽松写法的存量用户，下次任务会得到明确
  的配置错误卡（fail closed，有可见兜底）。

## kittest 点击记录勘误

「测试一律用它」措辞过宽：实际只有开关钮与清除密钥两处（滚动区内）用
`click_accesskit`；保存/取消在动作行，指针回放点击稳定，仍走 `click()`。

## SettingsAction 装箱（任务 14，2026-09-24）

- `SettingsAction::Save` 的 `config` 装箱（`Box<Config>`）：`Config` 已长到 208 字节，
  而本枚举每帧都当返回值传，`large_enum_variant` 会被 clippy 拦下。装箱后枚举
  只剩指针，代价是「按下保存」多一次分配——人手触发的落盘路径，不在热路径上。
  取舍写在枚举的文档注释里（那里是读代码的人会看到的地方）。

## 可见性（2026-10-05）

- `draw` 从 `pub(crate)` 放开为 `pub`（当初随 `Text` 的 `pub(crate)` 一起收进去，
  见 i18n_comment.md 的修订记录）：`benches/clone.rs` 需要无头驱动设置窗一帧，
  计量逐帧分配（错误文案与更新区词条的逐帧 clone 都在这条路径上）。纯可见性，
  无行为变化。

## `toggle_switch`（剪贴板图片监听开关）

- 形式要求：设计文档 §5.2 修订后明确「设置页以开关（switch）形式呈现，
  不用勾选框（需求方明确要求）」；初版用 `egui::Checkbox::without_text`
  是按修订前文案（「设置页勾选项」）实现的，评审定为 HIGH 偏离后改自
  绘开关。
- 自绘而非等依赖：egui 0.36 **没有**内建 `Switch`（评审核实 docs.rs
  0.36.2/latest 的 Switch 页均 404、本地 registry 全量 grep 无 Switch
  类型；`WidgetType` 枚举亦无 switch 档），设计文档「0.36 已内建」前提
  不成立——这是要在设计文档勘误的点。按官方 demo 的 `toggle_switch`
  范式实现最小件：`allocate_exact_size` + `Sense::click()` 整块点击、
  `mark_changed` 上报变更、`WidgetInfo::selected` 上报无障碍。
- 无障碍角色仍是 `Role::CheckBox`：egui 0.36 的 `WidgetType → accesskit
  Role` 映射表（response.rs:943）没有 switch 档，`WidgetType::Checkbox`
  是唯一双态档（官方 demo 的 toggle_switch 也这样上报）；`selected`
  布尔经 response.rs:975 映射为 `Toggled::True/False`，辅助功能读到的
  「双态 + 当前态」语义完备，只是角色名不叫 switch——等 egui 上游补
  switch 档后可一行换。
- 视觉：轨 36×20、滑钮半径 7、内缩 3；on 用 `selection.bg_fill`（主题
  强调色）、off 用 `extreme_bg_color` + noninteractive 描边（浅色卡片
  上白轨需要边界）；滑钮 on 用常量白 `TOGGLE_KNOB_ON`、off 用
  `weak_text_color`。不引动画：设置窗只在交互帧重绘，静态两态足以；
  也避免快照对动画时间的依赖。
- 滑钮 on 色的改判（2026-10-12）：原 `strong_text_color` 在浅色主题是
  墨色，落在朱砂着轨上对比不足（选中态底色改品牌朱砂后暴露，见
  context_comment.md 同日节）；改白钮与保存主按钮（朱砂底白字）同观感，
  深色主题原值本就近白，两主题通用一个常量。
- 勘误（待设计文档维护者执行，文档在主检出的 docs/plans/，worktree
  纪律不碰主检出）：§5.2「用 egui 内建 `Switch`（0.36 已内建）」与 §8
  步骤 5「（`egui::Switch`，非勾选框）」中的内建前提不实，应改为「按
  官方 demo 范式自绘最小开关（0.36 无内建）」。

## 符号注记（迁自代码内注释）

- `FieldKey`：校验出错的字段——错误提示按字段定位到具体控件。
  `BaseUrl`：Base URL（结构性校验）；`Model`：模型 id（禁换行）。
- `FieldError`：校验错误的类型化形态——只记「哪里错了、错成什么样」，
  措辞归文案表。变体与 `gloss_settings_error_*` 的键一一对应（见
  `FieldError::message`），校验输出因此与语言无关——同一份草稿在任何
  locale 下得出同一张错误表。变体：`NewlineInModel` 模型名含换行；
  `BaseUrlEmpty` Base URL 为空；`BaseUrlNotHttps` Base URL 不是合法地址
  （语法不合法或非 https）；`BaseUrlCredentials` Base URL 内嵌账号密码；
  `BaseUrlQuery` Base URL 携带查询参数或锚点。
- `FieldError::message`：就地提示文案——借自传入的文案表，不复制。
- `SettingsState`：设置窗口的一个编辑会话——打开时以当前快照建草稿，
  保存/关闭由壳销毁。字段：
  - `draft`：编辑中的整份配置；「保存」时整体上交（整份快照语义）。
  - `api_key`：API key 输入框；只在保存时交给壳写 keychain，永不进
    `draft`。
  - `clear_key`：是否已标记「清除密钥」（保存时才真正删除；重新输入即
    撤销）。
  - `notice`：壳回写的提示（保存失败等）；文案在渲染帧落地。
  - `validated`：是否已进入校验态——首次点「保存」置位，此后每帧就地
    标注错误；置位前编辑不打扰。
- `KeyUpdate`：密钥的保存语义。删除不可逆，所以「清除密钥」不立即生效：
  它先标记，再随保存一起上交——否则点「取消」也留着一条删掉的密钥，
  与撤销语义矛盾。`Keep` 密钥保持不变；`Replace(String)` 用这个密钥覆盖
  （已 trim，非空）；`Clear` 删除当前 provider 的密钥。
- `SettingsAction`：设置窗口上交的动作——浮层只渲染，落盘/密钥/关窗都在
  壳。`Save` 的配置装箱：`Config` 是快照的大小头（每加一组字段就长一截），
  而这个枚举每帧都当返回值传，不装箱迟早把 `Idle`/`Close` 一起撑大——
  装箱后枚举本体只剩指针，代价是每次保存多一次分配，而保存是人手按下
  的路径，不在热路径上。`Idle` 普通编辑帧，无需壳动作；`Save` 保存整份
  配置并应用密钥变更；`Close` 关闭窗口并丢弃草稿；`Update(UpdateMsg)`
  更新区的用户动作（检查/下载/重启/取消/重试），壳转投更新模块。
- `SettingsNotice`：壳回写的用户提示——三种失败各有各的措辞，具体错因按
  `GlossError` 变体带进来（不预拼英文 `Display` 字符串——那是诊断文本，
  改它不该改界面）。
- `SettingsNotice::message`：提示文案——前缀说明场合，`{{detail}}` 填错因
  的本地化措辞。
- `open`：打开一个编辑会话——草稿取自当前快照（打开后的配置变更不跟读，
  保存即整份覆盖——与「单次任务内配置一致」同一取舍）。
- `SettingsState::report`：壳回写提示（保存失败等）；在下一次渲染帧按当前
  locale 展示。
- `SettingsState::draft`：当前草稿（壳只读：未保存变更提示等）。
- `SettingsState::notice`：最近一条壳回写的提示。
- `SettingsState::errors`：逐字段校验结果——每帧从草稿重算，不存陈旧错误。
  未进入校验态时返回空表（编辑中不标注）。
- `build_save`：「保存」的上交物——草稿先全量校验：有错则不落盘、就地
  标注并汇总统计；无错才走整份快照 + 密钥变更（清除标记优先，其次输入框
  内容，都为空则保持原密钥）。保存尝试即进入校验态并保持：壳侧落盘失败
  保留会话时，后续编辑仍实时复检，不会退出错误模式。模型名保存时 trim；
  空白折叠回出厂默认——输入框 hint 承诺「留空使用内置默认」，而下游
  （任务冻结 → LLM 层校验）对空串是明确失败，不接住会把用户锁进
  「清空 → 失败卡 → 回设置页」的死循环。
- `validate_draft`：逐字段校验（纯逻辑）——规则单点下沉 gloss-core，这里
  只做组合与 `FieldError` 映射（文案留给渲染帧）。模型名：保存时 trim，
  禁换行（粘贴事故防护）；空 = 用内置默认，合法。
- `base_url_error`：Base URL 校验错误 → 字段错误（规则与提示的分界在这里）。
- `draw`：画一帧设置窗口，返回本帧用户上交的动作。`text` 是当前 locale 的
  文案表（由壳按帧给），本函数不探测语言。自下而上布局：动作行钉在窗口
  底部（保存主按钮右对齐），提示在其上，其余全部区块进滚动区——内容再长
  也不会把「保存」推出视口。窗口内边距由 `WINDOW_PADDING` 统一给出；
  整幅先铺 `window_fill` 底色（设置窗不透明，清屏色不随主题，底色必须由
  egui 自己画，深浅主题切换才连同文字一起翻转）。bottom_up 会渗进子 Ui：
  滚动内容显式转回 top_down，区块才从顶部开始排列。
- `WINDOW_PADDING`：设置窗口内边距（窗口私有量，设计定稿 16）。
