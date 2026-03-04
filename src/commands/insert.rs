use std::fs;
use std::path::Path;

use crate::cli::InsertArgs;
use crate::config::Config;
use crate::error::{Error, Result};
use crate::invoice::{mapper, parser};
use crate::net;

pub fn run(args: InsertArgs, config: &Config) -> Result<()> {
    let url = args.url.trim().to_string();

    if url.is_empty() {
        return Err(Error::Parse("URL must not be empty".to_string()));
    }

    // ── Duplicate check ───────────────────────────────────────────────────────
    if !args.force && config.transaction_dir.exists() {
        if is_duplicate(&url, &config.transaction_dir) {
            eprintln!("Skipped: URL already recorded (use --force to override):\n  {url}");
            return Ok(());
        }
    }

    // ── Offline → queue ───────────────────────────────────────────────────────
    if !net::has_internet() {
        eprintln!("No internet connection – queuing URL for later processing.");
        queue_url(&url, &config.queue_file)?;
        // Best-effort offline sync (commits any pending local changes).
        let _ = crate::commands::sync::commit_and_push(
            &config.data_dir,
            Some("Offline"),
            None,
            /*push=*/ false,
        );
        return Ok(());
    }

    // ── Parse ─────────────────────────────────────────────────────────────────
    eprintln!("Parsing: {url}");
    let invoice = match parser::parse(&url) {
        Ok(inv) => inv,
        Err(e) => {
            eprintln!("Failed to parse invoice: {e}");
            record_failure(&url, &config.failed_links_file)?;
            return Err(e);
        }
    };

    // ── Write / dry-run ───────────────────────────────────────────────────────
    if args.dry_run {
        mapper::print_to_stdout(&invoice);
    } else {
        let written = mapper::write_to_dir(&invoice, &config.transaction_dir)?;
        println!("Wrote {} file(s).", written.len());

        // ── Sync ──────────────────────────────────────────────────────────────
        if !args.no_sync {
            if let Err(e) = crate::commands::sync::run(
                crate::cli::SyncArgs { message: None, no_push: false },
                config,
            ) {
                eprintln!("Warning: sync failed: {e}");
            }
        }
    }

    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// True when `url` appears literally in any file inside `dir`.
fn is_duplicate(url: &str, dir: &Path) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("md") {
            if let Ok(contents) = fs::read_to_string(&path) {
                if contents.contains(url) {
                    return true;
                }
            }
        }
    }
    false
}

/// Append `url` to the local queue file (creating parent directories as needed).
pub fn queue_url(url: &str, queue_file: &Path) -> Result<()> {
    if let Some(parent) = queue_file.parent() {
        fs::create_dir_all(parent)?;
    }
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(queue_file)?;
    writeln!(file, "{url}")?;
    eprintln!("Queued: {url}");
    Ok(())
}

/// Append `url` to the failed-links log file.
fn record_failure(url: &str, failed_file: &Path) -> Result<()> {
    if let Some(parent) = failed_file.parent() {
        fs::create_dir_all(parent)?;
    }
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(failed_file)?;
    writeln!(file, "{url}")?;
    Ok(())
}
