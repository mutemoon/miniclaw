use crate::channels::traits::{Channel, ChannelMessage, SendMessage};
use crate::config::schema::WeComConfig;
use aes::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use async_trait::async_trait;
use base64::{
    engine::GeneralPurpose,
    engine::{general_purpose, DecodePaddingMode},
    Engine as _,
};
use cbc::{Decryptor, Encryptor};
use sha1::{Digest, Sha1};
use std::time::{SystemTime, UNIX_EPOCH};

type Aes256CbcEnc = Encryptor<aes::Aes256>;
type Aes256CbcDec = Decryptor<aes::Aes256>;

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
        self.verify_signature(msg_signature, timestamp, nonce, echostr)?;
        let decrypted = self.decrypt(echostr)?;
        Ok(decrypted)
    }

    fn verify_signature(
        &self,
        msg_signature: &str,
        timestamp: &str,
        nonce: &str,
        data: &str,
    ) -> anyhow::Result<()> {
        let Some(ref token) = self.config.token else {
            anyhow::bail!("WeCom token not configured; signature verification aborted");
        };

        let mut params = [token.as_str(), timestamp, nonce, data];
        params.sort_unstable();

        let mut hasher = Sha1::new();
        hasher.update(params.concat());
        let expected = hex::encode(hasher.finalize());

        if expected != msg_signature {
            anyhow::bail!("Invalid WeCom signature");
        }
        Ok(())
    }

    fn decrypt(&self, encrypted: &str) -> anyhow::Result<String> {
        let Some(ref encoding_aes_key) = self.config.encoding_aes_key else {
            anyhow::bail!("WeCom encoding_aes_key not configured; decryption aborted");
        };

        let alphabet = base64::alphabet::STANDARD;
        let config = general_purpose::GeneralPurposeConfig::new()
            .with_decode_padding_mode(DecodePaddingMode::Indifferent)
            .with_decode_allow_trailing_bits(true);
        let engine = GeneralPurpose::new(&alphabet, config);

        let key_to_decode = if encoding_aes_key.len() == 43 {
            format!("{encoding_aes_key}=")
        } else {
            encoding_aes_key.clone()
        };

        let aes_key_full = engine
            .decode(&key_to_decode)
            .or_else(|_| engine.decode(encoding_aes_key))?;

        if aes_key_full.len() < 32 {
            anyhow::bail!(
                "Invalid aes_key length: expected at least 32 bytes, got {}",
                aes_key_full.len()
            );
        }
        let aes_key = &aes_key_full[..32];

        let mut iv = [0u8; 16];
        iv.copy_from_slice(&aes_key[..16]);

        let mut ciphertext = engine
            .decode(encrypted.trim())
            .or_else(|_| engine.decode(encrypted.trim()))?;

        let decryptor = Aes256CbcDec::new(aes_key.into(), &iv.into());
        use aes::cipher::block_padding::NoPadding;
        let decrypted_raw = decryptor
            .decrypt_padded_mut::<NoPadding>(&mut ciphertext)
            .map_err(|e| anyhow::anyhow!("AES decryption failed: {:?}", e))?;

        let padding_len = *decrypted_raw
            .last()
            .ok_or_else(|| anyhow::anyhow!("Empty decrypted buffer"))?
            as usize;
        if padding_len == 0 || padding_len > 32 {
            anyhow::bail!("Invalid WeCom padding length: {}", padding_len);
        }
        let padding_start = decrypted_raw
            .len()
            .checked_sub(padding_len)
            .ok_or_else(|| anyhow::anyhow!("Padding length exceeds buffer size"))?;

        if !decrypted_raw[padding_start..]
            .iter()
            .all(|&b| b == padding_len as u8)
        {
            anyhow::bail!("Invalid WeCom PKCS#7 padding bytes");
        }
        let decrypted = &decrypted_raw[..padding_start];

        if decrypted.len() < 20 {
            anyhow::bail!("Decrypted content too short ({} bytes)", decrypted.len());
        }

        let msg_len_bytes = &decrypted[16..20];
        let msg_len = u32::from_be_bytes([
            msg_len_bytes[0],
            msg_len_bytes[1],
            msg_len_bytes[2],
            msg_len_bytes[3],
        ]) as usize;

        if decrypted.len() < 20 + msg_len {
            anyhow::bail!(
                "Decrypted message length mismatch: buffer={} msg_len={}",
                decrypted.len(),
                msg_len
            );
        }

        let msg = &decrypted[20..20 + msg_len];
        let receive_id = &decrypted[20 + msg_len..];

        tracing::info!(
            "WeCom decrypted successfully: msg_len={} receive_id={}",
            msg_len,
            String::from_utf8_lossy(receive_id)
        );

        Ok(String::from_utf8_lossy(msg).to_string())
    }

    #[allow(dead_code)]
    #[allow(dead_code)]
    fn encrypt(&self, plain_text: &str) -> anyhow::Result<String> {
        let Some(ref encoding_aes_key) = self.config.encoding_aes_key else {
            anyhow::bail!("WeCom encoding_aes_key not configured");
        };

        let alphabet = base64::alphabet::STANDARD;
        let config = general_purpose::GeneralPurposeConfig::new()
            .with_decode_padding_mode(DecodePaddingMode::Indifferent)
            .with_decode_allow_trailing_bits(true);
        let engine = GeneralPurpose::new(&alphabet, config);

        let key_to_decode = if encoding_aes_key.len() == 43 {
            format!("{encoding_aes_key}=")
        } else {
            encoding_aes_key.clone()
        };

        let aes_key_full = engine
            .decode(&key_to_decode)
            .or_else(|_| engine.decode(encoding_aes_key))?;

        if aes_key_full.len() < 32 {
            anyhow::bail!(
                "Invalid aes_key length: expected at least 32 bytes, got {}",
                aes_key_full.len()
            );
        }
        let aes_key = &aes_key_full[..32];

        let mut iv = [0u8; 16];
        iv.copy_from_slice(&aes_key[..16]);

        let random_bytes: [u8; 16] = rand::random();
        let msg_bytes = plain_text.as_bytes();
        let msg_len = msg_bytes.len() as u32;
        let receive_id = "";

        let mut data = Vec::with_capacity(20 + msg_bytes.len() + receive_id.len());
        data.extend_from_slice(&random_bytes);
        data.extend_from_slice(&msg_len.to_be_bytes());
        data.extend_from_slice(msg_bytes);
        data.extend_from_slice(receive_id.as_bytes());

        let padding_len = 32 - (data.len() % 32);
        data.extend(std::iter::repeat(padding_len as u8).take(padding_len));

        let data_len = data.len();
        let mut buffer = data;
        let encryptor = Aes256CbcEnc::new(aes_key.into(), &iv.into());
        use aes::cipher::block_padding::NoPadding;
        let ciphertext = encryptor
            .encrypt_padded_mut::<NoPadding>(&mut buffer, data_len)
            .map_err(|e| anyhow::anyhow!("AES encryption failed: {:?}", e))?;

        Ok(engine.encode(ciphertext))
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
                if let Err(e) = self.verify_signature(sig, ts, n, encrypt) {
                    tracing::error!("WeCom signature verification failed: {e}");
                    return vec![];
                }
            }

            match self.decrypt(encrypt) {
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

        let content = data
            .text
            .as_ref()
            .map(|t| t.content.clone())
            .unwrap_or_default();
        if content.is_empty() {
            return vec![];
        }

        vec![ChannelMessage {
            id: data.msgid,
            sender: data.from.userid,
            reply_target: data.response_url,
            content,
            channel: "wecom".to_string(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            thread_ts: data.chatid,
        }]
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

#[derive(Debug, serde::Deserialize)]
struct WeComPayload {
    msgid: String,
    chatid: Option<String>,
    from: WeComFrom,
    response_url: String,
    text: Option<WeComText>,
}

#[derive(Debug, serde::Deserialize)]
struct WeComFrom {
    userid: String,
}

#[derive(Debug, serde::Deserialize)]
struct WeComText {
    content: String,
}
