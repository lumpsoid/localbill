use std::collections::HashMap;
use std::path::PathBuf;

use crate::error::{Error, Result};

pub struct Config {
    pub transaction_dir: PathBuf,
    /// Git-backed data repository (may be the same as transaction_dir).
    pub data_dir: PathBuf,
    pub queue_file: PathBuf,
    pub failed_links_file: PathBuf,
    pub api_host: String,
    pub api_port: u16,
    pub api_endpoint: String,
    /// Path to the JSON Schema file (YAML or JSON) used by `localbill validate`.
    /// Set via `SCHEMA_FILE` in the config file or environment.
    pub schema_file: Option<PathBuf>,
}

impl Config {
    pub fn api_base_url(&self) -> String {
        format!("http://{}:{}{}", self.api_host, self.api_port, self.api_endpoint)
    }
}

/// Load configuration from the XDG config file (or the path supplied by the user).
///
/// Variables are read from the file first, then overridden by the environment,
/// so environment variables always take precedence – the same behaviour as the
/// original shell `config_loader.sh`.
pub fn load(override_path: Option<&std::path::Path>) -> Result<Config> {
    let config_path = match override_path {
        Some(p) => p.to_path_buf(),
        None => {
            let xdg = std::env::var("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|_| home_dir().join(".config"));
            xdg.join("localbills").join("config")
        }
    };

    let mut vars: HashMap<String, String> = HashMap::new();

    if config_path.exists() {
        let content = std::fs::read_to_string(&config_path).map_err(|e| {
            Error::Config(format!("Cannot read {}: {e}", config_path.display()))
        })?;
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim().to_string();
                // Strip surrounding quotes from value
                let value = value
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'')
                    .to_string();
                vars.insert(key, value);
            }
        }
    }

    // Helper: file-vars first, then env override.
    let get = |key: &str| -> Option<String> {
        std::env::var(key).ok().or_else(|| vars.get(key).cloned())
    };

    let home = home_dir();
    let xdg_data = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home.join(".local").join("share"));

    let transaction_dir = get("TRANSACTION_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("localbills-data"));

    let data_dir = get("DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| transaction_dir.clone());

    let queue_file = get("QUEUE_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| xdg_data.join("localbills").join("queue.txt"));

    let failed_links_file = get("FAILED_LINKS")
        .map(PathBuf::from)
        .unwrap_or_else(|| xdg_data.join("localbills").join("failed.txt"));

    let api_host = get("API_HOST").unwrap_or_else(|| "192.168.1.2".to_string());
    let api_port = get("API_PORT")
        .and_then(|s| s.parse().ok())
        .unwrap_or(8087u16);
    let api_endpoint = get("API_ENDPOINT").unwrap_or_else(|| "/queue".to_string());

    let schema_file = get("SCHEMA_FILE").map(PathBuf::from);

    Ok(Config {
        transaction_dir,
        data_dir,
        queue_file,
        failed_links_file,
        api_host,
        api_port,
        api_endpoint,
        schema_file,
    })
}

fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}
