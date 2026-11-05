//! 系统（Windows）实现：适配器层的系统（Windows）能力宿主目录。
//!
//! 核心链路（划词）在本目录落地：低级鼠标钩子（SetWindowsHookExW）的手势
//! 源、用户界面自动化（UIA）文本模式（TextPattern）主路加按键注入
//! （SendInput）模拟复制兜底的取材通道、前台窗口进程查询加敏感应用名单的
//! 场景探针。系统库绑定与组件对象模型（COM）面集中在外部函数接口（FFI）
//! 模块，业务模块不直接触达系统库绑定；平台事件线程跑消息泵（Win32）宿主
//! 并在启动序初始化单线程套间（STA），线程约束见各模块头。尚未落地的能力
//! （渲染期外观、托盘菜单、剪贴板监视与图片取材、密钥存储等）保留移植方案
//! 后续阶段的同名镜像桩，未实现的端口一律返回平台未支持（PlatformUnsupported）。

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
