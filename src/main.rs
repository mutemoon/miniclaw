mod channels;
mod config;
mod handlers;
mod state;
mod utils;
mod wecom;

use axum::{Router, routing::get};
use handlers::wecom::{handle_wecom_verify, handle_wecom_webhook};
use state::AppState;
use std::sync::Arc;
use wecom::WeComChannel;

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
