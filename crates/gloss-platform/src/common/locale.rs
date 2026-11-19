//! 系统语言档位映射的平台中立共享层：语言名到界面语言档位的纯映射只写
//! 一份，两侧系统语言读取器（各自持有原语读取面）共用；本模块零目标条件
//! 编译、零外部函数接口（FFI）、零不安全块，不触任何系统接口。

use gloss_core::model::Locale;

pub(crate) fn locale_for(preferred: Option<&str>) -> Locale {
    match preferred {
        Some(code) if code.to_ascii_lowercase().starts_with("zh") => Locale::Zh,
        _ => Locale::En,
    }
}

#[cfg(test)]
mod tests {
    use gloss_core::model::Locale;

    use super::locale_for;

    #[test]
    fn locale_maps_language_names_to_locales() {
        assert_eq!(locale_for(Some("zh-Hans-CN")), Locale::Zh);
        assert_eq!(locale_for(Some("zh_CN")), Locale::Zh);
        assert_eq!(
            locale_for(Some("ZH-TW")),
            Locale::Zh,
            "language names are case insensitive"
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
            "an unreadable language name means the English locale"
        );
    }
}
