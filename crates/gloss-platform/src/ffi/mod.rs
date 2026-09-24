//! 平台 FFI 的唯一落点：框架声明、不透明类型、`unsafe` 助手与所有权守卫。
//!
//! 本模块集中平台适配层用到的全部 C 接口声明与 SAFETY 依据，按框架分文件：
//! [`cf`]（CoreFoundation 符号与守卫）、[`ax`]（Accessibility）、
//! [`pasteboard`]（粘贴板）、[`eventtap`]（CoreGraphics 事件 tap）、
//! [`carbon`]（系统输入态查询）。业务模块
//! 从这里引用，不再各自声明——签名与内存管理规则因此只有一处需要审计。
//!
//! 调用方负责在自己那侧的 SAFETY 注释里写明「为何此处的前置条件成立」
//! （指针生命周期、线程归属、授权状态等），本模块只写接口自身的契约。

pub(crate) mod ax;
pub(crate) mod carbon;
pub(crate) mod cf;
pub(crate) mod eventtap;
pub(crate) mod pasteboard;
