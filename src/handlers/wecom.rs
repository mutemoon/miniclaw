use crate::channels::traits::{Channel, SendMessage};
use crate::state::AppState;
use crate::utils::{run_claude_process, truncate_with_ellipsis};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use bytes::Bytes;
use rust_i18n::t;
use tracing::info;

#[derive(Debug, serde::Deserialize)]
pub struct WeComVerifyQuery {
    pub msg_signature: String,
    pub timestamp: String,
    pub nonce: String,
    pub echostr: Option<String>,
}

pub async fn handle_wecom_verify(
    State(state): State<AppState>,
    Path(agent_name): Path<String>,
    Query(params): Query<WeComVerifyQuery>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let locale = crate::i18n::get_locale_from_headers(&headers);
    rust_i18n::set_locale(&locale);

    let Some(entry) = state.agents.get(&agent_name) else {
        return (
            StatusCode::NOT_FOUND,
            t!("agent_not_found", name = agent_name).to_string(),
        );
    };

    let Some(ref wecom) = entry.wecom else {
        return (
            StatusCode::NOT_FOUND,
            t!("wecom_not_configured").to_string(),
        );
    };

    let Some(echostr) = params.echostr else {
        return (StatusCode::BAD_REQUEST, t!("missing_echostr").to_string());
    };

    match wecom.verify_url(
        &params.msg_signature,
        &params.timestamp,
        &params.nonce,
        &echostr,
    ) {
        Ok(plain_text) => {
            tracing::info!("{}", t!("wecom_verified_success"));
            (StatusCode::OK, plain_text)
        }
        Err(e) => {
            tracing::warn!("{}", t!("wecom_verification_failed", error = e));
            (StatusCode::FORBIDDEN, t!("forbidden").to_string())
        }
    }
}

pub async fn handle_wecom_webhook(
    State(state): State<AppState>,
    Path(agent_name): Path<String>,
    Query(query): Query<WeComVerifyQuery>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let locale = crate::i18n::get_locale_from_headers(&headers);
    rust_i18n::set_locale(&locale);

    info!("{}", t!("wecom_webhook_received"));

    let Some(entry) = state.agents.get(&agent_name) else {
        info!("{}", t!("agent_not_found", name = agent_name));
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": t!("agent_not_found", name = agent_name)})),
        );
    };

    let Some(ref wecom) = entry.wecom else {
        info!("{}", t!("wecom_not_configured"));
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": t!("wecom_not_configured")})),
        );
    };

    let Ok(payload) = serde_json::from_slice::<serde_json::Value>(&body) else {
        info!("{}", t!("invalid_json_payload"));
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": t!("invalid_json_payload")})),
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

    let repo = entry.repo.clone();
    for msg in messages {
        let wecom = wecom.clone();
        let locale = locale.clone();
        let repo = repo.clone();

        tokio::spawn(async move {
            rust_i18n::set_locale(&locale);

            tracing::info!(
                "{}",
                t!(
                    "wecom_webhook_message",
                    sender = msg.sender,
                    content = truncate_with_ellipsis(&msg.content, 50)
                )
            );

            match run_claude_process(&msg.content, &repo).await {
                Ok(response) => {
                    if let Err(e) = wecom
                        .send(
                            &SendMessage::new(response, &msg.reply_target)
                                .in_thread(msg.thread_ts.clone()),
                        )
                        .await
                    {
                        tracing::error!("{}", t!("failed_to_send_wecom_reply", error = e));
                    }
                }
                Err(e) => {
                    tracing::error!("{}", t!("claude_cli_error", error = e.to_string()));
                }
            }
        });
    }

    (StatusCode::OK, Json(serde_json::json!({"status": "ok"})))
}
