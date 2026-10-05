//! 代码语言探测、归一化与语法着色（demo 同款架构：单趟正则分组捕获）。
//!
//! 按职责拆三个子模块，行为零变化：
//! - [`detect`]——语言探测（shebang、doctype、语言签名形）与别名归一化；
//! - [`tokenize`]——按语言组装规则集正则（惰性编译一次）并单趟扫描出
//!   各类别的字节区间；
//! - [`palette`]——六类着色类别（[`Class`]）的 demo 调色板与斜体规则。
//!
//! 规则集由共享词法积木组装——行/块注释、字符串（含转义与三引号）、
//! 数字（含十六进制/浮点）、大写驼峰类型、函数形——每语言只补关键字表；
//! JSON/YAML/TOML/SQL/HTML/XML/CSS 出特化规则（键/标签/选择器）。未知
//! 语言走通用启发集（字符串/两类注释/数字/类型/函数形仍着色），探测
//! 不出的语言角标不显示。
//!
//! 语言有两级来源：平台 hint（[`gloss_core::task::InputHint::CodeLanguage`]）
//! 优先，缺失时对文本内容探测。判定在视图创建时做一次，不进渲染热路径；
//! 结果同时供语言角标文字与高亮规则集选择消费。归一化把别名收拢到规范名
//! （rs→rust、py→python…），角标与规则集都只认规范名。

mod detect;
mod palette;
mod tokenize;

pub(crate) use detect::{detect_language, normalize_language};
pub(crate) use tokenize::tokenize;
