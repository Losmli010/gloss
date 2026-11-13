//! 系统语言读取（macOS）：配置选“跟随系统”时的唯一输入，提示词模板
//! 与界面文案共用这一份结果。
//!
//! 只看系统首选语言列表的首项，例如简体中文（zh-Hans-CN）；进程内
//! 启动期读一次，系统语言变了需重启应用。语言名到档位的映射住平台中立
//! 共享层（common），本模块只提供首选语言的原语读取面。

use gloss_core::model::Locale;
use objc2_foundation::NSLocale;

use crate::common::locale::locale_for;

pub fn system_locale() -> Locale {
    let preferred = NSLocale::preferredLanguages();
    let first = preferred.to_vec().first().map(ToString::to_string);
    locale_for(first.as_deref())
}
