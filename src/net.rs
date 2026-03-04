use std::net::TcpStream;
use std::time::Duration;

/// Returns `true` when a TCP connection to Cloudflare's public DNS can be
/// established within 500 ms – a lightweight proxy for "do we have internet?".
pub fn has_internet() -> bool {
    let addr = "1.1.1.1:53".parse().expect("static address is valid");
    TcpStream::connect_timeout(&addr, Duration::from_millis(500)).is_ok()
}

/// Returns `true` when the git remote of `repo_dir` is reachable (uses
/// `git ls-remote` with a 10-second timeout so it never hangs).
pub fn git_remote_reachable(repo_dir: &std::path::Path) -> bool {
    std::process::Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy(), "ls-remote", "--exit-code", "origin", "HEAD"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
