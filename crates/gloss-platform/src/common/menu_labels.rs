//! 应用菜单词条表的平台中立共享层：中英两份静态词条与按系统语言的档位
//! 查取只写一份，两侧应用菜单（系统（macOS）坞菜单与系统（Windows）托盘
//! 菜单）按各自装配的词条取用；词条属平台层启动期文案，早于应用文案表
//! 装入点，随放行标记显式豁免文案门禁。本模块零目标条件编译、零外部函数
//! 接口（FFI）、零不安全块。

use gloss_core::model::Locale;

pub struct MenuLabels {
    pub about: &'static str,
    pub settings: &'static str,
    pub services: &'static str,
    pub hide: &'static str,
    pub hide_others: &'static str,
    pub show_all: &'static str,
    pub quit: &'static str,
}

const ZH: MenuLabels = MenuLabels {
    about: "关于 Gloss",     // i18n:allow 平台层启动期菜单词条，文案表装入点之前
    settings: "设置",        // i18n:allow 平台层启动期菜单词条，文案表装入点之前
    services: "服务",        // i18n:allow 平台层启动期菜单词条，文案表装入点之前
    hide: "隐藏 Gloss",      // i18n:allow 平台层启动期菜单词条，文案表装入点之前
    hide_others: "隐藏其他", // i18n:allow 平台层启动期菜单词条，文案表装入点之前
    show_all: "显示全部",    // i18n:allow 平台层启动期菜单词条，文案表装入点之前
    quit: "退出 Gloss",      // i18n:allow 平台层启动期菜单词条，文案表装入点之前
};

const EN: MenuLabels = MenuLabels {
    about: "About Gloss",
    settings: "Settings", // i18n:allow platform-layer startup menu copy, installed before the app catalog load point
    services: "Services",
    hide: "Hide Gloss",
    hide_others: "Hide Others",
    show_all: "Show All",
    quit: "Quit Gloss",
};

pub(crate) fn labels(locale: Locale) -> &'static MenuLabels {
    match locale {
        Locale::Zh => &ZH,
        Locale::En => &EN,
    }
}

#[cfg(test)]
mod tests {
    use gloss_core::model::Locale;

    use super::labels;

    #[test]
    fn labels_follow_their_language_per_locale() {
        for (locale, expect_cjk) in [(Locale::Zh, true), (Locale::En, false)] {
            let table = labels(locale);
            let rows = [
                table.about,
                table.settings,
                table.services,
                table.hide,
                table.hide_others,
                table.show_all,
                table.quit,
            ];
            for row in rows {
                let has_cjk = row.chars().any(|c| ('\u{4E00}'..='\u{9FFF}').contains(&c));
                assert_eq!(has_cjk, expect_cjk, "label table language mismatch: {row}");
                assert!(!row.trim().is_empty(), "menu labels must not be blank");
            }
        }
    }
}
