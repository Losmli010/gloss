# judge.rs（gloss-eval）决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。

## JUDGE_PROMPT / JUDGE_MAX_TOKENS

- `JUDGE_PROMPT`：judge rubric——输入 / 产出 / 参考答案三占位符。
- `JUDGE_MAX_TOKENS`：judge 回复的 token 上限——分数 + 一句话理由，比分类的 kind 标识宽裕（分类的 128 会截断 reason，丢掉人读的依据）。

## render_judge

- 渲染 judge 请求的 messages：系统指令 = 填好三段占位符的 rubric，用户消息 = 输入。

## parse_judge_reply

- 从 judge 回复解析 1–5 分：先认 `{"score": n}` JSON，退到行内 `SCORE: n`；越界或缺失返回 `None`（该条不计入均分）。
