use axum::{Router, routing::get};
use miniclaw::handlers::wecom::{handle_wecom_verify, handle_wecom_webhook};
use miniclaw::state::{AgentEntry, AppState};
rust_i18n::i18n!("locales");
use miniclaw::wecom::WeComChannel;
use rust_i18n::t;
use std::collections::HashMap;
use std::sync::Arc;
use tracing_subscriber::prelude::*;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let log_dir = ".claude/miniclaw";
    std::fs::create_dir_all(log_dir)?;

    let file_appender = tracing_appender::rolling::never(log_dir, "miniclaw.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    let stdout_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stdout)
        .with_ansi(true)
        .with_target(false)
        .compact();

    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_target(false)
        .compact();

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::DEBUG.into()),
        )
        .with(stdout_layer)
        .with(file_layer)
        .init();

    let system_locale = sys_locale::get_locale().unwrap_or_else(|| "en".to_string());
    miniclaw::rust_i18n::set_locale(&system_locale);
    tracing::info!("{}", t!("system_locale_detected", locale = system_locale));

    // 网关配置路径：~/.claude/claw/config.toml
    let config_path = miniclaw::config::gateway_config_path();
    let config = miniclaw::config::load_config(&config_path)?;

    // 构建 agents HashMap
    let mut agents: HashMap<String, AgentEntry> = HashMap::new();
    for (name, agent_cfg) in &config.agents {
        let wecom = if let Some(wecom_cfg) = &agent_cfg.wecom {
            tracing::info!("{}", t!("initializing_agent_wecom", name = name));
            Some(Arc::new(WeComChannel::new(wecom_cfg.clone())))
        } else {
            None
        };

        agents.insert(
            name.clone(),
            AgentEntry {
                wecom,
                repo: agent_cfg.repo.clone(),
            },
        );
    }

    if agents.is_empty() {
        tracing::warn!(
            "{}",
            t!(
                "no_agents_configured",
                path = config_path.display().to_string()
            )
        );
    } else {
        tracing::info!("{}", t!("agents_loaded", count = agents.len()));
    }

    let state = AppState { agents };

    let app = Router::new()
        .route(
            "/wecom/:agent_name",
            get(handle_wecom_verify).post(handle_wecom_webhook),
        )
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .with_state(state);

    let addr = format!("{}:{}", config.server.addr, config.server.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("{}", t!("listening_on", addr = listener.local_addr()?));
    tracing::info!("{}", t!("server_ready"));
    axum::serve(listener, app).await?;

    Ok(())
}
