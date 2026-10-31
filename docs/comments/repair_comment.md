# repair 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。

## RepairStage

- 修复轮所属的编排段：只决定留痕的固定措辞（日志一律英文）。
- `Task`：任务段，耗尽后把原始回复交给完成态解析（降级产物）。
- `Classify`：分类段，耗尽后落常量兜底 kind。

## RepairStage::warn_exhausted

- 预算耗尽的留痕：调用方随即按各自语义上交产物。

## RepairStage::warn_violated

- 一次违约的留痕：只记类别与轮次，无选区与回复原文（安全红线）。

## RepairLoop

- 一段的修复循环状态：所属段（留痕措辞）与预算（已用/上限）——两者总是同生共死，调用方持有它跑完本段的全部轮次。

## RepairLoop::spent

- 已消耗的修复轮数：进产物（`RunOutput::repairs` 等）。

## RepairLoop::next_round

- 违约后的下一轮对话：预算内留痕并返回修复对话；耗尽返回 `None`，由调用方按本段语义上交产物。
- EmptyReply 无内容可纠：原样重发底座，不拼 assistant 轮也不渲染纠错指令（部分端点拒收空 assistant content，而首轮请求本身已含完整契约）——`instruction` 只在真正要发修复轮时求值。

### 泛化与语言段（2026-10-28，输出语言体例门禁设计 §6）

- `next_round` 把违约参数从 `Violation` 放宽为 `impl Display`、回复参数改为 `Option<&str>`（None = 空回复原样重发）、指令改为零参 `FnOnce` 惰性渲染——三段（任务/分类/语言）共用一个骨架，差异收口在调用方三参。`warn_*` 同步放宽，语言段有自己的两句话术。
- `RepairStage::Lang` 的留痕话术：违约为“reply violates the output language rules”、耗尽为“delivering the reply degraded”——与设计“降级不拦截”对应；reason 用 `LangViolation::summary`（类别集合），不带回复原文。
