use std::path::Path;

use crate::cli::SyncArgs;
use crate::config::Config;
use crate::error::{Error, Result};
use crate::ports::{Clock, Network, Platform, RemoteReachable, Reporter, Vcs};

pub fn run<P: Platform>(args: SyncArgs, config: &Config, p: &P) -> Result<()> {
    commit_and_push(
        &config.data_dir,
        None,
        args.message.as_deref(),
        !args.no_push,
        false,
        p,
    )
}

/// Core sync logic, also called by `insert`/`add` after writing files.
///
/// * `offline_prefix` – prepended to the commit message when offline
///   (pass `Some("Offline")` from internal callers, `None` to auto-detect).
/// * `user_message` – optional suffix supplied by the user.
/// * `push` – whether to push after committing.
/// * `quiet` – suppress progress/result lines (used when `insert` drives its
///   own live progress display, which line output would corrupt).
pub fn commit_and_push<P: Platform>(
    data_dir: &Path,
    offline_prefix: Option<&str>,
    user_message: Option<&str>,
    push: bool,
    quiet: bool,
    p: &P,
) -> Result<()> {
    let vcs = p.vcs();
    let reporter = p.reporter();

    if !vcs.is_repo(data_dir) {
        return Err(Error::Git(format!(
            "DATA_DIR '{}' is not a git repository",
            data_dir.display()
        )));
    }

    // ── Connectivity check ────────────────────────────────────────────────────
    let online = if offline_prefix.is_some() {
        // Caller already determined connectivity (offline path from insert).
        false
    } else if p.network().has_internet() {
        if !quiet {
            reporter.status("Internet detected, checking git remote…");
        }
        p.remote().reachable(data_dir)
    } else {
        false
    };

    // ── Pull ──────────────────────────────────────────────────────────────────
    if online {
        vcs.pull(data_dir)?;
    }

    // ── Check for changes ─────────────────────────────────────────────────────
    if !vcs.is_dirty(data_dir)? {
        if !quiet {
            reporter.out(&format!(
                "No changes in {}. Nothing to commit.",
                data_dir.display()
            ));
        }
        return Ok(());
    }

    // ── Commit message ────────────────────────────────────────────────────────
    let now = p.clock().timestamp();
    let prefix = offline_prefix.unwrap_or(if online { "" } else { "Offline " });
    let commit_msg = match user_message {
        Some(msg) => format!("{prefix}Data sync: {now} - {msg}"),
        None => format!("{prefix}Data sync: {now}"),
    };

    // ── Commit ────────────────────────────────────────────────────────────────
    vcs.commit_all(data_dir, &commit_msg)?;
    if !quiet {
        reporter.out(&format!("Committed: {commit_msg}"));
    }

    // ── Push ──────────────────────────────────────────────────────────────────
    if !push || !online {
        if !online && !quiet {
            reporter.status("Offline: changes committed locally but not pushed.");
        }
        return Ok(());
    }

    let branch = vcs.current_branch(data_dir)?;
    vcs.push(data_dir, &branch)?;
    if !quiet {
        reporter.out(&format!("Pushed to origin/{branch}."));
    }

    Ok(())
}
