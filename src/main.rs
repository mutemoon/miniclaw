use axum::{Router, routing::get};
use miniclaw::handlers::wecom::{handle_wecom_verify, handle_wecom_webhook};
use miniclaw::state::AppState;
rust_i18n::i18n!("locales");
use miniclaw::wecom::WeComChannel;
use rust_i18n::t;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let system_locale = sys_locale::get_locale().unwrap_or_else(|| "en".to_string());
    miniclaw::rust_i18n::set_locale(&system_locale);
    tracing::info!("System locale detected: {}", system_locale);

    let config_path = ".claude/config.toml";
    let config = miniclaw::config::load_config(config_path)?;

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
    tracing::info!("{}", t!("listening_on", addr = listener.local_addr()?));
    axum::serve(listener, app).await?;

    Ok(())
}
