use std::path::Path;
use std::process::Command;

use crate::cli::SyncArgs;
use crate::config::Config;
use crate::error::{Error, Result};
use crate::net;

pub fn run(args: SyncArgs, config: &Config) -> Result<()> {
    commit_and_push(
        &config.data_dir,
        None,
        args.message.as_deref(),
        !args.no_push,
    )
}

/// Core sync logic, also called by `insert` after writing files.
///
/// * `offline_prefix` – prepended to the commit message when offline
///   (pass `Some("Offline")` from internal callers, `None` to auto-detect).
/// * `user_message` – optional suffix supplied by the user.
/// * `push` – whether to push after committing.
pub fn commit_and_push(
    data_dir: &Path,
    offline_prefix: Option<&str>,
    user_message: Option<&str>,
    push: bool,
) -> Result<()> {
    if !data_dir.join(".git").exists() {
        return Err(Error::Git(format!(
            "DATA_DIR '{}' is not a git repository",
            data_dir.display()
        )));
    }

    // ── Connectivity check ────────────────────────────────────────────────────
    let online = if offline_prefix.is_some() {
        // Caller already determined connectivity (offline path from insert).
        false
    } else {
        let has_net = net::has_internet();
        if has_net {
            eprintln!("Internet detected, checking git remote…");
            net::git_remote_reachable(data_dir)
        } else {
            false
        }
    };

    // ── Pull ──────────────────────────────────────────────────────────────────
    if online {
        git(data_dir, &["pull"])?;
    }

    // ── Check for changes ─────────────────────────────────────────────────────
    let status = git_output(data_dir, &["status", "--porcelain"])?;
    if status.trim().is_empty() {
        println!("No changes in {}. Nothing to commit.", data_dir.display());
        return Ok(());
    }

    // ── Commit message ────────────────────────────────────────────────────────
    let now = current_timestamp();
    let prefix = offline_prefix.unwrap_or(if online { "" } else { "Offline " });
    let commit_msg = match user_message {
        Some(msg) => format!("{prefix}Data sync: {now} - {msg}"),
        None => format!("{prefix}Data sync: {now}"),
    };

    // ── Commit ────────────────────────────────────────────────────────────────
    git(data_dir, &["add", "."])?;
    git(data_dir, &["commit", "-m", &commit_msg])?;
    println!("Committed: {commit_msg}");

    // ── Push ──────────────────────────────────────────────────────────────────
    if !push || !online {
        if !online {
            eprintln!("Offline: changes committed locally but not pushed.");
        }
        return Ok(());
    }

    let branch = current_branch(data_dir)?;
    git(data_dir, &["push", "origin", &branch])?;
    println!("Pushed to origin/{branch}.");

    Ok(())
}

// ── Git helpers ───────────────────────────────────────────────────────────────

fn git(dir: &Path, args: &[&str]) -> Result<()> {
    let status = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .status()?;

    if !status.success() {
        return Err(Error::Git(format!(
            "`git {}` exited with {}",
            args.join(" "),
            status
        )));
    }
    Ok(())
}

fn git_output(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(Error::Git(format!(
            "`git {}` failed: {stderr}",
            args.join(" ")
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn current_branch(dir: &Path) -> Result<String> {
    let out = git_output(dir, &["symbolic-ref", "--short", "HEAD"])?;
    Ok(out.trim().to_string())
}

fn current_timestamp() -> String {
    // Use the `date` command to avoid pulling in a datetime crate.
    std::process::Command::new("date")
        .arg("+%Y-%m-%d %H:%M:%S")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}
