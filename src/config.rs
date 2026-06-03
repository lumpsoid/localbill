use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{Error, Result};
use crate::ports::{Env, EnvVar};

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
        format!(
            "http://{}:{}{}",
            self.api_host, self.api_port, self.api_endpoint
        )
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
/// user). Reads the file from disk, then delegates merging to [`parse`].
///
/// This is the one place config touches the filesystem directly — it runs
/// before the [`crate::ports::Platform`] exists.
pub fn load<E: Env>(override_path: Option<&Path>, env: &E) -> Result<Config> {
    let config_path = match override_path {
        Some(p) => p.to_path_buf(),
        None => {
            let xdg = env
                .var(EnvVar::XdgConfigHome)
                .map(PathBuf::from)
                .unwrap_or_else(|| home_dir(env).join(".config"));
            xdg.join("localbills").join("config.yaml")
        }
    };

    let file_text =
        if config_path.exists() {
            Some(std::fs::read_to_string(&config_path).map_err(|e| {
                Error::Config(format!("Cannot read {}: {e}", config_path.display()))
            })?)
        } else {
            None
        };

    parse(file_text.as_deref(), env)
}

/// Merge the optional config-file text with environment-variable overrides into
/// a [`Config`]. Pure with respect to I/O (apart from the injected [`Env`]),
/// so it is directly unit-testable. Environment variables always win.
pub fn parse<E: Env>(file_text: Option<&str>, env: &E) -> Result<Config> {
    let file: ConfigFile = match file_text {
        Some(content) => serde_yaml::from_str(content)
            .map_err(|e| Error::Config(format!("Invalid config YAML: {e}")))?,
        None => ConfigFile::default(),
    };

    // Helper: env var first, then YAML file value.
    let env_or = |env_key: EnvVar, file_val: Option<String>| -> Option<String> {
        env.var(env_key).or(file_val)
    };

    let home = home_dir(env);
    let xdg_data = env
        .var(EnvVar::XdgDataHome)
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local").join("share"));

    let transaction_dir = env_or(EnvVar::TransactionDir, file.transaction_dir)
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("localbills-data"));

    let data_dir = env_or(EnvVar::DataDir, file.data_dir)
        .map(PathBuf::from)
        .unwrap_or_else(|| transaction_dir.clone());

    let queue_file = env_or(EnvVar::QueueFile, file.queue_file)
        .map(PathBuf::from)
        .unwrap_or_else(|| xdg_data.join("localbills").join("queue.txt"));

    let failed_links_file = env_or(EnvVar::FailedLinks, file.failed_links_file)
        .map(PathBuf::from)
        .unwrap_or_else(|| xdg_data.join("localbills").join("failed.txt"));

    let api_host =
        env_or(EnvVar::ApiHost, file.api.host).unwrap_or_else(|| "192.168.1.2".to_string());

    let api_port = env
        .var(EnvVar::ApiPort)
        .and_then(|s| s.parse().ok())
        .or(file.api.port)
        .unwrap_or(8087u16);

    let api_endpoint =
        env_or(EnvVar::ApiEndpoint, file.api.endpoint).unwrap_or_else(|| "/queue".to_string());

    let schema_file = env_or(EnvVar::SchemaFile, file.schema_file).map(PathBuf::from);

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

fn home_dir<E: Env>(env: &E) -> PathBuf {
    env.var(EnvVar::Home)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}
