pub mod schema;

use anyhow::Context;
use rust_i18n::t;
use schema::{AgentConfig, Config, WeComConfig};
use std::path::{Path, PathBuf};

/// 返回全局网关配置路径：~/.claude/claw/config.toml
pub fn gateway_config_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".claude").join("claw").join("config.toml")
}

/// 返回仓库级 channel 配置路径：<repo>/.claude/claw/config.toml
fn repo_config_path(repo: &str) -> PathBuf {
    PathBuf::from(repo)
        .join(".claude")
        .join("claw")
        .join("config.toml")
}

pub fn load_config<P: AsRef<Path>>(path: P) -> anyhow::Result<Config> {
    let path = path.as_ref();

    if !path.exists() {
        anyhow::bail!(t!("failed_to_read_config"));
    }

    tracing::info!("{}", t!("loading_config"));
    tracing::info!("{}", path.display().to_string());
    let content = std::fs::read_to_string(path).context(t!("failed_to_read_config"))?;
    let mut config: Config = toml::from_str(&content).context(t!("failed_to_parse_config"))?;

    if let Ok(addr) = std::env::var("MINICLAW_ADDR") {
        tracing::info!("{}", t!("env_var_detected", var = "MINICLAW_ADDR"));
        config.server.addr = addr;
    }
    if let Ok(port) = std::env::var("MINICLAW_PORT") {
        if let Ok(port) = port.parse::<u16>() {
            tracing::info!("{}", t!("env_var_detected", var = "MINICLAW_PORT"));
            config.server.port = port;
        }
    }

    // 对每个 agent，尝试合并仓库级 channel 配置（仓库配置优先级更高）
    for (name, agent) in config.agents.iter_mut() {
        merge_repo_config(name, agent);
    }

    Ok(config)
}

pub fn save_config<P: AsRef<Path>>(path: P, config: &Config) -> anyhow::Result<()> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context(t!("failed_to_create_config_dir"))?;
    }

    let content = toml::to_string_pretty(config)?;
    std::fs::write(path, content).context(t!("failed_to_write_config"))?;
    Ok(())
}

/// 读取仓库级配置并合并到 agent 配置（仓库配置字段优先）
fn merge_repo_config(agent_name: &str, agent: &mut AgentConfig) {
    let repo_cfg_path = repo_config_path(&agent.repo);
    if !repo_cfg_path.exists() {
        return;
    }

    let content = match std::fs::read_to_string(&repo_cfg_path) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(
                "{}",
                t!(
                    "agent_repo_config_read_failed",
                    name = agent_name,
                    path = repo_cfg_path.display().to_string(),
                    error = e.to_string()
                )
            );
            return;
        }
    };

    #[derive(serde::Deserialize, Default)]
    struct RepoConfig {
        wecom: Option<WeComConfig>,
    }

    let repo_cfg: RepoConfig = match toml::from_str(&content) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(
                "{}",
                t!(
                    "agent_repo_config_parse_failed",
                    name = agent_name,
                    error = e.to_string()
                )
            );
            return;
        }
    };

    if let Some(repo_wecom) = repo_cfg.wecom {
        tracing::info!("{}", t!("agent_repo_wecom_override", name = agent_name));
        agent.wecom = Some(repo_wecom);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::{Config, ServerConfig, AgentConfig, WeComConfig};
    use std::collections::HashMap;

    #[test]
    fn test_save_and_load_config() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("config.toml");
        
        let mut agents = HashMap::new();
        agents.insert("test-agent".to_string(), AgentConfig {
            repo: "/tmp/repo".to_string(),
            wecom: Some(WeComConfig {
                bot_id: "test-bot".to_string(),
                secret: "test-secret".to_string(),
            }),
        });

        let config = Config {
            server: ServerConfig {
                addr: "127.0.0.1".to_string(),
                port: 8080,
            },
            agents,
        };

        save_config(&config_path, &config).unwrap();
        let loaded_config = load_config(&config_path).unwrap();

        assert_eq!(loaded_config.server.addr, "127.0.0.1");
        assert_eq!(loaded_config.server.port, 8080);
        assert!(loaded_config.agents.contains_key("test-agent"));
        let agent = loaded_config.agents.get("test-agent").unwrap();
        assert_eq!(agent.repo, "/tmp/repo");
        assert_eq!(agent.wecom.as_ref().unwrap().bot_id, "test-bot");
    }
}
