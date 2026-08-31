//! LLM 客户端：OpenAI 兼容协议的薄封装。

pub mod openai;
pub mod types;

pub use openai::OpenAiClient;
pub use types::{ChatMessage, LlmEvent};
