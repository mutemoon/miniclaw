pub mod crypto;
pub mod types;

use crate::channels::traits::{Channel, ChannelMessage, SendMessage};
use crate::config::schema::WeComConfig;
use async_trait::async_trait;
use rust_i18n::t;
use tracing::info;
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
                    tracing::error!("{}", t!("wecom_signature_verification_failed", error = e));
                    return vec![];
                }
            }

            match crypto::decrypt(&self.config, encrypt) {
                Ok(decrypted) => match serde_json::from_str::<serde_json::Value>(&decrypted) {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::error!(
                            "{}",
                            t!("failed_to_parse_decrypted_wecom_json", error = e)
                        );
                        return vec![];
                    }
                },
                Err(e) => {
                    tracing::error!("{}", t!("failed_to_decrypt_wecom_message", error = e));
                    return vec![];
                }
            }
        } else {
            payload.clone()
        };

        info!("{:?}", msg_json);

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
            anyhow::bail!(t!("invalid_wecom_response_url", url = response_url));
        }

        tracing::info!(
            "{}",
            t!(
                "wecom_sending_message",
                recipient = response_url,
                len = message.content.len()
            )
        );

        let body = serde_json::json!({
            "msgtype": "markdown",
            "markdown": {
                "content": message.content
            }
        });

        tracing::info!("{}", t!("wecom_sending_message_body", body = body));

        let res = self.client.post(response_url).json(&body).send().await?;

        if !res.status().is_success() {
            let status = res.status();
            let text = res.text().await.unwrap_or_default();
            tracing::error!("{}", t!("wecom_reply_failed", status = status, body = text));
            anyhow::bail!(t!("wecom_reply_failed", status = status, body = text));
        }

        tracing::info!("{}", t!("wecom_sent_success", url = response_url));

        Ok(())
    }

    async fn listen(&self, _tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> anyhow::Result<()> {
        Ok(())
    }
}
