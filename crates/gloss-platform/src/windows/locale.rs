//! 系统语言读取（Windows）：配置选“跟随系统”时的输入。
//!
//! 只读用户默认区域名（GetUserDefaultLocaleName）一项，例如
//! 简体中文（zh-Hans-CN）；进程内启动期读一次，系统语言变了需重启应用。
//! 读不到区域名即回落英文档位。

use gloss_core::model::Locale;
use windows::Win32::Globalization::GetUserDefaultLocaleName;
use windows::Win32::System::SystemServices::LOCALE_NAME_MAX_LENGTH;

pub fn system_locale() -> Locale {
    let mut buffer = [0u16; LOCALE_NAME_MAX_LENGTH as usize];
    // SAFETY: the buffer is a plain local array of `LOCALE_NAME_MAX_LENGTH`
    // UTF-16 units, exactly what the call asks for; the call only writes
    // into it and returns the written length.
    let written = unsafe { GetUserDefaultLocaleName(&mut buffer) };
    let name = (written > 0).then(|| {
        let units = &buffer[..written as usize];
        let length = units
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(units.len());
        String::from_utf16_lossy(&units[..length])
    });
    locale_for(name.as_deref())
}

fn locale_for(locale_name: Option<&str>) -> Locale {
    match locale_name {
        Some(name) if name.to_ascii_lowercase().starts_with("zh") => Locale::Zh,
        _ => Locale::En,
    }
}

#[cfg(test)]
mod tests {
    use super::{Locale, locale_for};

    #[test]
    fn locale_maps_the_default_locale_name() {
        assert_eq!(locale_for(Some("zh-Hans-CN")), Locale::Zh);
        assert_eq!(locale_for(Some("zh_CN")), Locale::Zh);
        assert_eq!(
            locale_for(Some("ZH-TW")),
            Locale::Zh,
            "locale names are case insensitive"
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
            "an unreadable locale name means the English locale"
        );
    }
}
