pub mod crypto;
pub mod types;

use crate::channels::traits::{Channel, ChannelMessage, SendMessage};
use crate::config::schema::WeComConfig;
use async_trait::async_trait;
use types::WeComPayload;

#[derive(Debug)]
pub struct WeComChannel {
    pub config: WeComConfig,
    pub client: reqwest::Client,
}

impl WeComChannel {
    pub fn new(config: WeComConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    pub fn verify_url(
        &self,
        msg_signature: &str,
        timestamp: &str,
        nonce: &str,
        echostr: &str,
    ) -> anyhow::Result<String> {
        crypto::verify_signature(&self.config, msg_signature, timestamp, nonce, echostr)?;
        let decrypted = crypto::decrypt(&self.config, echostr)?;
        Ok(decrypted)
    }

    pub async fn parse_webhook_payload(
        &self,
        payload: &serde_json::Value,
        signature: Option<&str>,
        timestamp: Option<&str>,
        nonce: Option<&str>,
    ) -> Vec<ChannelMessage> {
        let msg_json = if let Some(encrypt) = payload.get("encrypt").and_then(|e| e.as_str()) {
            if let (Some(sig), Some(ts), Some(n)) = (signature, timestamp, nonce) {
                if let Err(e) = crypto::verify_signature(&self.config, sig, ts, n, encrypt) {
                    tracing::error!("WeCom signature verification failed: {e}");
                    return vec![];
                }
            }

            match crypto::decrypt(&self.config, encrypt) {
                Ok(decrypted) => match serde_json::from_str::<serde_json::Value>(&decrypted) {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::error!("Failed to parse decrypted WeCom JSON: {e}");
                        return vec![];
                    }
                },
                Err(e) => {
                    tracing::error!("Failed to decrypt WeCom message: {e}");
                    return vec![];
                }
            }
        } else {
            payload.clone()
        };

        let Ok(data) = serde_json::from_value::<WeComPayload>(msg_json) else {
            return vec![];
        };

        data.to_channel_messages()
    }
}

#[async_trait]
impl Channel for WeComChannel {
    fn name(&self) -> &str {
        "wecom"
    }

    async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
        let response_url = &message.recipient;
        if !response_url.starts_with("http") {
            anyhow::bail!("Invalid WeCom response_url: {}", response_url);
        }

        tracing::info!(
            "WeCom sending message: recipient={} content_len={}",
            response_url,
            message.content.len()
        );

        let body = serde_json::json!({
            "msgtype": "markdown",
            "markdown": {
                "content": message.content
            }
        });

        tracing::info!("WeCom sending message: body={}", body);

        let res = self.client.post(response_url).json(&body).send().await?;

        if !res.status().is_success() {
            let status = res.status();
            let text = res.text().await.unwrap_or_default();
            tracing::error!("WeCom reply failed: status={} body={}", status, text);
            anyhow::bail!("WeCom reply failed with status {}: {}", status, text);
        }

        tracing::info!("WeCom message sent successfully to {}", response_url);

        Ok(())
    }

    async fn listen(&self, _tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> anyhow::Result<()> {
        Ok(())
    }
}
