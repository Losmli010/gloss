//! 消息契约类型：开放接口（OpenAI）兼容消息的域形态——角色与正文部件，
//! 任务渲染、分类请求与修复轮会话共同组装的消息序列都以它承载。
//!
//! 本模块只产“域形态”的消息（角色与消息部件），不做传输序列化，也不携带
//! 内联数据——把部件数组映成开放接口（OpenAI）兼容内容、把图像字节编码成
//! 数据地址前缀都归传输层，即大语言模型（LLM）客户端的请求体组装。

use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageContent {
    Text(String),
    Parts(Vec<ContentPart>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentPart {
    ImagePng(Arc<[u8]>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: Role,
    pub content: MessageContent,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: MessageContent::Text(content.into()),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: MessageContent::Text(content.into()),
        }
    }

    pub fn user_parts(parts: Vec<ContentPart>) -> Self {
        Self {
            role: Role::User,
            content: MessageContent::Parts(parts),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: MessageContent::Text(content.into()),
        }
    }
}
