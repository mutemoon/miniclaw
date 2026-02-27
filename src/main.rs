mod channels;
mod config;
mod wecom;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use bytes::Bytes;
use channels::traits::{Channel, SendMessage};
use std::sync::Arc;
use wecom::WeComChannel;

#[derive(Clone)]
struct AppState {
    wecom: Option<Arc<WeComChannel>>,
}

fn truncate_with_ellipsis(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len])
    }
}

async fn run_claude_process(prompt: &str) -> anyhow::Result<String> {
    let output = tokio::process::Command::new("claude")
        .arg(prompt)
        .output()
        .await?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let err = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Claude process failed: {}", err)
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let config_path = ".claude/config.toml";
    let config = config::load_config(config_path)?;

    let wecom = config.channel.wecom.map(|c| Arc::new(WeComChannel::new(c)));

    let state = AppState { wecom };

    let app = Router::new()
        .route(
            "/wecom",
            get(handle_wecom_verify).post(handle_wecom_webhook),
        )
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    tracing::info!("Listening on {}", listener.local_addr()?);
    axum::serve(listener, app).await?;

    Ok(())
}

#[derive(Debug, serde::Deserialize)]
pub struct WeComVerifyQuery {
    pub msg_signature: String,
    pub timestamp: String,
    pub nonce: String,
    pub echostr: Option<String>,
}

async fn handle_wecom_verify(
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

async fn handle_wecom_webhook(
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
