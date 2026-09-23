//! 系统语言读取：`Config::language` 的 `System` 态落定成 [`Locale`] 时的
//! 唯一输入，prompt 模板与界面文案共用这一份结果。
//!
//! macOS 把用户偏好语言列表放在 `NSLocale.preferredLanguages`（如
//! `["zh-Hans-CN", "en-US"]`），本模块只看首项，判据见 [`locale_for`]。
//! 进程内只读一次（启动期）：系统语言变了要重启应用。

use gloss_core::model::Locale;
use objc2_foundation::NSLocale;

/// 本机系统语言对应的语言环境。
pub fn system_locale() -> Locale {
    let preferred = NSLocale::preferredLanguages();
    let first = preferred.to_vec().first().map(ToString::to_string);
    locale_for(first.as_deref())
}

/// 首选语言标识 → 语言环境：`zh*` 归中文，其余（含拿不到偏好）归英文。
fn locale_for(preferred: Option<&str>) -> Locale {
    match preferred {
        Some(code) if code.to_ascii_lowercase().starts_with("zh") => Locale::Zh,
        _ => Locale::En,
    }
}

#[cfg(test)]
mod tests {
    use super::{Locale, locale_for};

    #[test]
    fn locale_maps_preferred_languages() {
        assert_eq!(locale_for(Some("zh-Hans-CN")), Locale::Zh);
        assert_eq!(locale_for(Some("zh_CN")), Locale::Zh);
        assert_eq!(
            locale_for(Some("ZH-TW")),
            Locale::Zh,
            "language tags are case insensitive"
        );
        assert_eq!(locale_for(Some("en-US")), Locale::En);
        assert_eq!(
            locale_for(Some("ja-JP")),
            Locale::En,
            "non-Chinese systems get the English locale"
        );
        assert_eq!(
            locale_for(None),
            Locale::En,
            "an unreadable preference means the English locale"
        );
    }
}
