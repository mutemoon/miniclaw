use crate::channels::traits::{Channel, SendMessage};
use crate::state::AppState;
use crate::utils::{run_claude_process, truncate_with_ellipsis};
use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use bytes::Bytes;

#[derive(Debug, serde::Deserialize)]
pub struct WeComVerifyQuery {
    pub msg_signature: String,
    pub timestamp: String,
    pub nonce: String,
    pub echostr: Option<String>,
}

pub async fn handle_wecom_verify(
    State(state): State<AppState>,
    Query(params): Query<WeComVerifyQuery>,
) -> impl IntoResponse {
    let Some(ref wecom) = state.wecom else {
        return (StatusCode::NOT_FOUND, "WeCom not configured".to_string());
    };

    let Some(echostr) = params.echostr else {
        return (StatusCode::BAD_REQUEST, "Missing echostr".to_string());
    };

    match wecom.verify_url(
        &params.msg_signature,
        &params.timestamp,
        &params.nonce,
        &echostr,
    ) {
        Ok(plain_text) => {
            tracing::info!("WeCom webhook verified successfully");
            (StatusCode::OK, plain_text)
        }
        Err(e) => {
            tracing::warn!("WeCom webhook verification failed: {e}");
            (StatusCode::FORBIDDEN, "Forbidden".to_string())
        }
    }
}

pub async fn handle_wecom_webhook(
    State(state): State<AppState>,
    Query(query): Query<WeComVerifyQuery>,
    body: Bytes,
) -> impl IntoResponse {
    let Some(ref wecom) = state.wecom else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "WeCom not configured"})),
        );
    };

    let Ok(payload) = serde_json::from_slice::<serde_json::Value>(&body) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Invalid JSON payload"})),
        );
    };

    let messages = wecom
        .parse_webhook_payload(
            &payload,
            Some(&query.msg_signature),
            Some(&query.timestamp),
            Some(&query.nonce),
        )
        .await;
    if messages.is_empty() {
        return (StatusCode::OK, Json(serde_json::json!({"status": "ok"})));
    }

    for msg in &messages {
        tracing::info!(
            "WeCom webhook message from {}: {}",
            msg.sender,
            truncate_with_ellipsis(&msg.content, 50)
        );

        match run_claude_process(&msg.content).await {
            Ok(response) => {
                if let Err(e) = wecom
                    .send(
                        &SendMessage::new(response, &msg.reply_target)
                            .in_thread(msg.thread_ts.clone()),
                    )
                    .await
                {
                    tracing::error!("Failed to send WeCom reply: {e}");
                }
            }
            Err(e) => {
                tracing::error!("Claude CLI error: {e:#}");
            }
        }
    }

    (StatusCode::OK, Json(serde_json::json!({"status": "ok"})))
}
