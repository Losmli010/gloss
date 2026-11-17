//! 开放接口（OpenAI）方言文件归组：请求体构造、流式事件（SSE）解码与失败
//! 信封映射。方言中立传输（连接、重试、空闲看门狗）只经本模块取用方言
//! 条目；与提供方线上形态绑定的改动面收在本目录内。

pub(crate) mod failure;
pub(crate) mod request_body;
pub(crate) mod sse;
