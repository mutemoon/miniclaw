use crate::channels::traits::{Channel, ChannelMessage, SendMessage};
use crate::config::schema::WeComConfig;
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{Mutex, mpsc};
use tokio::time::interval;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::protocol::Message;
use tracing::{debug, error, info, trace, warn};

const WS_URL: &str = "wss://openws.work.weixin.qq.com";

#[derive(Debug, Serialize, Deserialize)]
struct WsFrame {
    #[serde(skip_serializing_if = "Option::is_none")]
    cmd: Option<String>,
    headers: WsHeaders,
    #[serde(skip_serializing_if = "Option::is_none")]
    body: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    errcode: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    errmsg: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct WsHeaders {
    req_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct SubscribeBody {
    bot_id: String,
    secret: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct CallbackBody {
    msgid: String,
    msgtype: String,
    #[serde(default)]
    sender: Option<WeComSender>,
    #[serde(default)]
    text: Option<WeComText>,
    #[serde(default)]
    event: Option<WeComEvent>,
}

#[derive(Debug, Serialize, Deserialize)]
struct WeComSender {
    senderid: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct WeComEvent {
    eventtype: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct WeComText {
    content: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct RespondBody {
    msgtype: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    markdown: Option<MarkdownContent>,
}

#[derive(Debug, Serialize, Deserialize)]
struct MarkdownContent {
    pub content: String,
}

pub struct WeComChannel {
    pub config: WeComConfig,
    tx_ws: Arc<Mutex<Option<mpsc::Sender<WsFrame>>>>,
}

impl WeComChannel {
    pub fn new(config: WeComConfig) -> Self {
        Self {
            config,
            tx_ws: Arc::new(Mutex::new(None)),
        }
    }

    fn generate_req_id(prefix: &str) -> String {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let rand: u32 = rand::random();
        format!("{}_{}_{}", prefix, ts, rand)
    }
}

#[async_trait]
impl Channel for WeComChannel {
    fn name(&self) -> &str {
        "wecom"
    }

    async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
        let tx = self.tx_ws.lock().await;
        if let Some(tx) = tx.as_ref() {
            let frame = WsFrame {
                cmd: Some("aibot_respond_msg".to_string()),
                headers: WsHeaders {
                    req_id: message.recipient.clone(), // Use stored req_id as recipient
                },
                body: Some(serde_json::to_value(RespondBody {
                    msgtype: "markdown".to_string(),
                    markdown: Some(MarkdownContent {
                        content: message.content.clone(),
                    }),
                })?),
                errcode: None,
                errmsg: None,
            };
            tx.send(frame).await.map_err(|e| anyhow::anyhow!(e))?;
            Ok(())
        } else {
            anyhow::bail!("WebSocket connection not active")
        }
    }

    async fn listen(&self, tx: mpsc::Sender<ChannelMessage>) -> anyhow::Result<()> {
        let mut retry_count = 0;
        let max_retry_delay = Duration::from_secs(30);
        let base_retry_delay = Duration::from_secs(1);

        loop {
            info!("Connecting to WeCom WebSocket: {}", WS_URL);
            let (ws_stream, _) = match connect_async(WS_URL).await {
                Ok(v) => {
                    retry_count = 0; // Reset retry count on successful connection
                    v
                }
                Err(e) => {
                    let delay = std::cmp::min(
                        base_retry_delay * 2u32.pow(retry_count),
                        max_retry_delay,
                    );
                    error!("Failed to connect to WeCom WS: {}. Retrying in {:?}...", e, delay);
                    tokio::time::sleep(delay).await;
                    retry_count += 1;
                    continue;
                }
            };

            let (mut ws_write, mut ws_read) = ws_stream.split();

            // Internal channel for sending frames to WS
            let (tx_ws_internal, mut rx_ws_internal) = mpsc::channel::<WsFrame>(100);
            {
                let mut guard = self.tx_ws.lock().await;
                *guard = Some(tx_ws_internal);
            }

            // 1. Subscribe
            let req_id = Self::generate_req_id("aibot_subscribe");
            let sub_frame = WsFrame {
                cmd: Some("aibot_subscribe".to_string()),
                headers: WsHeaders {
                    req_id: req_id.clone(),
                },
                body: Some(serde_json::to_value(SubscribeBody {
                    bot_id: self.config.bot_id.clone(),
                    secret: self.config.secret.clone(),
                })?),
                errcode: None,
                errmsg: None,
            };

            ws_write
                .send(Message::Text(serde_json::to_string(&sub_frame)?.into()))
                .await?;
            info!("Sent subscription request: {}", req_id);

            // 2. Heartbeat loop components
            let mut heartbeat_interval = interval(Duration::from_secs(30));
            let mut missed_pong_count = 0;
            let max_missed_pongs = 2;
            let mut stop_reconnecting = false;

            loop {
                tokio::select! {
                    _ = heartbeat_interval.tick() => {
                        if missed_pong_count >= max_missed_pongs {
                            warn!("No heartbeat ack received for {} consecutive pings, connection considered dead", missed_pong_count);
                            break;
                        }

                        missed_pong_count += 1;
                        let ping_req_id = Self::generate_req_id("ping");
                        let ping_frame = WsFrame {
                            cmd: Some("ping".to_string()),
                            headers: WsHeaders { req_id: ping_req_id },
                            body: None,
                            errcode: None,
                            errmsg: None,
                        };
                        trace!("Sending heartbeat (ping), missed_pong_count: {}", missed_pong_count);
                        if let Err(e) = ws_write.send(Message::Text(serde_json::to_string(&ping_frame)?.into())).await {
                            error!("Failed to send heartbeat: {}", e);
                            break;
                        }
                    }
                    Some(frame_to_send) = rx_ws_internal.recv() => {
                         if let Err(e) = ws_write.send(Message::Text(serde_json::to_string(&frame_to_send)?.into())).await {
                            error!("Failed to send frame via WS: {}", e);
                            break;
                        }
                    }
                    msg = ws_read.next() => {
                        match msg {
                            Some(Ok(Message::Text(text))) => {
                                let frame: WsFrame = match serde_json::from_str(&text) {
                                    Ok(f) => f,
                                    Err(e) => {
                                        error!("Failed to parse WS frame: {}", e);
                                        continue;
                                    }
                                };

                                if let Some(cmd) = &frame.cmd {
                                    match cmd.as_str() {
                                        "aibot_msg_callback" => {
                                            if let Some(body) = frame.body {
                                                if let Ok(callback) = serde_json::from_value::<CallbackBody>(body) {
                                                    let content = match callback.msgtype.as_str() {
                                                        "text" => callback.text.map(|t| t.content).unwrap_or_default(),
                                                        _ => String::from("Unsupported message type"),
                                                    };
                                                    let msg = ChannelMessage {
                                                        id: callback.msgid,
                                                        sender: callback.sender.map(|s| s.senderid).unwrap_or_else(|| "unknown".to_string()),
                                                        reply_target: frame.headers.req_id.clone(), // Use req_id for reply
                                                        content,
                                                        channel: "wecom".to_string(),
                                                        timestamp: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
                                                        thread_ts: None,
                                                    };
                                                    let _ = tx.send(msg).await;
                                                }
                                            }
                                        }
                                        "aibot_event_callback" => {
                                            if let Some(body) = &frame.body {
                                                if let Some(event) = body.get("event") {
                                                    if let Some(event_type) = event.get("eventtype") {
                                                        if event_type == "disconnected_event" {
                                                            warn!("Received disconnected_event: a new connection has been established, this connection will be closed by server. Stopping reconnection to avoid conflict.");
                                                            stop_reconnecting = true;
                                                            break;
                                                        }
                                                    }
                                                }
                                            }
                                            debug!("Received event: {:?}", frame.body);
                                        }
                                        _ => debug!("Unhandled command: {}", cmd),
                                    }
                                } else {
                                    // Response to our requests
                                    if frame.headers.req_id.starts_with("ping") {
                                        trace!("Received heartbeat ack (pong)");
                                        missed_pong_count = 0;
                                    }

                                    if let Some(errcode) = frame.errcode {
                                        if errcode != 0 {
                                            error!("Error response from WeCom: {} ({:?})", errcode, frame.errmsg);
                                            if frame.headers.req_id.starts_with("aibot_subscribe") {
                                                error!("Subscription failed. Closing connection.");
                                                break;
                                            }
                                        } else {
                                            if frame.headers.req_id.starts_with("aibot_subscribe") {
                                                info!("Subscription successful");
                                            }
                                        }
                                    }
                                }
                            }
                            Some(Ok(Message::Close(cf))) => {
                                warn!("WeCom WS connection closed: {:?}. Reconnecting...", cf);
                                break;
                            }
                            None => {
                                warn!("WeCom WS stream returned None. Reconnecting...");
                                break;
                            }
                            Some(Err(e)) => {
                                error!("WS Error: {}. Reconnecting...", e);
                                break;
                            }
                            _ => {}
                        }
                    }
                }
            }

            // Clean up
            {
                let mut guard = self.tx_ws.lock().await;
                *guard = None;
            }

            if stop_reconnecting {
                error!("Conflict detected. Exiting listener loop.");
                return Ok(());
            }

            let delay = std::cmp::min(
                base_retry_delay * 2u32.pow(retry_count),
                max_retry_delay,
            );
            error!("WS connection lost. Retrying in {:?}...", delay);
            tokio::time::sleep(delay).await;
            retry_count += 1;
        }
    }
}
