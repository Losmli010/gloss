//! 系统语言读取（系统（Windows）占位）：配置选“跟随系统”时的输入。
//!
//! 系统实现对应用户默认区域名（GetUserDefaultLocaleName），按移植方案
//! 后续阶段补齐；本桩回落英文档位——与系统实现“读不到偏好即英文”的
//! 降级语义一致。进程内启动期读一次。

use gloss_core::model::Locale;

pub fn system_locale() -> Locale {
    Locale::En
}
