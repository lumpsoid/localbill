//! `git`-subprocess adapter implementing both [`Vcs`] and [`RemoteReachable`].

use std::path::Path;
use std::process::{Command, Stdio};

use crate::error::{Error, Result};
use crate::ports::{RemoteReachable, Vcs};

pub struct GitCli;

impl GitCli {
    /// Run a git subcommand for its exit status only.
    fn run(&self, dir: &Path, args: &[&str]) -> Result<()> {
        let status = Command::new("git").arg("-C").arg(dir).args(args).status()?;
        if !status.success() {
            return Err(Error::Git(format!(
                "`git {}` exited with {}",
                args.join(" "),
                status
            )));
        }
        Ok(())
    }

    /// Run a git subcommand and capture its stdout.
    fn output(&self, dir: &Path, args: &[&str]) -> Result<String> {
        let out = Command::new("git").arg("-C").arg(dir).args(args).output()?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(Error::Git(format!(
                "`git {}` failed: {stderr}",
                args.join(" ")
            )));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }
}

impl Vcs for GitCli {
    fn is_repo(&self, dir: &Path) -> bool {
        dir.join(".git").exists()
    }

    fn pull(&self, dir: &Path) -> Result<()> {
        self.run(dir, &["pull"])
    }

    fn is_dirty(&self, dir: &Path) -> Result<bool> {
        let status = self.output(dir, &["status", "--porcelain"])?;
        Ok(!status.trim().is_empty())
    }

    fn commit_all(&self, dir: &Path, msg: &str) -> Result<()> {
        self.run(dir, &["add", "."])?;
        self.run(dir, &["commit", "-m", msg])
    }

    fn current_branch(&self, dir: &Path) -> Result<String> {
        Ok(self
            .output(dir, &["symbolic-ref", "--short", "HEAD"])?
            .trim()
            .to_string())
    }

    fn push(&self, dir: &Path, branch: &str) -> Result<()> {
        self.run(dir, &["push", "origin", branch])
    }
}

impl RemoteReachable for GitCli {
    /// Reachable when `git ls-remote` succeeds (10s implicit timeout, output
    /// suppressed so it never hangs interactively).
    fn reachable(&self, dir: &Path) -> bool {
        Command::new("git")
            .args([
                "-C",
                &dir.to_string_lossy(),
                "ls-remote",
                "--exit-code",
                "origin",
                "HEAD",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}
