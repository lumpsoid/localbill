use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "localbill",
    about = "Local billing and invoice management",
    version,
    propagate_version = true
)]
pub struct Cli {
    /// Override the config file path (default: $XDG_CONFIG_HOME/localbills/config)
    #[arg(long, global = true, value_name = "PATH")]
    pub config: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Parse an invoice URL and save it to the transaction directory
    Insert(InsertArgs),

    /// Manage the invoice processing queue
    Queue(QueueArgs),

    /// Validate invoice files against the data schema
    Validate(ValidateArgs),

    /// Generate spending reports
    Report(ReportArgs),

    /// Search transactions
    Search(SearchArgs),

    /// Commit and sync the data directory with its git remote
    Sync(SyncArgs),
}

// ── insert ───────────────────────────────────────────────────────────────────

#[derive(Args)]
pub struct InsertArgs {
    /// Serbian fiscal invoice URL
    pub url: String,

    /// Print parsed files to stdout; do not write to disk
    #[arg(long)]
    pub dry_run: bool,

    /// Skip git sync after writing files
    #[arg(long)]
    pub no_sync: bool,

    /// Insert even if the URL has already been recorded
    #[arg(long)]
    pub force: bool,
}

// ── queue ────────────────────────────────────────────────────────────────────

#[derive(Args)]
pub struct QueueArgs {
    #[command(subcommand)]
    pub command: QueueCommand,
}

#[derive(Subcommand)]
pub enum QueueCommand {
    /// Add a URL to the local queue
    Add {
        /// Invoice URL to enqueue
        url: String,
    },

    /// Process every URL in the queue (local file or remote API)
    Process {
        /// Fetch the queue from the remote API instead of the local file
        #[arg(long)]
        remote: bool,

        /// Skip git sync after processing each invoice
        #[arg(long)]
        no_sync: bool,
    },

    /// List queued URLs
    List,

    /// Remove a URL from the local queue
    Remove {
        /// Invoice URL to dequeue
        url: String,
    },
}

// ── validate ─────────────────────────────────────────────────────────────────

#[derive(Args)]
pub struct ValidateArgs {
    /// File or directory to validate (defaults to TRANSACTION_DIR from config)
    pub path: Option<PathBuf>,

    /// Continue after the first validation error
    #[arg(long, short = 'c')]
    pub continue_on_error: bool,

    /// Print only files that contain errors (implies --continue-on-error)
    #[arg(long, short = 'e')]
    pub errors_only: bool,
}

// ── report ───────────────────────────────────────────────────────────────────

#[derive(Args)]
pub struct ReportArgs {
    #[command(subcommand)]
    pub command: ReportCommand,
}

#[derive(Subcommand)]
pub enum ReportCommand {
    /// Summarise spending month by month
    Monthly {
        /// Restrict to this calendar year (e.g. 2024)
        #[arg(long, short, value_name = "YEAR")]
        year: Option<u32>,

        /// Restrict to this month number (1-12)
        #[arg(long, short, value_name = "MONTH")]
        month: Option<u32>,
    },
}

// ── search ───────────────────────────────────────────────────────────────────

#[derive(Args)]
pub struct SearchArgs {
    #[command(subcommand)]
    pub command: SearchCommand,
}

#[derive(Subcommand)]
pub enum SearchCommand {
    /// Find transactions by product name (case-insensitive substring)
    Name {
        /// Search term
        query: String,
    },

    /// Report invoice URLs that appear in more than one transaction file
    Duplicates,
}

// ── sync ─────────────────────────────────────────────────────────────────────

#[derive(Args)]
pub struct SyncArgs {
    /// Custom suffix appended to the auto-generated commit message
    #[arg(long, short, value_name = "MSG")]
    pub message: Option<String>,

    /// Stage and commit changes without pushing to the remote
    #[arg(long)]
    pub no_push: bool,
}
