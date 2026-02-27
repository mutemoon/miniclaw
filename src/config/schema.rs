use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct WeComConfig {
    pub token: Option<String>,
    pub encoding_aes_key: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct AgentChannelConfig {
    pub wecom: Option<WeComConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentConfig {
    /// 仓库根路径，claude 命令将在此目录下执行
    pub repo: String,
    /// 企微 channel 配置（可选，优先级低于仓库内配置）
    #[serde(default)]
    pub wecom: Option<WeComConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerConfig {
    pub addr: String,
    pub port: u16,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            addr: "0.0.0.0".to_string(),
            port: 3000,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct Config {
    pub server: ServerConfig,
    /// key 为 agent 名，对应路由 /wecom/<name>
    #[serde(default)]
    pub agents: HashMap<String, AgentConfig>,
}
