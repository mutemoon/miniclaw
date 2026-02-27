use crate::channels::traits::ChannelMessage;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, serde::Deserialize)]
pub struct WeComPayload {
    pub msgid: String,
    pub chatid: Option<String>,
    pub from: WeComFrom,
    pub response_url: String,
    pub text: Option<WeComText>,
}

#[derive(Debug, serde::Deserialize)]
pub struct WeComFrom {
    pub userid: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct WeComText {
    pub content: String,
}

impl WeComPayload {
    pub fn to_channel_messages(&self) -> Vec<ChannelMessage> {
        let content = self
            .text
            .as_ref()
            .map(|t| t.content.clone())
            .unwrap_or_default();
        if content.is_empty() {
            return vec![];
        }

        vec![ChannelMessage {
            id: self.msgid.clone(),
            sender: self.from.userid.clone(),
            reply_target: self.response_url.clone(),
            content,
            channel: "wecom".to_string(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            thread_ts: self.chatid.clone(),
        }]
    }
}
