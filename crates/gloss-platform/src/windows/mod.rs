//! 系统（Windows）实现：适配器层的系统（Windows）能力宿主目录。
//!
//! 核心链路（划词）在本目录落地：低级鼠标钩子（SetWindowsHookExW）的手势
//! 源、用户界面自动化（UIA）文本模式（TextPattern）主路加按键注入
//! （SendInput）模拟复制兜底的取材通道、前台窗口进程查询加敏感应用名单的
//! 场景探针。应用菜单落为托盘图标加托盘菜单（tray-icon），权限面按无系统
//! 级授权门定案（预检恒放行、引导为无操作），图标装配按任务栏图标来自打包
//! 资源定案为无操作。配置与密钥存储组合落为文件加凭据管理器，桌面入口落为
//! 桌面快捷方式重建。系统库绑定与组件对象模型（COM）面集中在外部函数接口
//! （FFI）模块，业务模块不直接触达系统库绑定；平台事件线程跑消息泵
//! （Win32）宿主并在启动序初始化单线程套间（STA），线程约束见各模块头。

pub mod appearance;
pub mod clipboard;
pub mod events;
pub(crate) mod ffi;
pub mod locale;
pub mod menu;
pub mod permissions;
pub mod scene;
pub mod selection;
pub mod shortcut;
pub mod storage;
