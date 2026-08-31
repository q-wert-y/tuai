//! 消息结构与角色。

/// 消息角色。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    System,
    User,
    Assistant,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
        }
    }

    pub fn parse(s: &str) -> Option<Role> {
        match s {
            "system" => Some(Role::System),
            "user" => Some(Role::User),
            "assistant" => Some(Role::Assistant),
            _ => None,
        }
    }
}

/// 一条持久化的聊天消息。
#[derive(Debug, Clone)]
#[allow(dead_code)] // session_id / created_at 为持久化字段，运行时暂不读取
pub struct Message {
    pub id: i64,
    pub session_id: i64,
    pub role: Role,
    pub content: String,
    /// 思考过程（DeepSeek / Kimi 等的 reasoning_content），可选。
    pub reasoning: Option<String>,
    pub created_at: i64,
}
