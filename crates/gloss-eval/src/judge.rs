//! LLM judge 轨（opt-in）：按 rubric 请评审模型给 1–5 分。
//!
//! 只在 live 轨、`GLOSS_LIVE_JUDGE=1` 时运行——judge 是额外的一次模型
//! 调用，成本与不确定性都归入 opt-in 轨。回复解析只认分数（理由留给
//! 人读的报告，不参与任何门禁）。

use gloss_core::prompt::{ChatMessage, Role};

/// judge rubric：输入 / 产出 / 参考答案三占位符。
pub const JUDGE_PROMPT: &str = include_str!("../prompts/judge.md");

/// judge 回复的 token 上限：分数 + 一句话理由，比分类的 kind 标识宽裕
/// （分类的 128 会截断 reason，丢掉人读的依据）。
pub const JUDGE_MAX_TOKENS: u32 = 512;

/// 渲染 judge 请求的 messages：系统指令 = rubric，用户消息 = 三段内容。
pub fn render_judge(input: &str, output: &str, reference: &str) -> Vec<ChatMessage> {
    let system = JUDGE_PROMPT
        .replace("{{input}}", input)
        .replace("{{output}}", output)
        .replace("{{reference}}", reference);
    vec![
        ChatMessage {
            role: Role::System,
            content: system,
        },
        ChatMessage {
            role: Role::User,
            content: input.to_owned(),
        },
    ]
}

/// 从 judge 回复解析 1–5 分：先认 `{"score": n}` JSON，退到行内
/// `SCORE: n`；越界或缺失返回 `None`（该条不计入均分）。
pub fn parse_judge_reply(reply: &str) -> Option<u8> {
    if let Some(score) = serde_json::from_str::<serde_json::Value>(reply.trim())
        .ok()
        .and_then(|value| value.get("score").and_then(|v| v.as_u64()))
    {
        return valid_score(score);
    }
    for token in reply.split(['\n', ',']) {
        let Some(rest) = token.trim().strip_prefix("SCORE:") else {
            continue;
        };
        if let Ok(score) = rest.trim().parse::<u64>() {
            return valid_score(score);
        }
    }
    None
}

fn valid_score(score: u64) -> Option<u8> {
    u8::try_from(score)
        .ok()
        .filter(|score| (1..=5).contains(score))
}

#[cfg(test)]
mod tests {
    use super::{parse_judge_reply, render_judge};

    #[test]
    fn judge_prompt_carries_all_three_sections() {
        let messages = render_judge("输入", "产出", "参考");
        assert_eq!(messages.len(), 2);
        assert!(messages[0].content.contains("输入") && messages[0].content.contains("参考"));
        assert!(!messages[0].content.contains("{{"));
    }

    #[test]
    fn judge_reply_parsing_accepts_json_and_score_line() {
        assert_eq!(
            parse_judge_reply("{\"score\":4,\"reason\":\"ok\"}"),
            Some(4)
        );
        assert_eq!(parse_judge_reply("分析……\nSCORE: 3"), Some(3));
        assert_eq!(parse_judge_reply("{\"score\":0}"), None, "1..=5 only");
        assert_eq!(parse_judge_reply("{\"score\":6}"), None);
        assert_eq!(parse_judge_reply("没有任何分数"), None);
    }
}
