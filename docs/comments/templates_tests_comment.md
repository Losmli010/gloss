# templates.rs 测试记录

> 按符号锚定（不写行号，防漂移）。改到相关测试时同步更新本文件。
> 测试代码禁止注释（AGENTS.md「注释纪律」节），测试的验收意图与辅助契约记录在此；`// SAFETY:` 仍留在代码内。

## 模块的由来（2026-10-30，重构计划阶段 3 自 prompt.rs 随主题迁移）

- 模板主题的七条用例随 `templates.rs` 拆分迁入本文件，断言零改动，
  `bdd.md` 清单节随迁；用例的既有缘由随用例迁入本文件（原
  `prompt_tests_comment.md` 各节）。

## 占位符配对判据（契约校验步骤 3 评审，2026-10-11）

- `every_template_placeholder_is_declared` 的括号检查从「{{ 计数 == }}
  计数」改为「每个 {{ 之后存在 }}」（配对判据）：嵌套契约的模板示例 JSON
  以 `}}` 收尾，`}}` 计数天然可以多于 `{{`，计数相等从此是错误判据；未声明
  占位符的白名单检查不变。

## 模板重设计期一的锁点迁移（2026-10-29，prompt-template-redesign-plan §3、§8、§9 期一）

- 新增 `task_templates_avoid_forbidden_legacy_markers`：§3.6 禁用标记清单落
  成单元测试——中文十二条（框架模式加训诂词簇）对 zh 五份、英文十二条
  （传统框架短语，前置试跑定稿）对 en 五份，比对统一转小写。放行边界：
  界面印章（经、注、疏）与字段名是产品语言不在受检面；「时间轴」等新造
  现代语汇不在清单——禁的是行话回潮，不禁新造语汇。
- `every_template_mentions_json` 与 `every_template_placeholder_is_declared`
  扩到 `discipline.md`：纪律块同为模板资产，端点 JSON 字样要求与占位符
  白名单同样适用。

## 形态说明（2026-09-22，R1 模板文件化）

-

（
模
板
内
容
断
言
口
径
随
主
题
迁
移
，
本
节
其
余
用
例
留

`
p
r
o
m
p
t
.
r
s
`

测
试
记
录
。
）

- 文件内容的断言只取关键词（"translation assistant" / "词典助手" / 提示行标签），不做整段逐字比对：逐字比对会让每次改提示词都必须动 Rust 测试，与「模板文件改动不需要碰 Rust 代码」的验收直接冲突。

## 期三组合与红线（提示词模板重设计方案 §8，2026-10-29）

- every_template_mentions_json / every_template_placeholder_is_declared：
  模板清单缩至七份与八份（修复骨架迁入措辞注册表后模板占位符白名单只剩
  `target`，修复指令一侧随主题迁往 `prompt_repair.rs` 测试记录）。
