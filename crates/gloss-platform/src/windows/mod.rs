//! 系统（Windows）占位实现：端口桩，与系统（macOS）实现同名镜像。
//!
//! 编译面先行：装配契约（bootstrap）的句柄构造（Platform::connect）所
//! 引用的端口适配在这里按同名镜像给出桩实现，行为一律如实报告未支持——有结果
//! 通道的返回平台未支持（PlatformUnsupported），事件源轮询恒产出空集，
//! 菜单装配报告未装配。能力落地与线程模型（系统（Windows）的消息泵
//! （Win32）宿主）按移植方案后续阶段补齐，届时本目录整体替换为真实现。
//!
//! 镜像只覆盖装配触及的公开面：系统（macOS）实现内部的子模块（如外部
//! 函数接口、辅助功能读取器）没有占位——无消费方、无假实现。

pub mod appearance;
pub mod clipboard;
pub mod events;
pub mod locale;
pub mod menu;
pub mod permissions;
pub mod scene;
pub mod selection;
pub mod storage;
