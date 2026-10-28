# schema.rs 决策记录（步骤 3：契约校验器）

## 契约三行同构表与校验分级（§3 的落地）

- 硬约束刻意只有两条：整段是合法 JSON **对象**、`note` 非空（trim 后）。其余（structured 缺失/非对象、变体内字段类型错、数组坏条目）全部按软约束就地回落——全字段严格校验会把重试率推高几个量级（方案 §1.1 的取舍，评审六轮收敛的结论，不要"顺手"收紧）。
- 严格裸 JSON、无围栏容错：围栏输出即 `NotJson`。`validate_task` 消费方是 finalize 的降级分支（本步起围栏不再是容错位）；`validate_classify` 本步只定义 + 单测，**尚未**接进 `classify.rs`（生产分类仍走 `parse_classify_reply` 的围栏容错）——分类重试与兜底换值归步骤 5（General kind）与步骤 6（删围栏容错），eval 分类轨本步也留在 `parse_classify_reply` 以与当步生产保持同源。步骤 5/7 接线时这里是第一个要改的点。
- `KindNotAllowed` 对任务路径不可产生（kind 由编排给定），它留在同一枚举里是因为分类轨共用这套违约词表；缺 kind / kind 非法 / 清单外统一归 `KindNotAllowed`——修复轮（步骤 5）的措辞只需要"清单"一个语义。
- `Violation` 用 `Display` 而非方法承载固定措辞：它要进日志字段（`reason = %violation`）与后续步骤 5 的 `repair.md` 占位符，`Display` 是这两处的自然管道。措辞是英文（日志门禁），与 i18n 无关。
- `empty_structured(kind)` 独立成 pub 函数：finalize 的降级分支与 eval 的 `TaskVerdict` 都需要"该 kind 空变体"，收敛单点。

## wire 形状与 serde 的关系（容易踩的坑）

`OutcomeStructured` 的 serde derive 是外部标签（`{"WordCard":{…}}`），**不是** wire 形状；wire 的 `structured` 对象由 `schema.rs` 手工解析。模型侧 JSON 不经 serde 枚举反序列化——`task.rs` 的 `degraded_defaults_to_false_on_the_wire` 测试因此用 serde 自身表示做往返，而不是 wire 示例行。缓存是进程内存、恒存类型化 `TaskOutcome`，无跨版本持久化（方案 §1.9，不需要 schema version）。

## 本步边界（步骤 3 做了什么、没做什么）

- 做了：校验器模块 + 三变体类型定稿 + 全部任务模板嵌套化（不只 `image_explain.md`，见 prompt.rs 记录）+ finalize 两分支 + 降级卡 + retry 守卫 + `code_hl::adopt` + eval 任务轨接同一校验器。
- 没做（后续步骤条目，勿顺手）：修复轮与 `CLASSIFY_REPAIRS`/`TASK_REPAIRS`（步骤 5）、`General` 兜底 kind 与 `CLASSIFY_FALLBACK` 换值（步骤 4）、classify.rs 围栏容错删除（步骤 6）、eval 夹具改嵌套与异常夹具、违约/修复轮次统计（步骤 7）。

## General 落 Plain（2026-10-12，契约校验与重试方案步骤 4）

- `empty_structured`/`structured_of` 的 Plain 臂并入 General：通用任务的疏证就是展开条目，与句译/图像同构（方案 §3 三行同构表第二行）。`validate_task` 的硬/软约束对 General 无任何特判——kind 只决定落哪个变体，不改变违约判定；违约矩阵与分类矩阵各多一格（General 作任务 kind 过全矩阵、作分类回复落 `KindNotAllowed`）。

## Violation 的 Ord 与 serde 派生（2026-10-12，契约校验与重试方案步骤 7）

- `Violation` 加 `PartialOrd/Ord` + `serde::Serialize/Deserialize`：评测报告的违约类别计数用 `BTreeMap<Violation, usize>` 承载——`Ord` 给稳定迭代序（报告与 JSON 键序确定，变体声明序即展示序），serde 让报告结构体直接以变体名为键序列化（`"NotJson": 2`），不必在 eval 侧转字符串丢类型。违约的固定措辞（`Display`）不变，serde 形态（变体名）只进评测报告工件，不进日志与修复指令。

## 模块头（三行同构示例，自文件头迁入）

```json
{"note":"…","structured":{"phonetic":"…或 null","examples":["…"]}}       // TranslateWord → WordCard
{"note":"…","structured":{"examples":["…"]}}                              // TranslateSentence、ImageExplain、General → Plain
{"note":"…","structured":{"examples":["…"],"code_language":"…或 null"}}  // ExplainCode → Code
```

## Violation

- 契约违约类别：措辞固定（`Display`），永不携带回复原文——类别进日志、降级产物与修复指令。serde 形态是变体名本身（评测报告的违约类别计数键）；`Ord` 供按类别的计数表稳定迭代。
- 变体：`EmptyReply` 回复为空（trim 后零长度）；`NotJson` 回复不是合法的 JSON 对象（含围栏输出——严格裸 JSON，无围栏容错）；`MissingNote` JSON 对象缺 `note` 或 `note` trim 后为空；`KindNotAllowed` 分类回复的 kind 不是 classify 模板的类别标识（含缺失）。

## ClassifyChoice

- 分类回复的判定：模型输出的原始类别与执行落点 kind。模板类别 `Fallback` 映射到 `TaskKind::General`（兜底类别的执行落点就是通用任务模板），其余三个类别与 kind 同名直映。
- `model_kind`：模型输出的原始类别标识（classify 模板四个类别之一）。
- `kind`：执行落点，`Fallback` 落 `TaskKind::General`，其余原样。

## validate_classify

- 校验分类回复：裸 JSON 对象携带 `kind`，取值为 classify 模板的四个类别标识之一（`TranslateWord` / `TranslateSentence` / `ExplainCode` / `Fallback`；`General` 不在其中——它是执行落点，不是模型可选类别）。返回 `ClassifyChoice`，失守给 `Violation`。

## validate_task

- 校验任务回复：`EmptyReply` / `NotJson` / `MissingNote` 之外的失守按软约束就地容错（见模块文档）。返回 `(note, structured)`；`structured` 缺失或非对象回落 `empty_structured`。

## empty_structured

- kind 的空疏证变体：软约束（`structured` 缺失或非对象）与降级产物的回落形态。

## structured_of

- 按 kind 解析 `structured` 对象的疏证字段（软约束：对象缺失或非对象整体回落空变体；变体内字段类型错当缺省、数组坏条目跳过）。

## text_field

- 取字符串字段；JSON null 与缺失同样返回 `None`（phonetic/code_language 允许缺省）。

## examples_of

- 解析字符串数组（best-effort）：非字符串的坏条目跳过、好条目保留；字段整体缺失或非数组回落空表。

## FormatDetail 与 format_detail（提示词模板重设计方案 §4.2，期三，2026-10-29）

- `Violation` 枚举与 `Display` 一字不动（方案 §6 决策 1：序列化契约、日志
  措辞、红线测试均已锁定）；定位细节走新增 `FormatDetail`，单元变体只装
  结构事实（围栏、截断、键缺失），序列化键为蛇形小写，与注册表细节键一字
  对齐。`Violation::key` 补注册表键（变体名）供违约级措辞行查表，日志措辞
  仍走 `Display`，两不干扰。
- `format_detail(violation, raw)` 是纯函数：围栏判定先行（去空白后以围栏
  记号开头即围栏，围栏内带什么原文都不随行——红线）；解析成功非对象落非
  对象，解析错误按输入耗尽（EOF）判截断，首字节语法错不判截断；注键缺失
  与注非字符串或空白分落两细节；空回复、类别越清单与契约合法输入恒无细节
  ——细分不改变硬约束数量与降级语义，只把同一违约的位置说清。
- `CLASSIFY_CATEGORY_NAMES`：分类模板四类别标识的单一事实源，与校验分支
  同处本模块；注册表一致性测试与类别段渲染按此顺序消费，类别集漂移由
  同源测试当场报。
