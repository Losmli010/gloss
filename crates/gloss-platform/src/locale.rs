//! 系统语言读取：`Config::language` 的 `System` 态解析成 prompt 模板语言
//! （见 gloss-core 的 [`PromptLocale`]）时的唯一输入。
//!
//! macOS 把用户偏好语言列表放在 `NSLocale.preferredLanguages`（如
//! `["zh-Hans-CN", "en-US"]`）。判定只看首选项：`zh*` 归中文模板，其余
//! （含列表为空这种拿不到答案的情形）归英文模板——非中文用户读英文指令比
//! 读中文指令更接近其习惯，这条口径同时是 R5「纯英文 locale 下跳过 CJK
//! 字体加载」的判据。
//!
//! 进程内只读一次（启动期）：系统语言变了要重启应用，与「任务内配置一致」
//! 的无歧义口径保持一致。

use gloss_core::prompt::PromptLocale;
use objc2_foundation::NSLocale;

/// 本机系统语言对应的 prompt 模板语言。
pub fn system_prompt_locale() -> PromptLocale {
    let preferred = NSLocale::preferredLanguages();
    let first = preferred.to_vec().first().map(ToString::to_string);
    prompt_locale_for(first.as_deref())
}

/// 首选语言标识 → prompt 模板语言：`zh*` 归中文，其余归英文。
fn prompt_locale_for(preferred: Option<&str>) -> PromptLocale {
    match preferred {
        Some(code) if code.to_ascii_lowercase().starts_with("zh") => PromptLocale::Zh,
        _ => PromptLocale::En,
    }
}

#[cfg(test)]
mod tests {
    use super::{PromptLocale, prompt_locale_for};

    #[test]
    fn prompt_locale_maps_preferred_languages() {
        assert_eq!(prompt_locale_for(Some("zh-Hans-CN")), PromptLocale::Zh);
        assert_eq!(prompt_locale_for(Some("zh_CN")), PromptLocale::Zh);
        assert_eq!(
            prompt_locale_for(Some("ZH-TW")),
            PromptLocale::Zh,
            "language tags are case insensitive"
        );
        assert_eq!(prompt_locale_for(Some("en-US")), PromptLocale::En);
        assert_eq!(
            prompt_locale_for(Some("ja-JP")),
            PromptLocale::En,
            "non-Chinese systems read English prompts"
        );
        assert_eq!(
            prompt_locale_for(None),
            PromptLocale::En,
            "no preference means the English templates"
        );
    }
}
