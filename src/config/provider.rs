//! 提供商定义。

use serde::{Deserialize, Serialize};

/// 一个 OpenAI 兼容提供商配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provider {
    /// 唯一名称（如 "deepseek"）。
    pub name: String,
    /// API 基地址，以 `/v1` 结尾（如 https://api.deepseek.com/v1）。
    pub base_url: String,
    /// API Key（存于配置文件，可被环境变量 TUAI_API_KEY 覆盖）。
    #[serde(default)]
    pub api_key: String,
    /// 默认模型。
    pub default_model: String,
}
