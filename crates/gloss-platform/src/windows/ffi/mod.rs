//! 系统（Windows）外部函数接口（FFI）的唯一落点：系统库绑定取用、组件对象
//! 模型（COM）面与安全包装。
//!
//! 本模块集中平台适配层用到的全部系统接口调用，按关注面分文件：组件对象
//! 模型（COM）套间与自动化对象创建、低级鼠标钩子与消息泵、按键注入、剪贴板
//! 序列号与格式清单、前台窗口进程查询、用户界面自动化（UIA）文本选区读取、
//! 凭据管理器通用凭据、桌面快捷方式写入。
//! 业务模块从这里引用，不再各自触达系统库绑定——调用面与安全性依据因此
//! 只有一处需要审计。
//!
//! 调用方负责在自己那侧的不变量注释里写明“为何此处的前置条件成立”
//! （指针生命周期、线程归属、句柄配对等），本模块只写接口自身的契约。

pub(crate) mod board;
pub(crate) mod com;
pub(crate) mod cred;
pub(crate) mod foreground;
pub(crate) mod hook;
pub(crate) mod inject;
pub(crate) mod shell_link;
pub(crate) mod uia;
