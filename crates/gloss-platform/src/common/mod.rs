//! 平台中立共享层：两侧平台目录零差异的业务逻辑宿主目录。本目录零目标
//! 条件编译、零外部函数接口（FFI）、零不安全块——平台差异只经注入闭包
//! 与类型参数进入；条目按问题域分文件，时序与预算常量随各自的共享模块
//! 单源。平台目录只留平台原语，并把各自的密钥半边等平台原生件绑定到
//! 本层的共享形状上。

pub mod clipboard_watch;
pub mod gesture;
pub mod image;
pub mod locale;
pub mod menu_labels;
pub mod storage;
