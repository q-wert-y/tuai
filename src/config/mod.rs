//! 配置：`.tuai/config.toml` 的加载、保存与提供商管理。

pub mod provider;

use anyhow::Context;
use serde::{Deserialize, Serialize};

pub use provider::Provider;

/// 全局配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// 默认提供商名称（providers 中的 name）。
    pub default_provider: String,
    /// 全局默认系统提示词（会话未单独设置时使用）。
    #[serde(default)]
    pub system_prompt: Option<String>,
    pub providers: Vec<Provider>,
    /// 收藏的模型，条目格式为 "provider/model"。
    #[serde(default)]
    pub favorites: Vec<String>,
    /// 强制代理地址（如 "http://127.0.0.1:7890"）。
    /// 留空 = 自动：环境变量 > Windows 系统代理（Clash 开系统代理即生效）> 直连。
    #[serde(default)]
    pub proxy: Option<String>,
    /// 上次使用的提供商（新建会话时沿用，而非默认提供商）。
    #[serde(default)]
    pub last_provider: Option<String>,
    /// 上次使用的模型（新建会话时沿用）。
    #[serde(default)]
    pub last_model: Option<String>,
}

impl Config {
    /// 加载配置；文件不存在时写入默认模板（空提供商，首启引导添加）。
    pub fn load_or_init(data_dir: &std::path::Path) -> anyhow::Result<Config> {
        let path = data_dir.join("config.toml");
        if path.exists() {
            let raw = std::fs::read_to_string(&path)
                .with_context(|| format!("读取配置失败: {}", path.display()))?;
            let cfg: Config = toml::from_str(&raw)
                .with_context(|| format!("解析配置失败: {}", path.display()))?;
            return Ok(cfg);
        }
        let cfg = Config::default();
        cfg.save(data_dir)?;
        Ok(cfg)
    }

    /// 保存配置到 `.tuai/config.toml`。
    pub fn save(&self, data_dir: &std::path::Path) -> anyhow::Result<()> {
        let path = data_dir.join("config.toml");
        let raw = toml::to_string_pretty(self)?;
        std::fs::write(&path, raw).with_context(|| format!("写入配置失败: {}", path.display()))?;
        Ok(())
    }

    /// 按名称查找提供商。
    pub fn provider(&self, name: &str) -> Option<&Provider> {
        self.providers.iter().find(|p| p.name == name)
    }

    /// 按名称获取生效配置（应用环境变量 TUAI_API_KEY / TUAI_BASE_URL 覆盖）。
    pub fn effective_provider(&self, name: &str) -> Option<Provider> {
        let mut p = self.provider(name)?.clone();
        if let Ok(key) = std::env::var("TUAI_API_KEY") {
            if !key.trim().is_empty() {
                p.api_key = key.trim().to_string();
            }
        }
        if let Ok(base) = std::env::var("TUAI_BASE_URL") {
            if !base.trim().is_empty() {
                p.base_url = base.trim().trim_end_matches('/').to_string();
            }
        }
        Some(p)
    }

    /// 是否已收藏（provider + model）。
    pub fn is_favorite(&self, provider: &str, model: &str) -> bool {
        let key = format!("{provider}/{model}");
        self.favorites.contains(&key)
    }

    /// 切换收藏状态，返回切换后是否已收藏。
    pub fn toggle_favorite(&mut self, provider: &str, model: &str) -> bool {
        let key = format!("{provider}/{model}");
        if let Some(i) = self.favorites.iter().position(|f| *f == key) {
            self.favorites.remove(i);
            false
        } else {
            self.favorites.push(key);
            true
        }
    }

    /// 某提供商下已收藏的模型名列表。
    pub fn favorites_of(&self, provider: &str) -> Vec<String> {
        let prefix = format!("{provider}/");
        self.favorites
            .iter()
            .filter_map(|f| f.strip_prefix(&prefix).map(|s| s.to_string()))
            .collect()
    }

    fn default() -> Config {
        // 不内置任何提供商：首次运行引导用户添加
        Config {
            default_provider: String::new(),
            system_prompt: None,
            providers: Vec::new(),
            favorites: Vec::new(),
            proxy: None,
            last_provider: None,
            last_model: None,
        }
    }
}
