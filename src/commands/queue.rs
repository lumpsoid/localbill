use crate::cli::{InsertArgs, QueueArgs, QueueCommand};
use crate::config::Config;
use crate::error::{Error, Result};
use crate::ports::{Platform, QueueStore, RemoteQueue, Reporter};

pub fn run<P: Platform>(args: QueueArgs, config: &Config, p: &P) -> Result<()> {
    match args.command {
        QueueCommand::Add { url } => add(&url, p),
        QueueCommand::Remove { url } => remove(&url, p),
        QueueCommand::List => list(p),
        QueueCommand::Process { remote, no_sync } => {
            if remote {
                process_remote(config, no_sync, p)
            } else {
                process_local(config, no_sync, p)
            }
        }
    }
}

// ── add ───────────────────────────────────────────────────────────────────────

fn add<P: Platform>(url: &str, p: &P) -> Result<()> {
    p.queue().enqueue(url)?;
    p.reporter().status(&format!("Queued: {url}"));
    Ok(())
}

// ── remove ────────────────────────────────────────────────────────────────────

fn remove<P: Platform>(url: &str, p: &P) -> Result<()> {
    let lines = p.queue().list()?;
    let before = lines.len();
    let kept: Vec<String> = lines.into_iter().filter(|l| l != url).collect();
    let removed = before - kept.len();
    p.queue().replace(&kept)?;
    if removed == 0 {
        p.reporter()
            .status(&format!("URL not found in queue: {url}"));
    } else {
        p.reporter().out(&format!(
            "Removed {removed} occurrence(s) of the URL from the queue."
        ));
    }
    Ok(())
}

// ── list ──────────────────────────────────────────────────────────────────────

fn list<P: Platform>(p: &P) -> Result<()> {
    let lines = p.queue().list()?;
    let reporter = p.reporter();
    if lines.is_empty() {
        reporter.out("Queue is empty.");
    } else {
        for (i, line) in lines.iter().enumerate() {
            reporter.out(&format!("{:>4}. {line}", i + 1));
        }
        reporter.out(&format!("\n{} URL(s) in queue.", lines.len()));
    }
    Ok(())
}

// ── process local ─────────────────────────────────────────────────────────────

fn process_local<P: Platform>(config: &Config, no_sync: bool, p: &P) -> Result<()> {
    let urls = p.queue().list()?;
    let reporter = p.reporter();
    if urls.is_empty() {
        reporter.out("Queue is empty.");
        return Ok(());
    }

    reporter.out(&format!("Processing {} queued URL(s)…", urls.len()));
    let (succeeded, failed) = process_urls(&urls, no_sync, config, p);

    // Remove successfully-processed URLs from the queue.
    let remaining: Vec<String> = urls.into_iter().filter(|u| failed.contains(u)).collect();
    p.queue().replace(&remaining)?;

    reporter.out(&format!(
        "\nDone. {} succeeded, {} failed.",
        succeeded.len(),
        failed.len()
    ));
    if !failed.is_empty() {
        reporter.status("Failed URLs remain in the queue.");
        return Err(Error::Parse(format!("{} URL(s) failed", failed.len())));
    }
    Ok(())
}

// ── process remote (API) ──────────────────────────────────────────────────────

fn process_remote<P: Platform>(config: &Config, no_sync: bool, p: &P) -> Result<()> {
    let reporter = p.reporter();
    reporter.status(&format!("Fetching queue from {}…", config.api_base_url()));

    let urls = p.remote_queue().fetch()?;
    if urls.is_empty() {
        reporter.out("Remote queue is empty.");
        return Ok(());
    }

    reporter.out(&format!("Processing {} remote URL(s)…", urls.len()));
    let (succeeded, failed) = process_urls(&urls, no_sync, config, p);

    // Tell the API to remove the successfully-processed items.
    if !succeeded.is_empty() {
        p.remote_queue().remove(&succeeded)?;
    }

    reporter.out(&format!(
        "\nDone. {} succeeded, {} failed.",
        succeeded.len(),
        failed.len()
    ));
    if !failed.is_empty() {
        return Err(Error::Parse(format!("{} URL(s) failed", failed.len())));
    }
    Ok(())
}

/// Insert all URLs (parse + write each, then one git sync), returning the
/// (succeeded, failed) URL lists. Delegates the parse-then-sync flow to
/// `insert::run_batch` so queue processing syncs once, not per URL.
fn process_urls<P: Platform>(
    urls: &[String],
    no_sync: bool,
    config: &Config,
    p: &P,
) -> (Vec<String>, Vec<String>) {
    let reporter = p.reporter();
    let args = InsertArgs {
        url: None,
        file: None,
        dry_run: false,
        no_sync,
        force: false,
    };

    let outcomes = crate::commands::insert::run_batch(urls, &args, config, p).outcomes;

    let mut succeeded: Vec<String> = Vec::new();
    let mut failed: Vec<String> = Vec::new();
    for (url, outcome) in urls.iter().zip(&outcomes) {
        if outcome.is_failure() {
            reporter.status(&format!("  {url} … FAILED"));
            failed.push(url.clone());
        } else {
            reporter.status(&format!("  {url} … ok"));
            succeeded.push(url.clone());
        }
    }

    (succeeded, failed)
}
