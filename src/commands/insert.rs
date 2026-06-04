use std::fs;

use crate::cli::InsertArgs;
use crate::config::Config;
use crate::error::{Error, Result};
use crate::invoice::{mapper, parser};
use crate::ports::{
    FailedLog, Network, Platform, Progress, ProgressTask, QueueStore, Reporter, TransactionStore,
};

/// The result of attempting to load URLs from a file.
enum LoadedUrls {
    /// At least one URL was found.
    Found(Vec<String>),
    /// The file was read successfully but contained no usable URLs.
    Empty,
}

/// What happened to a single URL — the unit the final report is built from.
pub enum Outcome {
    /// Parsed and written. Carries the fields shown in the report one-liner.
    Saved {
        date: String,
        retailer: String,
        total: f64,
        currency: String,
        files: usize,
    },
    /// `--dry-run`: parsed and printed, nothing written.
    DryRun { date: String, retailer: String },
    /// Already recorded (duplicate); skipped.
    Skipped { url: String },
    /// Offline; the URL was queued for later.
    Queued { url: String },
    /// Parsing failed; the URL was recorded in the failed log.
    Failed { url: String },
}

impl Outcome {
    pub fn is_failure(&self) -> bool {
        matches!(self, Outcome::Failed { .. })
    }
}

/// Parse `contents` (a file of one URL per line), stripping blank lines and
/// `#`-comments. Pure — the file read happens in the caller.
fn parse_url_list(contents: &str) -> LoadedUrls {
    let urls: Vec<String> = contents
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_owned)
        .collect();

    if urls.is_empty() {
        LoadedUrls::Empty
    } else {
        LoadedUrls::Found(urls)
    }
}

pub fn run<P: Platform>(args: InsertArgs, config: &Config, p: &P) -> Result<()> {
    let reporter = p.reporter();

    let urls = match resolve_urls(&args, p)? {
        Some(urls) => urls,
        None => return Ok(()), // a file was given but held no URLs — nothing to do
    };
    let batch = args.file.is_some();

    // The whole flow: parse + write every URL, then one git sync at the end.
    // A single URL is just a batch of one — the pipeline never interleaves
    // parse and sync.
    let outcomes = run_batch(&urls, &args, config, p);
    reporter.out(&render_report(&outcomes, batch));

    let failed = outcomes.iter().filter(|o| o.is_failure()).count();
    if failed > 0 {
        let msg = if batch {
            format!("{failed} URL(s) failed to process")
        } else {
            "URL failed to process".to_string()
        };
        return Err(Error::Parse(msg));
    }
    Ok(())
}

/// Resolve the URLs this invocation will process: the lines of `--file`, or the
/// single positional URL. `Ok(None)` means a file was given but held no usable
/// URLs — the caller should stop without error.
fn resolve_urls<P: Platform>(args: &InsertArgs, p: &P) -> Result<Option<Vec<String>>> {
    if let Some(file_path) = &args.file {
        // The `--file` path is user-supplied and arbitrary, so it is read
        // directly here rather than through a store (the one remaining direct
        // read in the command layer).
        let contents = fs::read_to_string(file_path).map_err(Error::Io)?;
        match parse_url_list(&contents) {
            LoadedUrls::Empty => {
                p.reporter().status("No URLs found in file.");
                Ok(None)
            }
            LoadedUrls::Found(urls) => Ok(Some(urls)),
        }
    } else {
        let url = args.url.as_deref().unwrap_or("").trim().to_string();
        Ok(Some(vec![url]))
    }
}

/// Parse + write every URL (no per-URL sync), then run a single git sync for the
/// whole set. The unit of work shared by `insert` and `queue process`: many
/// parses, then one sync — never parse/sync interleaved.
pub fn run_batch<P: Platform>(
    urls: &[String],
    args: &InsertArgs,
    config: &Config,
    p: &P,
) -> Vec<Outcome> {
    let reporter = p.reporter();
    let mut outcomes = Vec::with_capacity(urls.len());
    for url in urls {
        match run_one(url, args, p) {
            Ok(outcome) => outcomes.push(outcome),
            Err(e) => {
                reporter.status(&format!("Error processing {url}: {e}"));
                outcomes.push(Outcome::Failed { url: url.clone() });
            }
        }
    }
    run_sync(&outcomes, args.no_sync, config, p);
    outcomes
}

/// Carry out the one git operation a finished batch warrants (per [`SyncPlan`]).
fn run_sync<P: Platform>(outcomes: &[Outcome], no_sync: bool, config: &Config, p: &P) {
    match SyncPlan::for_outcomes(outcomes, no_sync) {
        SyncPlan::Push => {
            if let Some(e) = sync_data(config, p, /*push=*/ true, None) {
                p.reporter().status(&format!("Warning: sync failed: {e}"));
            }
        }
        // Best-effort: commit whatever is pending locally while offline.
        SyncPlan::CommitOnly => {
            let _ = sync_data(config, p, /*push=*/ false, Some("Offline"));
        }
        SyncPlan::Nothing => {}
    }
}

/// Parse + write one URL. Performs no git sync — that is the batch's job (see
/// [`run_sync`]) so the flow stays parse-then-sync, never interleaved.
pub fn run_one<P: Platform>(url: &str, args: &InsertArgs, p: &P) -> Result<Outcome> {
    if url.is_empty() {
        return Err(Error::Parse("URL must not be empty".to_string()));
    }

    // ── Duplicate check ───────────────────────────────────────────────────────
    if !args.force && is_duplicate(url, p.transactions())? {
        return Ok(Outcome::Skipped {
            url: url.to_string(),
        });
    }

    // ── Offline → queue ───────────────────────────────────────────────────────
    if !p.network().has_internet() {
        p.queue().enqueue(url)?;
        return Ok(Outcome::Queued {
            url: url.to_string(),
        });
    }

    // ── Live progress checklist for the phases this run will perform ──────────
    let phases = run_phases(args);
    let task = p.progress().start(&phases);

    // ── Parse ─────────────────────────────────────────────────────────────────
    let invoice = match parser::parse(url, p.http()) {
        Ok(inv) => inv,
        Err(_) => {
            task.finish();
            p.failed().record(url)?;
            return Ok(Outcome::Failed {
                url: url.to_string(),
            });
        }
    };
    task.complete(); // Parse ✓

    // ── Dry-run: print and stop before writing ────────────────────────────────
    if args.dry_run {
        task.finish();
        mapper::print_invoice(&invoice, p.reporter());
        return Ok(Outcome::DryRun {
            date: invoice.date.clone(),
            retailer: invoice.retailer.clone(),
        });
    }

    // ── Write ─────────────────────────────────────────────────────────────────
    let written = mapper::write_invoice(&invoice, p.transactions())?;
    task.complete(); // Save ✓
    task.finish();

    Ok(Outcome::Saved {
        date: invoice.date,
        retailer: invoice.retailer,
        total: invoice.total_price,
        currency: invoice.currency,
        files: written.len(),
    })
}

// ── Sync decision (pure) ────────────────────────────────────────────────────────

/// The one git operation a finished batch warrants. The single home for the
/// sync decision — derived purely from the outcomes plus the `--no-sync` opt-out.
enum SyncPlan {
    /// Invoices were written and we're online: commit and push.
    Push,
    /// URLs were queued offline: commit pending changes without pushing.
    CommitOnly,
    /// Nothing to commit.
    Nothing,
}

impl SyncPlan {
    fn for_outcomes(outcomes: &[Outcome], no_sync: bool) -> Self {
        let any = |pred: fn(&Outcome) -> bool| outcomes.iter().any(pred);
        if no_sync {
            SyncPlan::Nothing
        } else if any(|o| matches!(o, Outcome::Saved { .. })) {
            SyncPlan::Push
        } else if any(|o| matches!(o, Outcome::Queued { .. })) {
            SyncPlan::CommitOnly
        } else {
            SyncPlan::Nothing
        }
    }
}

// ── Report rendering (pure) ─────────────────────────────────────────────────────

/// The phase labels shown in the live checklist for one URL. Sync is a
/// batch-level step (see [`run_sync`]), so it never appears here.
fn run_phases(args: &InsertArgs) -> Vec<&'static str> {
    if args.dry_run {
        vec!["Parse invoice"]
    } else {
        vec!["Parse invoice", "Save line items"]
    }
}

/// Commit (and optionally push) `data_dir` quietly. Returns any error. Pure
/// plumbing — the decision to call it lives in [`SyncPlan`].
fn sync_data<P: Platform>(
    config: &Config,
    p: &P,
    push: bool,
    message: Option<&str>,
) -> Option<Error> {
    crate::commands::sync::commit_and_push(
        &config.data_dir,
        message,
        None,
        push,
        /*quiet=*/ true,
        p,
    )
    .err()
}

/// Build the final report: a minimal one-liner per URL, written invoices first
/// sorted by date (oldest first), then skipped/queued/failed, plus a summary
/// footer in batch (`--file`) mode.
fn render_report(outcomes: &[Outcome], batch: bool) -> String {
    let is_dated = |o: &&Outcome| matches!(o, Outcome::Saved { .. } | Outcome::DryRun { .. });

    let mut dated: Vec<&Outcome> = outcomes.iter().filter(is_dated).collect();
    dated.sort_by(|a, b| date_key(a).cmp(date_key(b)));

    let mut lines: Vec<String> = dated.iter().map(|o| outcome_line(o)).collect();
    lines.extend(outcomes.iter().filter(|o| !is_dated(o)).map(outcome_line));

    if batch {
        lines.push(footer(outcomes));
    }
    lines.join("\n")
}

/// The ISO date used to sort dated outcomes (empty for the rest).
fn date_key(o: &Outcome) -> &str {
    match o {
        Outcome::Saved { date, .. } | Outcome::DryRun { date, .. } => date,
        _ => "",
    }
}

/// `"2024-03-15T14:30:00"` → `"2024-03-15 14:30"` for compact display.
fn short_datetime(iso: &str) -> String {
    iso.get(..16).unwrap_or(iso).replacen('T', " ", 1)
}

fn outcome_line(o: &Outcome) -> String {
    match o {
        Outcome::Saved {
            date,
            retailer,
            total,
            currency,
            files,
        } => {
            let unit = if *files == 1 { "file" } else { "files" };
            format!(
                "✓ {} · {} · {:.2} {} · {} {}",
                short_datetime(date),
                retailer,
                total,
                currency,
                files,
                unit
            )
        }
        Outcome::DryRun { date, retailer } => {
            format!(
                "· {} · {} · dry-run (not written)",
                short_datetime(date),
                retailer
            )
        }
        Outcome::Skipped { url } => format!("⚠ duplicate · {url}"),
        Outcome::Queued { url } => format!("⏸ queued (offline) · {url}"),
        Outcome::Failed { url } => format!("✗ failed · {url}"),
    }
}

fn footer(outcomes: &[Outcome]) -> String {
    let count = |pred: fn(&Outcome) -> bool| outcomes.iter().filter(|o| pred(o)).count();
    let saved = count(|o| matches!(o, Outcome::Saved { .. } | Outcome::DryRun { .. }));
    let skipped = count(|o| matches!(o, Outcome::Skipped { .. }));
    let queued = count(|o| matches!(o, Outcome::Queued { .. }));
    let failed = count(|o| matches!(o, Outcome::Failed { .. }));

    let mut footer = format!(
        "Processed {} · {} saved · {} skipped · {} failed",
        outcomes.len(),
        saved,
        skipped,
        failed
    );
    if queued > 0 {
        footer.push_str(&format!(" · {queued} queued"));
    }
    footer
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// True when `url` appears literally in any `.md` doc in the store.
fn is_duplicate(url: &str, store: &impl TransactionStore) -> Result<bool> {
    Ok(store
        .list()?
        .iter()
        .filter(|d| d.path.extension().and_then(|e| e.to_str()) == Some("md"))
        .any(|d| d.content.contains(url)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_urls() {
        let LoadedUrls::Found(urls) =
            parse_url_list("https://example.com/1\nhttps://example.com/2\n")
        else {
            panic!("expected Found");
        };
        assert_eq!(urls, ["https://example.com/1", "https://example.com/2"]);
    }

    #[test]
    fn skips_blank_lines_and_comments() {
        let LoadedUrls::Found(urls) = parse_url_list(
            "# comment\n\nhttps://example.com/1\n  \n# another\nhttps://example.com/2",
        ) else {
            panic!("expected Found");
        };
        assert_eq!(urls, ["https://example.com/1", "https://example.com/2"]);
    }

    #[test]
    fn trims_whitespace() {
        let LoadedUrls::Found(urls) = parse_url_list("  https://example.com/1  \n") else {
            panic!("expected Found");
        };
        assert_eq!(urls[0], "https://example.com/1");
    }

    #[test]
    fn returns_empty_for_blank_content() {
        assert!(matches!(
            parse_url_list("# just a comment\n\n   \n"),
            LoadedUrls::Empty
        ));
    }

    fn saved(date: &str, retailer: &str) -> Outcome {
        Outcome::Saved {
            date: date.to_string(),
            retailer: retailer.to_string(),
            total: 100.0,
            currency: "RSD".to_string(),
            files: 1,
        }
    }

    #[test]
    fn report_sorts_saved_by_date_oldest_first() {
        let outcomes = vec![
            saved("2024-03-15T14:30:00", "Maxi"),
            saved("2024-03-12T09:02:00", "Idea"),
        ];
        let report = render_report(&outcomes, false);
        let first = report.lines().next().unwrap();
        assert!(first.contains("Idea"), "oldest should be first: {report}");
        assert!(first.contains("2024-03-12 09:02"));
    }

    #[test]
    fn report_groups_non_saved_after_saved_with_footer() {
        let outcomes = vec![
            Outcome::Failed {
                url: "http://x/2".to_string(),
            },
            saved("2024-03-15T14:30:00", "Maxi"),
            Outcome::Skipped {
                url: "http://x/3".to_string(),
            },
        ];
        let report = render_report(&outcomes, true);
        let lines: Vec<&str> = report.lines().collect();
        assert!(lines[0].starts_with('✓')); // saved first
        assert!(lines[1].starts_with('⚠') || lines[1].starts_with('✗'));
        assert_eq!(
            *lines.last().unwrap(),
            "Processed 3 · 1 saved · 1 skipped · 1 failed"
        );
    }

    #[test]
    fn sync_plan_pushes_when_anything_written() {
        let outcomes = vec![
            Outcome::Failed { url: "x".into() },
            saved("2024-03-15T14:30:00", "Maxi"),
        ];
        assert!(matches!(
            SyncPlan::for_outcomes(&outcomes, /*no_sync=*/ false),
            SyncPlan::Push
        ));
    }

    #[test]
    fn sync_plan_commits_only_when_queued_offline() {
        let outcomes = vec![Outcome::Queued { url: "x".into() }];
        assert!(matches!(
            SyncPlan::for_outcomes(&outcomes, false),
            SyncPlan::CommitOnly
        ));
    }

    #[test]
    fn sync_plan_does_nothing_without_writes_or_queue() {
        let outcomes = vec![
            Outcome::Skipped { url: "x".into() },
            Outcome::Failed { url: "y".into() },
        ];
        assert!(matches!(
            SyncPlan::for_outcomes(&outcomes, false),
            SyncPlan::Nothing
        ));
    }

    #[test]
    fn sync_plan_respects_no_sync() {
        let outcomes = vec![saved("2024-03-15T14:30:00", "Maxi")];
        assert!(matches!(
            SyncPlan::for_outcomes(&outcomes, /*no_sync=*/ true),
            SyncPlan::Nothing
        ));
    }

    #[test]
    fn report_pluralises_files() {
        let outcomes = vec![Outcome::Saved {
            date: "2024-03-15T14:30:00".to_string(),
            retailer: "Maxi".to_string(),
            total: 179.98,
            currency: "RSD".to_string(),
            files: 2,
        }];
        assert!(render_report(&outcomes, false).contains("2 files"));
    }
}
