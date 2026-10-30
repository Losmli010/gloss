//! 领域层分组：领域词汇与校验——模型、任务、输出契约、配置与防护。
//!
//! 本组是任务智能层与端口契约的公共底座，零渲染零窗口依赖；成员对包外
//! 的可用路径经库入口转出承载，分组只改落位，不改任何类型与签名。
pub mod config;
pub mod guard;
pub mod model;
pub mod schema;
pub mod task;
