//! 会话结构。

/// 一次对话会话。
#[derive(Debug, Clone)]
#[allow(dead_code)] // 时间戳为持久化/排序字段，运行时暂不读取
pub struct Session {
    pub id: i64,
    pub title: String,
    pub provider: String,
    pub model: String,
    pub system_prompt: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Session {
    pub const DEFAULT_TITLE: &'static str = "新会话";
}
