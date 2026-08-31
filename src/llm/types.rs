//! LLM 协议类型与 SSE chunk 解析（纯函数，可单元测试）。

use serde::{Deserialize, Serialize};

/// 请求用消息（仅 role + content，零额外字段）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn new(role: &str, content: impl Into<String>) -> Self {
        Self {
            role: role.to_string(),
            content: content.into(),
        }
    }
}

/// 后台流式任务 → 主循环的事件。
#[derive(Debug, Clone, PartialEq)]
pub enum LlmEvent {
    /// 正文增量
    Delta { text: String },
    /// 思考过程增量（reasoning_content）
    Reasoning { text: String },
    /// 正常结束
    Done,
    /// 失败（网络 / HTTP / 协议错误）
    Failed { error: String },
}

/// 一个 SSE data 增量中解析出的内容。
#[derive(Debug, Default, PartialEq)]
pub struct StreamDelta {
    pub content: Option<String>,
    pub reasoning: Option<String>,
}

/// 解析流式 chunk JSON（`data: {...}` 的 {...} 部分）。
/// 返回 None 表示无有效 delta（如首帧 role-only chunk 或无关事件）。
pub fn parse_chunk(json: &str) -> Option<StreamDelta> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let delta = v.get("choices")?.get(0)?.get("delta")?;
    let mut out = StreamDelta::default();
    if let Some(c) = delta.get("content").and_then(|c| c.as_str()) {
        if !c.is_empty() {
            out.content = Some(c.to_string());
        }
    }
    // DeepSeek / Kimi 的思考字段
    if let Some(r) = delta
        .get("reasoning_content")
        .and_then(|c| c.as_str())
        .or_else(|| delta.get("reasoning").and_then(|c| c.as_str()))
    {
        if !r.is_empty() {
            out.reasoning = Some(r.to_string());
        }
    }
    Some(out)
}

/// 从响应 JSON 中提取错误消息（`{"error":{"message":...}}`）。
pub fn parse_error(json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let msg = v
        .get("error")
        .and_then(|e| e.get("message"))
        .and_then(|m| m.as_str())
        .or_else(|| v.get("message").and_then(|m| m.as_str()))?;
    Some(msg.to_string())
}

/// SSE 行 → data 负载（去掉 `data:` 前缀与空白）。非 data 行返回 None。
pub fn sse_data_line(line: &str) -> Option<&str> {
    let line = line.trim();
    line.strip_prefix("data:").map(|d| d.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_line() {
        assert_eq!(sse_data_line("data: hello"), Some("hello"));
        assert_eq!(sse_data_line("data:[DONE]"), Some("[DONE]"));
        assert_eq!(sse_data_line("event: x"), None);
        assert_eq!(sse_data_line(""), None);
    }

    #[test]
    fn chunk_content() {
        let j = r#"{"id":"1","choices":[{"delta":{"content":"你好"}}]}"#;
        let d = parse_chunk(j).unwrap();
        assert_eq!(d.content.as_deref(), Some("你好"));
        assert_eq!(d.reasoning, None);
    }

    #[test]
    fn chunk_reasoning() {
        let j = r#"{"choices":[{"delta":{"reasoning_content":"思考中"}}]}"#;
        let d = parse_chunk(j).unwrap();
        assert_eq!(d.reasoning.as_deref(), Some("思考中"));
    }

    #[test]
    fn chunk_empty_delta() {
        let j = r#"{"choices":[{"delta":{"role":"assistant"}}]}"#;
        let d = parse_chunk(j).unwrap();
        assert_eq!(d.content, None);
    }

    #[test]
    fn error_msg() {
        let j = r#"{"error":{"message":"invalid api key","type":"auth"}}"#;
        assert_eq!(parse_error(j), Some("invalid api key".to_string()));
    }
}
