use std::fs;
use std::path::Path;

use crate::cli::{InsertArgs, QueueArgs, QueueCommand};
use crate::config::Config;
use crate::error::{Error, Result};

pub fn run(args: QueueArgs, config: &Config) -> Result<()> {
    match args.command {
        QueueCommand::Add { url } => add(&url, &config.queue_file),
        QueueCommand::Remove { url } => remove(&url, &config.queue_file),
        QueueCommand::List => list(&config.queue_file),
        QueueCommand::Process { remote, no_sync } => {
            if remote {
                process_remote(config, no_sync)
            } else {
                process_local(config, no_sync)
            }
        }
    }
}

// ── add ───────────────────────────────────────────────────────────────────────

fn add(url: &str, queue_file: &Path) -> Result<()> {
    crate::commands::insert::queue_url(url, queue_file)
}

// ── remove ────────────────────────────────────────────────────────────────────

fn remove(url: &str, queue_file: &Path) -> Result<()> {
    let lines = read_queue(queue_file)?;
    let before = lines.len();
    let kept: Vec<String> = lines.into_iter().filter(|l| l != url).collect();
    let removed = before - kept.len();
    write_queue(queue_file, &kept)?;
    if removed == 0 {
        eprintln!("URL not found in queue: {url}");
    } else {
        println!("Removed {removed} occurrence(s) of the URL from the queue.");
    }
    Ok(())
}

// ── list ──────────────────────────────────────────────────────────────────────

fn list(queue_file: &Path) -> Result<()> {
    let lines = read_queue(queue_file)?;
    if lines.is_empty() {
        println!("Queue is empty.");
    } else {
        for (i, line) in lines.iter().enumerate() {
            println!("{:>4}. {line}", i + 1);
        }
        println!("\n{} URL(s) in queue.", lines.len());
    }
    Ok(())
}

// ── process local ─────────────────────────────────────────────────────────────

fn process_local(config: &Config, no_sync: bool) -> Result<()> {
    let urls = read_queue(&config.queue_file)?;
    if urls.is_empty() {
        println!("Queue is empty.");
        return Ok(());
    }

    println!("Processing {} queued URL(s)…", urls.len());
    let mut succeeded: Vec<String> = Vec::new();
    let mut failed: Vec<String> = Vec::new();

    for url in &urls {
        eprint!("  {url} … ");
        let args = InsertArgs {
            url: Some(url.clone()),
            file: None,
            dry_run: false,
            no_sync,
            force: false,
        };
        match crate::commands::insert::run(args, config) {
            Ok(()) => {
                eprintln!("ok");
                succeeded.push(url.clone());
            }
            Err(e) => {
                eprintln!("FAILED: {e}");
                failed.push(url.clone());
            }
        }
    }

    // Remove successfully-processed URLs from the queue.
    let remaining: Vec<String> = urls.into_iter().filter(|u| failed.contains(u)).collect();
    write_queue(&config.queue_file, &remaining)?;

    println!(
        "\nDone. {} succeeded, {} failed.",
        succeeded.len(),
        failed.len()
    );
    if !failed.is_empty() {
        eprintln!("Failed URLs remain in the queue.");
        return Err(Error::Parse(format!("{} URL(s) failed", failed.len())));
    }
    Ok(())
}

// ── process remote (API) ──────────────────────────────────────────────────────

fn process_remote(config: &Config, no_sync: bool) -> Result<()> {
    let api_url = config.api_base_url();
    eprintln!("Fetching queue from {api_url}…");

    let response: serde_json::Value = ureq::get(&api_url)
        .call()?
        .body_mut()
        .read_json::<serde_json::Value>()?;

    let items = response["items"]
        .as_array()
        .ok_or_else(|| Error::Parse("API response missing 'items' array".to_string()))?;

    if items.is_empty() {
        println!("Remote queue is empty.");
        return Ok(());
    }

    let urls: Vec<String> = items
        .iter()
        .filter_map(|v| v["item"].as_str().map(str::to_string))
        .collect();

    println!("Processing {} remote URL(s)…", urls.len());
    let mut succeeded: Vec<String> = Vec::new();
    let mut failed: Vec<String> = Vec::new();

    for url in &urls {
        eprint!("  {url} … ");
        let args = InsertArgs {
            url: Some(url.clone()),
            file: None,
            dry_run: false,
            no_sync,
            force: false,
        };
        match crate::commands::insert::run(args, config) {
            Ok(()) => {
                eprintln!("ok");
                succeeded.push(url.clone());
            }
            Err(e) => {
                eprintln!("FAILED: {e}");
                failed.push(url.clone());
            }
        }
    }

    // Tell the API to remove the successfully-processed items.
    if !succeeded.is_empty() {
        remove_from_remote_queue(config, &succeeded)?;
    }

    println!(
        "\nDone. {} succeeded, {} failed.",
        succeeded.len(),
        failed.len()
    );
    if !failed.is_empty() {
        return Err(Error::Parse(format!("{} URL(s) failed", failed.len())));
    }
    Ok(())
}

fn remove_from_remote_queue(config: &Config, urls: &[String]) -> Result<()> {
    let api_url = config.api_base_url();
    let body = serde_json::json!({ "items": urls });
    ureq::delete(&api_url)
        .force_send_body()
        .header("Content-Type", "application/json")
        .send_json(&body)?;
    Ok(())
}

// ── File I/O helpers ──────────────────────────────────────────────────────────

fn read_queue(path: &Path) -> Result<Vec<String>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(path)?;
    Ok(content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect())
}

fn write_queue(path: &Path, urls: &[String]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = urls.join("\n");
    let content = if content.is_empty() {
        content
    } else {
        format!("{content}\n")
    };
    fs::write(path, content)?;
    Ok(())
}
