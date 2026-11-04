//! 系统（macOS）实现：适配器层现有实现的宿主目录。
//!
//! 端口实现整体迁入本目录，子模块结构与迁移前的库根一致；入口（lib.rs）
//! 按编译目标声明本目录并把子模块重新导出为平台无关路径。系统框架
//! （AppKit/CoreFoundation）的线程约束见各模块头；全部系统接口声明与
//! 所有权守卫仍集中在外部函数接口（FFI）模块，业务模块不直接声明外部符号。

pub mod appearance;
pub mod clipboard;
pub mod events;
pub(crate) mod ffi;
pub mod locale;
pub mod menu;
pub mod permissions;
pub mod scene;
pub mod selection;
pub mod storage;
