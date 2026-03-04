use std::path::PathBuf;

use serde::Deserialize;

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
    pub schema_file: Option<PathBuf>,
}

impl Config {
    pub fn api_base_url(&self) -> String {
        format!("http://{}:{}{}", self.api_host, self.api_port, self.api_endpoint)
    }
}

// ── YAML file schema ─────────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
struct ApiConfig {
    host: Option<String>,
    port: Option<u16>,
    endpoint: Option<String>,
}

/// Mirrors the structure of `~/.config/localbills/config.yaml`.
///
/// Example:
/// ```yaml
/// transaction_dir: ~/localbills-data
/// data_dir: ~/localbills-data
/// queue_file: ~/.local/share/localbills/queue.txt
/// failed_links_file: ~/.local/share/localbills/failed.txt
/// api:
///   host: 192.168.1.2
///   port: 8087
///   endpoint: /queue
/// schema_file: /path/to/schema.yaml
/// ```
#[derive(Deserialize, Default)]
struct ConfigFile {
    transaction_dir: Option<String>,
    data_dir: Option<String>,
    queue_file: Option<String>,
    failed_links_file: Option<String>,
    #[serde(default)]
    api: ApiConfig,
    schema_file: Option<String>,
}

// ── public loader ─────────────────────────────────────────────────────────────

/// Load configuration from the XDG config file (or the path supplied by the
/// user).  Environment variables always override file values.
pub fn load(override_path: Option<&std::path::Path>) -> Result<Config> {
    let config_path = match override_path {
        Some(p) => p.to_path_buf(),
        None => {
            let xdg = std::env::var("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|_| home_dir().join(".config"));
            xdg.join("localbills").join("config.yaml")
        }
    };

    let file: ConfigFile = if config_path.exists() {
        let content = std::fs::read_to_string(&config_path).map_err(|e| {
            Error::Config(format!("Cannot read {}: {e}", config_path.display()))
        })?;
        serde_yaml::from_str(&content).map_err(|e| {
            Error::Config(format!("Invalid YAML in {}: {e}", config_path.display()))
        })?
    } else {
        ConfigFile::default()
    };

    // Helper: env var first, then YAML file value.
    let env_or = |env_key: &str, file_val: Option<String>| -> Option<String> {
        std::env::var(env_key).ok().or(file_val)
    };

    let home = home_dir();
    let xdg_data = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home.join(".local").join("share"));

    let transaction_dir = env_or("TRANSACTION_DIR", file.transaction_dir)
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("localbills-data"));

    let data_dir = env_or("DATA_DIR", file.data_dir)
        .map(PathBuf::from)
        .unwrap_or_else(|| transaction_dir.clone());

    let queue_file = env_or("QUEUE_FILE", file.queue_file)
        .map(PathBuf::from)
        .unwrap_or_else(|| xdg_data.join("localbills").join("queue.txt"));

    let failed_links_file = env_or("FAILED_LINKS", file.failed_links_file)
        .map(PathBuf::from)
        .unwrap_or_else(|| xdg_data.join("localbills").join("failed.txt"));

    let api_host = env_or("API_HOST", file.api.host)
        .unwrap_or_else(|| "192.168.1.2".to_string());

    let api_port = std::env::var("API_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .or(file.api.port)
        .unwrap_or(8087u16);

    let api_endpoint = env_or("API_ENDPOINT", file.api.endpoint)
        .unwrap_or_else(|| "/queue".to_string());

    let schema_file = env_or("SCHEMA_FILE", file.schema_file).map(PathBuf::from);

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
