//! 系统语言读取：`Config::language` 为 `System` 时的唯一输入，prompt 模板
//! 与界面文案共用这一份结果。
//!
//! 只看 `NSLocale.preferredLanguages` 的首项（如 `"zh-Hans-CN"`）；进程内
//! 启动期读一次，系统语言变了需重启应用。

use gloss_core::model::Locale;
use objc2_foundation::NSLocale;

pub fn system_locale() -> Locale {
    let preferred = NSLocale::preferredLanguages();
    let first = preferred.to_vec().first().map(ToString::to_string);
    locale_for(first.as_deref())
}

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
