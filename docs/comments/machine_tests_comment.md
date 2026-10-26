# machine.rs 测试记录

> 按符号锚定（不写行号，防漂移）。改到相关测试时同步更新本文件。
> 测试代码禁止注释（AGENTS.md「注释纪律」节），测试的验收意图与辅助契约记录在此。

## 测试行为速览（BDD）

已统一迁往 docs/tests/bdd.md（git 追踪，以测试代码为唯一事实源）；本文件只留缘由与形态说明。

## 辅助

- plain_outcome / text_input：构造产物与输入的夹具助手。
- trigger(machine, config)：按出厂系统语言与「干净场景」触发一次，隐去
  `trigger` 那两个与用例无关的入参（Locale 与 `SceneFacts`），让各用例只
  显式表达自己关心什么。
- dispatched(outcome)：把 `InputOutcome` 收成人话——期望下发却拿到别的变体时，
  断言失败信息直接打印那条 `InputOutcome`，不必再去猜「左右不是一个枚举」。
- blocked_by_app()：一份「前台应用在拦截名单内」的场景事实。单独抽出来是
  因为它在多处用到（划词被场景闸门拦下的两条路径），且前台应用的身份字段
  必须和内建名单里真的有一条对得上——写错就把测试变成永远放行。

## fixed_kinds_arrive_classified_while_the_auto_sentinel_waits

- 2026-10-02 锁定 `classified` 初始化语义（T3 根因修复）：热键固定 kind
  创建流式视图时即 `Some(kind)`（曾恒写 None，热键代码解释全程按宋体
  渲染的衬线闪现根因）；划词走向 Auto 哨兵，创建时 `None`、
  `accept_classified` 到达后精化（采纳条件由
  `classified_kind_updates_the_streaming_chip_only_once_current` 另行
  锁定）。语言断言顺带覆盖 `code_lang` 的内容探测兜底（`fn main`→rust，
  无 hint）。

## code_language_prefers_the_hint_and_rides_into_the_outcome

- 2026-10-02 锁定语言通路的两级来源与随行语义：`InputHint::CodeLanguage`
  归一化后优先于内容探测（py→python），`accept_done` 把 `code_lang` 与
  source 一并携带进 Outcome——完成态角标沿用流式判定，不因正文到达重探。

## 降级产物与重试守卫（契约校验步骤 3 评审，2026-10-11）

- `a_degraded_done_keeps_the_request_and_retry_redispatches_it`：retry()
  有副作用（取走请求原件并转移状态），断言「请求副本被保留」不能先探测再
  取用——测试体里一次 `retry()` 取走并直接断言重发形状（同代数、同
  input+options、新令牌、回流式视图），保留语义由「取得到」本身证明。
  同测试后半段用第二次探测提交正常产物：正常产物落 Show 即清
  active_request，`retry()` 返回 None——两种 Show（可重试的降级卡 / 不可
  重试的正常卡）在同一测试内对照。
