//! 数据模型：会话与消息。

pub mod message;
pub mod session;

pub use message::{Message, Role};
pub use session::Session;
