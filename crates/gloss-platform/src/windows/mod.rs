//! 系统（Windows）实现：适配器层的系统（Windows）能力宿主目录。
//!
//! 核心链路（划词）在本目录落地：低级鼠标钩子（SetWindowsHookExW）的手势
//! 源、用户界面自动化（UIA）文本模式（TextPattern）主路加按键注入
//! （SendInput）模拟复制兜底的取材通道、前台窗口进程查询加敏感应用名单的
//! 场景探针。应用菜单落为托盘图标加托盘菜单（tray-icon），权限面按无系统
//! 级授权门定案（预检恒放行、引导为无操作），图标装配按任务栏图标来自打包
//! 资源定案为无操作。系统库绑定与组件对象模型（COM）面集中在外部函数接口
//! （FFI）模块，业务模块不直接触达系统库绑定；平台事件线程跑消息泵
//! （Win32）宿主并在启动序初始化单线程套间（STA），线程约束见各模块头。
//! 尚未落地的能力（配置与密钥存储等）保留移植方案后续阶段的同名镜像桩，
//! 未实现的端口一律返回平台未支持（PlatformUnsupported）。

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
