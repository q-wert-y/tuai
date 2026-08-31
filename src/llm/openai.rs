//! OpenAI 兼容客户端：chat/completions 流式请求与 models 列表。

use crate::config::provider::Provider;
use crate::llm::types::{parse_chunk, parse_error, sse_data_line, ChatMessage, LlmEvent};
use futures_util::StreamExt;
use std::time::Duration;
use tokio::sync::mpsc;

/// 客户端（每次请求前按 provider/model 构造，内部复用连接池）。
#[derive(Clone)]
pub struct OpenAiClient {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
}

impl OpenAiClient {
    /// `forced_proxy`：config 里强制指定的代理；None = 自动检测（env > 系统代理 > 直连）。
    pub fn new(provider: &Provider, model: &str, forced_proxy: Option<&str>) -> Self {
        let proxy = forced_proxy
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .or_else(crate::util::detect_proxy);
        let mut builder = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .user_agent(concat!("tuai/", env!("CARGO_PKG_VERSION")));
        if let Some(p) = proxy {
            if let Ok(px) = reqwest::Proxy::all(&p) {
                builder = builder.proxy(px);
            }
        }
        let http = builder.build().unwrap_or_default();
        Self {
            http,
            base_url: provider.base_url.trim_end_matches('/').to_string(),
            api_key: provider.api_key.clone(),
            model: model.to_string(),
        }
    }

    fn api(&self, path: &str) -> String {
        format!("{}/{}", self.base_url, path)
    }

    /// 发起流式请求。所有结果通过 `tx` 推送：Delta/Reasoning/Done/Failed。
    /// 本函数不返回 Err —— 失败一律以 `Failed` 事件表达。
    pub async fn chat_stream(
        &self,
        system: Option<String>,
        history: &[ChatMessage],
        tx: &mpsc::Sender<LlmEvent>,
    ) {
        // 请求体：{model, messages, stream} —— 零工具字段、零额外 token
        let mut messages: Vec<ChatMessage> = Vec::with_capacity(history.len() + 1);
        if let Some(s) = system.filter(|s| !s.trim().is_empty()) {
            messages.push(ChatMessage::new("system", s));
        }
        messages.extend_from_slice(history);

        let body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "stream": true,
        });

        let resp = self
            .http
            .post(self.api("chat/completions"))
            .bearer_auth(&self.api_key)
            .header("Accept", "text/event-stream")
            .json(&body)
            .send()
            .await;

        let resp = match resp {
            Ok(r) => r,
            Err(e) => {
                let _ = tx
                    .send(LlmEvent::Failed {
                        error: format!("连接失败: {e}"),
                    })
                    .await;
                return;
            }
        };

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            let detail = parse_error(&body)
                .unwrap_or_else(|| crate::util::truncate_by_width(body.trim(), 300));
            let _ = tx
                .send(LlmEvent::Failed {
                    error: format!("HTTP {status}: {detail}"),
                })
                .await;
            return;
        }

        // SSE 流式读取：按行切分，逐行解析
        let mut stream = resp.bytes_stream();
        let mut buf: Vec<u8> = Vec::with_capacity(4096);
        let mut got_any = false;
        while let Some(item) = stream.next().await {
            let bytes = match item {
                Ok(b) => b,
                Err(e) => {
                    let _ = tx
                        .send(LlmEvent::Failed {
                            error: if got_any {
                                format!("连接中断（已保留部分内容）: {e}")
                            } else {
                                format!("连接中断: {e}")
                            },
                        })
                        .await;
                    return;
                }
            };
            buf.extend_from_slice(&bytes);
            while let Some(nl) = buf.iter().position(|&b| b == b'\n') {
                let line_bytes: Vec<u8> = buf.drain(..=nl).collect();
                let line = String::from_utf8_lossy(&line_bytes);
                let line = line.trim_end_matches('\r');
                let Some(data) = sse_data_line(line) else {
                    continue;
                };
                if data == "[DONE]" {
                    let _ = tx.send(LlmEvent::Done).await;
                    return;
                }
                match parse_chunk(data) {
                    Some(d) if d.content.is_some() || d.reasoning.is_some() => {
                        got_any = true;
                        if let Some(t) = d.content {
                            let _ = tx.send(LlmEvent::Delta { text: t }).await;
                        }
                        if let Some(t) = d.reasoning {
                            let _ = tx.send(LlmEvent::Reasoning { text: t }).await;
                        }
                    }
                    Some(_) => {}
                    None => {
                        // 非 chunk JSON：可能是错误事件
                        if let Some(err) = parse_error(data) {
                            let _ = tx.send(LlmEvent::Failed { error: err }).await;
                            return;
                        }
                    }
                }
            }
        }
        // 流正常结束但未收到 [DONE]（部分提供商如此）
        if got_any {
            let _ = tx.send(LlmEvent::Done).await;
        } else {
            let _ = tx
                .send(LlmEvent::Failed {
                    error: "流意外结束：未收到任何内容".to_string(),
                })
                .await;
        }
    }

    /// 拉取模型列表（GET /models）。
    pub async fn list_models(&self) -> anyhow::Result<Vec<String>> {
        let resp = self
            .http
            .get(self.api("models"))
            .bearer_auth(&self.api_key)
            .send()
            .await?;
        let status = resp.status();
        let body = resp.text().await?;
        if !status.is_success() {
            let detail =
                parse_error(&body).unwrap_or_else(|| crate::util::truncate_by_width(&body, 200));
            anyhow::bail!("HTTP {status}: {detail}");
        }
        let v: serde_json::Value =
            serde_json::from_str(&body).map_err(|e| anyhow::anyhow!("解析模型列表失败: {e}"))?;
        let mut models = Vec::new();
        if let Some(arr) = v.get("data").and_then(|d| d.as_array()) {
            for m in arr {
                if let Some(id) = m.get("id").and_then(|i| i.as_str()) {
                    models.push(id.to_string());
                }
            }
        }
        models.sort();
        Ok(models)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stream_end_to_end() {
        // 迷你 SSE mock server
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            use std::io::{Read, Write};
            let mut req = [0u8; 4096];
            let n = sock.read(&mut req).unwrap();
            let req = String::from_utf8_lossy(&req[..n]).to_string();
            assert!(req.contains("POST /v1/chat/completions"));
            assert!(req.contains("Bearer sk-test"));
            let body = concat!(
                "HTTP/1.1 200 OK\r\n",
                "Content-Type: text/event-stream\r\n",
                "Connection: close\r\n\r\n",
                "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n",
                "data: {\"choices\":[{\"delta\":{\"content\":\"你好\"}}]}\n\n",
                "data: {\"choices\":[{\"delta\":{\"content\":\"，世界\"}}]}\n\n",
                "data: [DONE]\n\n",
            );
            sock.write_all(body.as_bytes()).unwrap();
        });

        let provider = Provider {
            name: "test".into(),
            base_url: format!("http://{addr}/v1"),
            api_key: "sk-test".into(),
            default_model: "test-model".into(),
        };
        let client = OpenAiClient::new(&provider, "test-model", None);
        let (tx, mut rx) = mpsc::channel(64);
        let hist = vec![ChatMessage::new("user", "hi")];
        client.chat_stream(None, &hist, &tx).await;
        drop(tx);

        let mut events = Vec::new();
        while let Some(e) = rx.recv().await {
            events.push(e);
        }
        assert_eq!(
            events,
            vec![
                LlmEvent::Delta {
                    text: "你好".into()
                },
                LlmEvent::Delta {
                    text: "，世界".into()
                },
                LlmEvent::Done,
            ]
        );
        handle.join().unwrap();
    }
}
