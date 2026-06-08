use std::path::Path;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub addr: String,
    pub port: u16,
    pub database_path: String,
    pub frontend_config_path: String,
    pub llm: LlmConfig,
    pub vision: VisionConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LlmConfig {
    pub api_base: String,
    pub api_key: Option<String>,
    pub model: String,
    #[serde(default)]
    pub think: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VisionConfig {
    pub api_base: String,
    pub api_key: Option<String>,
    pub model: String,
}

pub fn load(path: &Path) -> Config {
    let content = std::fs::read_to_string(path)
        .unwrap_or_else(|_| panic!("failed to read config file: {}", path.display()));
    toml::from_str(&content)
        .unwrap_or_else(|e| panic!("failed to parse config {}: {e}", path.display()))
}
