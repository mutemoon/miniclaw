use clap::{Parser, Subcommand};
use miniclaw::channels::traits::{Channel, ChannelMessage, SendMessage};
use miniclaw::channels::wecom::WeComChannel;
use miniclaw::config::schema::{AgentConfig, Config, WeComConfig};
use miniclaw::state::AgentEntry;
use miniclaw::utils::run_claude_process;
use rust_i18n::t;
rust_i18n::i18n!("locales");
use std::collections::HashMap;
use std::sync::Arc;
use tracing_subscriber::prelude::*;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Run the gateway service (default)
    Run,
    /// Guided configuration of Agent and its Channel
    Onboard,
}

fn detect_system_locale() -> String {
    // ... (rest of detect_system_locale remains the same)
    // 首先尝试使用 sys-locale 获取系统语言
    if let Some(locale) = sys_locale::get_locale() {
        // 对于 "C.UTF-8" 这样的情况，尝试从环境变量获取更多信息
        if &locale == "C.UTF-8" || &locale == "C" {
            // 尝试从 LANG 或 LC_* 环境变量获取更多信息
            if let Ok(lang) = std::env::var("LANG") {
                if lang.starts_with("zh") {
                    return "zh-CN".to_string();
                }
            }
            // 尝试其他常见的语言环境变量
            for var in ["LC_ALL", "LC_CTYPE", "LC_MESSAGES"] {
                if let Ok(lang) = std::env::var(var) {
                    if lang.starts_with("zh") {
                        return "zh-CN".to_string();
                    }
                }
            }
            // 如果没有找到中文相关的环境变量，则返回 "en"
            return "en".to_string();
        }

        // 如果检测到的语言包含 "zh"，则返回 "zh-CN"
        if locale.to_lowercase().starts_with("zh") {
            return "zh-CN".to_string();
        } else {
            // 否则返回语言代码的前两个字母，并尝试匹配我们的语言文件
            let lang_code = locale
                .split('-')
                .next()
                .unwrap_or("en")
                .split('_')
                .next()
                .unwrap_or("en");
            match lang_code {
                "zh" => "zh-CN".to_string(),
                "en" => "en".to_string(),
                // 可以在这里添加其他语言支持
                _ => "en".to_string(), // 默认为英语
            }
        }
    } else {
        // 如果 sys-locale 无法获取语言，则尝试从环境变量获取
        if let Ok(lang) = std::env::var("LANG") {
            if lang.starts_with("zh") {
                return "zh-CN".to_string();
            }
        }

        // 最后返回默认语言
        "en".to_string()
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

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

    let system_locale = detect_system_locale();
    rust_i18n::set_locale(&system_locale);

    match cli.command {
        Some(Commands::Onboard) => run_onboard().await?,
        Some(Commands::Run) | None => run_server().await?,
    }

    Ok(())
}

async fn run_onboard() -> anyhow::Result<()> {
    println!("{}", t!("onboard_welcome"));

    let agent_name = inquire::Text::new(&t!("onboard_agent_name").to_string())
        .with_default("my-agent")
        .prompt()?;

    let repo_path = inquire::Text::new(&t!("onboard_repo_path").to_string()).prompt()?;

    let channel_type = inquire::Select::new(&t!("onboard_channel_type").to_string(), vec!["WeCom"])
        .prompt()?;

    let mut agents = HashMap::new();

    if channel_type == "WeCom" {
        let bot_id = inquire::Text::new(&t!("onboard_wecom_bot_id").to_string()).prompt()?;
        let secret = inquire::Password::new(&t!("onboard_wecom_secret").to_string())
            .with_display_mode(inquire::PasswordDisplayMode::Masked)
            .prompt()?;

        agents.insert(
            agent_name,
            AgentConfig {
                repo: repo_path,
                wecom: Some(WeComConfig { bot_id, secret }),
            },
        );
    }

    let config = Config {
        server: Default::default(),
        agents,
    };

    let config_path = miniclaw::config::gateway_config_path();
    match miniclaw::config::save_config(&config_path, &config) {
        Ok(_) => {
            println!(
                "{}",
                t!("onboard_success", path = config_path.display().to_string())
            );
        }
        Err(e) => {
            eprintln!("{}", t!("onboard_error", error = e.to_string()));
        }
    }

    Ok(())
}

async fn run_server() -> anyhow::Result<()> {
    tracing::info!(
        "{}",
        t!("system_locale_detected", system_locale = rust_i18n::locale().to_string())
    );

    // 网关配置路径：~/.claude/claw/config.toml
    let config_path = miniclaw::config::gateway_config_path();
    let config = miniclaw::config::load_config(&config_path)?;

    // 构建 agents HashMap
    let mut agents: HashMap<String, AgentEntry> = HashMap::new();
    for (name, agent_cfg) in &config.agents {
        let wecom = if let Some(wecom_cfg) = &agent_cfg.wecom {
            tracing::info!("{}", t!("initializing_agent_wecom", name = name));
            let wecom = Arc::new(WeComChannel::new(wecom_cfg.clone()));

            let (tx, mut rx) = tokio::sync::mpsc::channel::<ChannelMessage>(100);
            let wecom_clone = wecom.clone();
            let repo = agent_cfg.repo.clone();
            let name_clone = name.clone();

            // 启动消息处理循环
            tokio::spawn(async move {
                while let Some(msg) = rx.recv().await {
                    let wecom = wecom_clone.clone();
                    let repo = repo.clone();
                    tracing::info!(
                        "{}",
                        t!(
                            "wecom_webhook_message",
                            sender = msg.sender,
                            content = msg.content
                        )
                    );
                    // 先回复“收到”
                    let wecom_ack = wecom.clone();
                    let reply_target = msg.reply_target.clone();
                    tokio::spawn(async move {
                        if let Err(e) = wecom_ack
                            .send(&SendMessage::new(t!("wecom_receiving_msg").to_string(), &reply_target))
                            .await
                        {
                            tracing::error!("Failed to send acknowledgement: {}", e);
                        }
                    });

                    tokio::spawn(async move {
                        match run_claude_process(&msg.content, &repo).await {
                            Ok(response) => {
                                if let Err(e) = wecom
                                    .send(&SendMessage::new(response, &msg.reply_target))
                                    .await
                                {
                                    tracing::error!(
                                        "{}",
                                        t!("failed_to_send_wecom_reply", error = e)
                                    );
                                }
                            }
                            Err(e) => {
                                tracing::error!(
                                    "{}",
                                    t!("claude_cli_error", error = e.to_string())
                                );
                            }
                        }
                    });
                }
            });

            // 启动企微长链接监听
            let wecom_listen = wecom.clone();
            tokio::spawn(async move {
                if let Err(e) = wecom_listen.listen(tx).await {
                    tracing::error!("WeCom listener for {} failed: {}", name_clone, e);
                }
            });

            Some(wecom)
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

    tracing::info!("{}", t!("server_ready"));

    // 等待 Ctrl+C 信号，防止程序直接退出导致背景任务被取消
    tokio::signal::ctrl_c().await?;
    tracing::info!("Shutting down...");

    Ok(())
}
