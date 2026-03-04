use std::fmt;
use std::path::PathBuf;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Http(Box<ureq::Error>),
    Json(serde_json::Error),
    Yaml(serde_yaml::Error),
    Parse(String),
    Config(String),
    Validation(Vec<ValidationError>),
    Git(String),
}

#[derive(Debug)]
pub struct ValidationError {
    pub path: PathBuf,
    pub field: String,
    pub message: String,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} → {}: {}", self.path.display(), self.field, self.message)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "IO error: {e}"),
            Error::Http(e) => write!(f, "HTTP error: {e}"),
            Error::Json(e) => write!(f, "JSON parse error: {e}"),
            Error::Yaml(e) => write!(f, "YAML parse error: {e}"),
            Error::Parse(msg) => write!(f, "Parse error: {msg}"),
            Error::Config(msg) => write!(f, "Config error: {msg}"),
            Error::Validation(errs) => {
                write!(f, "{} validation error(s)", errs.len())
            }
            Error::Git(msg) => write!(f, "Git error: {msg}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            Error::Json(e) => Some(e),
            Error::Yaml(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<ureq::Error> for Error {
    fn from(e: ureq::Error) -> Self {
        Error::Http(Box::new(e))
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Json(e)
    }
}

impl From<serde_yaml::Error> for Error {
    fn from(e: serde_yaml::Error) -> Self {
        Error::Yaml(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
