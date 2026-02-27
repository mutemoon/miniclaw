pub mod schema;

use anyhow::Context;
use schema::Config;
use std::path::Path;

pub fn load_config<P: AsRef<Path>>(path: P) -> anyhow::Result<Config> {
    let content = std::fs::read_to_string(path).context("Failed to read config file")?;
    let config: Config = toml::from_str(&content).context("Failed to parse config file")?;
    Ok(config)
}
