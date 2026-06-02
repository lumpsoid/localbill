//! Filesystem-backed domain repositories. These own all file enumeration and
//! reading; they return raw text and never parse front-matter — parsing stays
//! in the core.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::ports::{FailedLog, QueueStore, SchemaSource, StoredDoc, TransactionStore};

// ── Transaction store ───────────────────────────────────────────────────────────

/// File types this tool ever treats as transaction documents.
fn is_transaction_file(path: &Path) -> bool {
    path.is_file()
        && matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("md") | Some("yaml") | Some("yml")
        )
}

pub struct FsTransactionStore {
    dir: PathBuf,
}

impl FsTransactionStore {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }
}

impl TransactionStore for FsTransactionStore {
    fn list(&self) -> Result<Vec<StoredDoc>> {
        // Lenient: a missing default directory means "no transactions yet".
        if !self.dir.is_dir() {
            return Ok(Vec::new());
        }
        self.list_at(&self.dir)
    }

    fn list_at(&self, dir: &Path) -> Result<Vec<StoredDoc>> {
        if !dir.is_dir() {
            return Err(Error::Config(format!(
                "path does not exist: {}",
                dir.display()
            )));
        }
        let mut paths: Vec<PathBuf> = fs::read_dir(dir)?
            .flatten()
            .map(|e| e.path())
            .filter(|p| is_transaction_file(p))
            .collect();
        paths.sort();

        let mut docs = Vec::with_capacity(paths.len());
        for path in paths {
            // Skip files that can't be read, mirroring the old report/search
            // resilience.
            if let Ok(content) = fs::read_to_string(&path) {
                docs.push(StoredDoc { path, content });
            }
        }
        Ok(docs)
    }

    fn read(&self, path: &Path) -> Result<StoredDoc> {
        let content = fs::read_to_string(path)?;
        Ok(StoredDoc {
            path: path.to_path_buf(),
            content,
        })
    }

    fn exists(&self, filename: &str) -> bool {
        self.dir.join(filename).exists()
    }

    fn write_new(&self, filename: &str, content: &str) -> Result<PathBuf> {
        fs::create_dir_all(&self.dir)?;
        let path = self.dir.join(filename);
        fs::write(&path, content)?;
        Ok(path)
    }
}

// ── Local queue store ───────────────────────────────────────────────────────────

pub struct FsQueueStore {
    path: PathBuf,
}

impl FsQueueStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl QueueStore for FsQueueStore {
    fn list(&self) -> Result<Vec<String>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let content = fs::read_to_string(&self.path)?;
        Ok(content
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect())
    }

    fn enqueue(&self, url: &str) -> Result<()> {
        append_line(&self.path, url)
    }

    fn replace(&self, urls: &[String]) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = urls.join("\n");
        let content = if content.is_empty() {
            content
        } else {
            format!("{content}\n")
        };
        fs::write(&self.path, content)?;
        Ok(())
    }
}

// ── Failed-links log ────────────────────────────────────────────────────────────

pub struct FileFailedLog {
    path: PathBuf,
}

impl FileFailedLog {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl FailedLog for FileFailedLog {
    fn record(&self, url: &str) -> Result<()> {
        append_line(&self.path, url)
    }
}

// ── Schema source ───────────────────────────────────────────────────────────────

pub struct FileSchemaSource {
    path: Option<PathBuf>,
}

impl FileSchemaSource {
    pub fn new(path: Option<PathBuf>) -> Self {
        Self { path }
    }
}

impl SchemaSource for FileSchemaSource {
    fn load(&self) -> Result<String> {
        let path = self.path.as_ref().ok_or_else(|| {
            Error::Config(
                "schema_file is not configured; set schema_file in config.yaml \
                 (or the SCHEMA_FILE environment variable)"
                    .to_string(),
            )
        })?;
        fs::read_to_string(path)
            .map_err(|e| Error::Config(format!("Cannot read schema file {}: {e}", path.display())))
    }
}

// ── Shared helper ───────────────────────────────────────────────────────────────

/// Append a single line to `path`, creating parent directories as needed.
fn append_line(path: &Path, line: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{line}")?;
    Ok(())
}
