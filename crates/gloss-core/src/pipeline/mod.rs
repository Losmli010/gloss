//! 任务智能层分组：语言门、措辞、提示词、契约修复、分类与任务引擎，含
//! 语言规范数据与违约简报两份包内支撑。
//!
//! 分组沿实测依赖分层落位——本组消费领域词汇；端口契约签名携带本组的
//! 消息契约类型是六边形既有形态，分组不改变它。私有成员保持包内私有，
//! 对包外的可用路径同样经库入口转出承载。
pub(crate) mod briefs;
pub mod classify;
pub mod engine;
pub mod lang_gate;
pub(crate) mod langspec;
pub mod prompt;
pub(crate) mod repair;
pub(crate) mod wordings;
