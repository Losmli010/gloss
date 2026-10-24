//! 平台外部函数接口（FFI）的唯一落点：框架声明、不透明类型、非安全助手与所有权守卫。
//!
//! 本模块集中平台适配层用到的全部系统接口声明与安全性依据，按框架分文件：
//! 核心基础框架（CoreFoundation）符号与守卫、辅助功能（AX）、
//! 粘贴板、图形核心框架（CoreGraphics）事件监听（tap）、
//! 图形核心框架（CoreGraphics）事件注入、系统框架（Carbon）系统输入态查询。
//! 业务模块从这里引用，不再各自声明——签名与内存管理规则因此只有一处
//! 需要审计。
//!
//! 调用方负责在自己那侧的不变量注释（SAFETY）里写明“为何此处的前置条件
//! 成立”（指针生命周期、线程归属、授权状态等），本模块只写接口自身的契约。

pub(crate) mod ax;
pub(crate) mod carbon;
pub(crate) mod cf;
pub(crate) mod event;
pub(crate) mod eventtap;
pub(crate) mod pasteboard;
