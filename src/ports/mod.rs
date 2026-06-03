//! Capability ports: traits abstracting every external dependency.
//!
//! Each command is generic over a [`Platform`], a supertrait that bundles one
//! concrete adapter per capability via associated types.  Dependencies are
//! provided by **static dispatch** — generics + monomorphisation, never `dyn`.
//! Leaf functions take only the narrow trait they need (e.g. `&impl Http`).
//!
//! Production adapters live in [`crate::adapters`]; test fakes live in
//! [`crate::testing`].

use std::path::{Path, PathBuf};

use crate::error::Result;

/// A transaction file read from a store: its path and raw contents.
///
/// Stores deliberately return raw text — all parsing/projection stays in the
/// core (commands, `invoice` helpers) so it remains unit-testable.
#[derive(Debug, Clone)]
pub struct StoredDoc {
    pub path: PathBuf,
    pub content: String,
}

// ── External-system ports ──────────────────────────────────────────────────────

/// Synchronous HTTP client used by the invoice parser.
pub trait Http {
    /// GET `url` and return the response body as text.
    fn get_text(&self, url: &str) -> Result<String>;

    /// POST a `application/x-www-form-urlencoded` body to `url` and parse the
    /// JSON response.
    fn post_form(&self, url: &str, body: &str) -> Result<serde_json::Value>;
}

/// Version-control operations against a working tree.
pub trait Vcs {
    fn is_repo(&self, dir: &Path) -> bool;
    fn pull(&self, dir: &Path) -> Result<()>;
    /// True when the working tree has uncommitted changes
    /// (`git status --porcelain` is non-empty).
    fn is_dirty(&self, dir: &Path) -> Result<bool>;
    fn commit_all(&self, dir: &Path, msg: &str) -> Result<()>;
    fn current_branch(&self, dir: &Path) -> Result<String>;
    fn push(&self, dir: &Path, branch: &str) -> Result<()>;
}

/// Whether the configured git remote is reachable. Kept separate from [`Vcs`]
/// so callers/tests can depend on just this probe.
pub trait RemoteReachable {
    fn reachable(&self, dir: &Path) -> bool;
}

/// Coarse connectivity check.
pub trait Network {
    fn has_internet(&self) -> bool;
}

/// Wall-clock timestamp, formatted `"%Y-%m-%d %H:%M:%S"`.
pub trait Clock {
    fn timestamp(&self) -> String;
}

/// Interactive line input (used by the `add` command).
pub trait Prompt {
    fn read_line(&self, prompt: &str) -> Result<String>;
}

/// Sink for user-facing output, replacing direct `println!`/`eprintln!`.
pub trait Reporter {
    /// Primary command output (results, reports) — conceptually stdout.
    fn out(&self, line: &str);
    /// Progress and warnings — conceptually stderr.
    fn status(&self, msg: &str);
}

/// Semantic styles for interactive terminal output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    /// Section header / banner.
    Header,
    /// A field name.
    Field,
    /// Secondary / hint text.
    Hint,
    /// The input caret.
    Prompt,
    /// Confirmation of accepted input.
    Success,
    /// A validation error.
    Error,
}

/// Applies terminal styling to text. Implementations **must** return `text`
/// unchanged when output is not a terminal, so piped/redirected output stays
/// clean. Callers send the returned string through [`Reporter`]/[`Prompt`], so
/// colour stays behind the adapter boundary like every other capability.
pub trait Styler {
    fn paint(&self, style: Style, text: &str) -> String;
}

/// A live, multi-line phase checklist with one animated ("spinning") phase.
///
/// Pure terminal eye-candy: the production adapter draws a fidget-spinner and
/// redraws the block in place; when output is not a TTY it returns a no-op task
/// so piped/redirected output stays clean. All real results still flow through
/// [`Reporter`] — a `Progress` block is transient and erased on `finish`.
pub trait Progress {
    type Task: ProgressTask;
    /// Render `phases` as a checklist (the first phase starts active) and begin
    /// animating. The returned handle drives it; dropping or `finish`ing the
    /// handle stops the animation and erases the block.
    fn start(&self, phases: &[&str]) -> Self::Task;
}

/// Handle to an in-progress [`Progress`] checklist.
pub trait ProgressTask {
    /// Mark the active phase done (✓) and advance to the next one.
    fn complete(&self);
    /// Stop the animation and erase the checklist block.
    fn finish(self);
}

/// Process environment lookup (used by config loading).
pub trait Env {
    fn var(&self, key: &str) -> Option<String>;
}

// ── Domain repositories ─────────────────────────────────────────────────────────

/// The store of transaction `.md` files (rooted at `transaction_dir`).
pub trait TransactionStore {
    /// All transaction docs in the default directory.
    fn list(&self) -> Result<Vec<StoredDoc>>;
    /// All transaction docs under an arbitrary directory (`validate <dir>`).
    fn list_at(&self, dir: &Path) -> Result<Vec<StoredDoc>>;
    /// Read a single doc by path (`validate <file>`).
    fn read(&self, path: &Path) -> Result<StoredDoc>;
    /// Whether `filename` already exists in the default directory
    /// (used for collision-free name resolution).
    fn exists(&self, filename: &str) -> bool;
    /// Write a new doc `filename` in the default directory, creating it as
    /// needed. Returns the full path written.
    fn write_new(&self, filename: &str, content: &str) -> Result<PathBuf>;
}

/// The local offline queue of invoice URLs.
pub trait QueueStore {
    fn list(&self) -> Result<Vec<String>>;
    fn enqueue(&self, url: &str) -> Result<()>;
    /// Overwrite the queue with `urls` (used by remove / post-process rewrite).
    fn replace(&self, urls: &[String]) -> Result<()>;
}

/// The remote HTTP queue API.
pub trait RemoteQueue {
    fn fetch(&self) -> Result<Vec<String>>;
    fn remove(&self, urls: &[String]) -> Result<()>;
}

/// Append-only log of URLs that failed to parse.
pub trait FailedLog {
    fn record(&self, url: &str) -> Result<()>;
}

/// Source of the raw schema text (YAML) used by `validate` and `add`.
pub trait SchemaSource {
    fn load(&self) -> Result<String>;
}

// ── Composition ─────────────────────────────────────────────────────────────────

/// Bundle of every capability, provided by one adapter set.
///
/// Commands take a single `<P: Platform>` and reach dependencies through the
/// accessor methods; associated types pin the concrete adapters at compile
/// time so all dispatch is static.
pub trait Platform {
    type Http: Http;
    type Vcs: Vcs;
    type RemoteReachable: RemoteReachable;
    type Network: Network;
    type Clock: Clock;
    type Prompt: Prompt;
    type Reporter: Reporter;
    type Styler: Styler;
    type Progress: Progress;
    type Transactions: TransactionStore;
    type Queue: QueueStore;
    type RemoteQueue: RemoteQueue;
    type Failed: FailedLog;
    type Schema: SchemaSource;

    fn http(&self) -> &Self::Http;
    fn vcs(&self) -> &Self::Vcs;
    fn remote(&self) -> &Self::RemoteReachable;
    fn network(&self) -> &Self::Network;
    fn clock(&self) -> &Self::Clock;
    fn prompt(&self) -> &Self::Prompt;
    fn reporter(&self) -> &Self::Reporter;
    fn styler(&self) -> &Self::Styler;
    fn progress(&self) -> &Self::Progress;
    fn transactions(&self) -> &Self::Transactions;
    fn queue(&self) -> &Self::Queue;
    fn remote_queue(&self) -> &Self::RemoteQueue;
    fn failed(&self) -> &Self::Failed;
    fn schema(&self) -> &Self::Schema;
}
