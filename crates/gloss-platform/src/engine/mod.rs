//! 引擎适配器：实现 core 的 [`gloss_core::ports::AiEngine`] 端口。
//!
//! - [`llm`]：OpenAI 兼容的流式客户端（M4-T4），请求渲染在 core、这里只
//!   负责传输；
//! - [`sse`]：流式响应的增量解码，纯函数、可脱离网络单测。
pub mod llm;
pub mod sse;
