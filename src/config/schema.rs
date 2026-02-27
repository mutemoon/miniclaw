use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct WeComConfig {
    pub token: Option<String>,
    pub encoding_aes_key: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChannelConfig {
    pub wecom: Option<WeComConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub channel: ChannelConfig,
}
