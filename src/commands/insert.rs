use std::fs;

use crate::cli::InsertArgs;
use crate::config::Config;
use crate::error::{Error, Result};
use crate::invoice::{mapper, parser};
use crate::ports::{
    FailedLog, Network, Platform, Progress, ProgressList, QueueStore, Reporter, RowState,
    TransactionStore,
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

/// The whole batch's result: every URL's [`Outcome`] plus the single sync step.
pub struct BatchResult {
    pub outcomes: Vec<Outcome>,
    sync: SyncReport,
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
    let urls = match resolve_urls(&args, p)? {
        Some(urls) => urls,
        None => return Ok(()), // a file was given but held no URLs — nothing to do
    };
    let batch = args.file.is_some();

    // The whole flow: parse + write every URL, then one git sync at the end.
    // A single URL is just a batch of one — the pipeline never interleaves
    // parse and sync.
    let result = run_batch(&urls, &args, config, p);
    p.reporter().out(&render_report(&result, batch));

    let failed = result.outcomes.iter().filter(|o| o.is_failure()).count();
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
///
/// Drives one live [`ProgressList`]: a row per URL (abbreviated link → in-place
/// summary), plus a trailing Sync row. The list is transient stderr eye-candy;
/// the durable report is rendered from the returned [`BatchResult`].
pub fn run_batch<P: Platform>(
    urls: &[String],
    args: &InsertArgs,
    config: &Config,
    p: &P,
) -> BatchResult {
    // Read the transaction directory ONCE, up front, and dedup against this
    // in-RAM snapshot for the whole batch.
    let mut dedup = Dedup::from_store(p.transactions()).unwrap_or_default();

    let labels = row_labels(urls, args.dry_run);
    let width = p.progress().width();
    let list = p.progress().start(&labels);
    let sync_idx = urls.len();

    let mut outcomes = Vec::with_capacity(urls.len());
    for (i, url) in urls.iter().enumerate() {
        list.activate(i);
        let outcome = match run_one(url, args, &dedup, p) {
            Ok(o) => o,
            Err(_) => {
                // run_one only errors on genuine I/O (queue/store) — treat the
                // URL as failed and surface it in the report, not mid-list.
                if !url.is_empty() {
                    let _ = p.failed().record(url);
                }
                Outcome::Failed { url: url.clone() }
            }
        };
        if matches!(outcome, Outcome::Saved { .. }) {
            dedup.record(url);
        }
        list.resolve(i, row_state(&outcome), &live_label(i + 1, &outcome, width));
        outcomes.push(outcome);
    }

    // One sync for the whole batch, shown as the final row.
    let sync = run_sync(&outcomes, args.no_sync, config, p);
    list.activate(sync_idx);
    list.resolve(sync_idx, sync.state, &sync.line);
    list.finish();

    BatchResult { outcomes, sync }
}

/// The sync row's outcome: its final state, its one-line label, and any captured
/// git output to surface (only present on failure).
struct SyncReport {
    state: RowState,
    line: String,
    error: Option<String>,
}

/// Carry out the one git operation a finished batch warrants (per [`SyncPlan`]),
/// mapping the result to the Sync row's [`SyncReport`].
fn run_sync<P: Platform>(
    outcomes: &[Outcome],
    no_sync: bool,
    config: &Config,
    p: &P,
) -> SyncReport {
    let ok = |line: &str| SyncReport {
        state: RowState::Ok,
        line: line.to_string(),
        error: None,
    };
    let failed = |e: Error| SyncReport {
        state: RowState::Fail,
        line: "Sync · failed".to_string(),
        error: Some(e.to_string()),
    };

    match SyncPlan::for_outcomes(outcomes, no_sync) {
        SyncPlan::Push => match sync_data(config, p, /*push=*/ true, None) {
            None => ok("Sync · pushed"),
            Some(e) => failed(e),
        },
        // Best-effort: commit whatever is pending locally while offline.
        SyncPlan::CommitOnly => match sync_data(config, p, /*push=*/ false, Some("Offline")) {
            None => ok("Sync · committed (offline)"),
            Some(e) => failed(e),
        },
        SyncPlan::Nothing => SyncReport {
            state: RowState::Skip,
            line: "Sync · nothing to commit".to_string(),
            error: None,
        },
    }
}

/// Parse + write one URL. Performs no git sync (that is the batch's job, see
/// [`run_sync`]) and no live-display or stdout/stderr output (the batch's
/// [`ProgressList`] owns the screen) — it only returns an [`Outcome`].
pub fn run_one<P: Platform>(url: &str, args: &InsertArgs, dedup: &Dedup, p: &P) -> Result<Outcome> {
    if url.is_empty() {
        return Err(Error::Parse("URL must not be empty".to_string()));
    }

    // ── Duplicate check (against the in-RAM snapshot) ─────────────────────────
    if !args.force && dedup.contains(url) {
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

    // ── Parse ─────────────────────────────────────────────────────────────────
    let invoice = match parser::parse(url, p.http()) {
        Ok(inv) => inv,
        Err(_) => {
            p.failed().record(url)?;
            return Ok(Outcome::Failed {
                url: url.to_string(),
            });
        }
    };

    // ── Dry-run: print and stop before writing ────────────────────────────────
    if args.dry_run {
        mapper::print_invoice(&invoice, p.reporter());
        return Ok(Outcome::DryRun {
            date: invoice.date.clone(),
            retailer: invoice.retailer.clone(),
        });
    }

    // ── Write ─────────────────────────────────────────────────────────────────
    let written = mapper::write_invoice(&invoice, p.transactions())?;

    Ok(Outcome::Saved {
        date: invoice.date,
        retailer: invoice.retailer,
        total: invoice.total_price,
        currency: invoice.currency,
        files: written.len(),
    })
}

// ── In-RAM duplicate index ──────────────────────────────────────────────────────

/// Snapshot of the transaction directory (read once) used to detect re-inserts.
///
/// Preserves the historical substring semantics — a URL is a duplicate when it
/// appears literally in any `.md` doc — but reads the directory a single time
/// per batch instead of once per URL. `session` tracks URLs saved earlier in the
/// same run so intra-batch duplicates are still caught.
#[derive(Default)]
pub struct Dedup {
    md_contents: Vec<String>,
    session: Vec<String>,
}

impl Dedup {
    /// Read every `.md` doc's content from the store once.
    pub fn from_store(store: &impl TransactionStore) -> Result<Self> {
        let md_contents = store
            .list()?
            .into_iter()
            .filter(|d| d.path.extension().and_then(|e| e.to_str()) == Some("md"))
            .map(|d| d.content)
            .collect();
        Ok(Self {
            md_contents,
            session: Vec::new(),
        })
    }

    /// True when `url` appears in any snapshot doc or was saved this run.
    fn contains(&self, url: &str) -> bool {
        self.md_contents.iter().any(|c| c.contains(url)) || self.session.iter().any(|u| u == url)
    }

    /// Note a URL just saved this run, so a later copy counts as a duplicate.
    fn record(&mut self, url: &str) {
        self.session.push(url.to_string());
    }
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

// ── Row / report rendering (pure) ───────────────────────────────────────────────

/// Compact a long URL to `<head>…<tail>` — the leading host chars plus the final
/// 8 — so rows stay a fixed, terminal-friendly width. Short URLs pass through.
fn abbreviate(url: &str) -> String {
    const HEAD: usize = 11; // e.g. "https://suf"
    const TAIL: usize = 8;
    let chars: Vec<char> = url.chars().collect();
    if chars.len() <= HEAD + TAIL + 1 {
        return url.to_string();
    }
    let head: String = chars[..HEAD].iter().collect();
    let tail: String = chars[chars.len() - TAIL..].iter().collect();
    format!("{head}…{tail}")
}

/// The pending labels for the live list: one abbreviated link per URL followed
/// by the Sync row. Empty in `--dry-run` (which prints invoices, so no list).
fn row_labels(urls: &[String], dry_run: bool) -> Vec<String> {
    if dry_run {
        return Vec::new();
    }
    let mut labels: Vec<String> = urls.iter().map(|u| abbreviate(u)).collect();
    labels.push("⟳ Sync".to_string());
    labels
}

/// The state (glyph/colour) a resolved row/report line takes for an outcome.
fn row_state(o: &Outcome) -> RowState {
    match o {
        Outcome::Saved { .. } => RowState::Ok,
        Outcome::DryRun { .. } => RowState::Skip,
        Outcome::Skipped { .. } | Outcome::Queued { .. } => RowState::Warn,
        Outcome::Failed { .. } => RowState::Fail,
    }
}

/// The plain report glyph mirroring a [`RowState`] (no ANSI — the live adapter
/// colours its own).
fn report_glyph(state: RowState) -> char {
    match state {
        RowState::Ok => '✓',
        RowState::Warn => '⚠',
        RowState::Fail => '✗',
        RowState::Skip => '·',
    }
}

/// `"2024-03-15T14:30:00"` → `"2024-03-15 14:30"` for compact display.
fn short_datetime(iso: &str) -> String {
    iso.get(..16).unwrap_or(iso).replacen('T', " ", 1)
}

/// The glyph-less label for row `n`'s outcome — shared by the live row (the
/// adapter prepends the coloured glyph) and the durable report line.
fn outcome_label(n: usize, o: &Outcome) -> String {
    match o {
        Outcome::Saved {
            date,
            retailer,
            total,
            currency,
            files,
        } => {
            let unit = if *files == 1 { "item" } else { "items" };
            format!(
                "#{n} · {} · {} · {:.2} {} · {} {}",
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
                "#{n} · {} · {} · dry-run (not written)",
                short_datetime(date),
                retailer
            )
        }
        Outcome::Skipped { url } => format!("#{n} duplicate · {}", abbreviate(url)),
        Outcome::Queued { url } => format!("#{n} queued (offline) · {}", abbreviate(url)),
        Outcome::Failed { url } => format!("#{n} failed · {}", abbreviate(url)),
    }
}

/// The widest visible prefix a live row can carry (indent + marker + space).
const LIVE_PREFIX: usize = 6;

/// The live-row label for row `n`, made to fit `width`. When the full one-liner
/// would overflow the terminal it drops to a dense form that keeps the essentials
/// (retailer · sum · count), so a wrapped row never breaks the in-place redraw.
/// The durable report still uses the full [`outcome_label`].
fn live_label(n: usize, o: &Outcome, width: Option<usize>) -> String {
    let full = outcome_label(n, o);
    let fits = width.is_none_or(|w| full.chars().count() + LIVE_PREFIX <= w);
    if fits {
        return full;
    }
    match o {
        // Drop the timestamp and currency; `k×` for the item count.
        Outcome::Saved {
            retailer,
            total,
            files,
            ..
        } => format!("#{n} · {} · {:.2} · {}×", retailer, total, files),
        // The other forms are already short; the adapter clamps if still tight.
        _ => full,
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

/// Build the final report: a one-liner per URL in input order (matching the live
/// list), then the Sync line (with any captured git error beneath it on
/// failure), plus a summary footer in batch (`--file`) mode.
fn render_report(result: &BatchResult, batch: bool) -> String {
    let mut lines: Vec<String> = result
        .outcomes
        .iter()
        .enumerate()
        .map(|(i, o)| format!("{} {}", report_glyph(row_state(o)), outcome_label(i + 1, o)))
        .collect();

    lines.push(format!(
        "{} {}",
        report_glyph(result.sync.state),
        result.sync.line
    ));
    if let Some(err) = &result.sync.error {
        lines.push(err.clone());
    }

    if batch {
        lines.push(footer(&result.outcomes));
    }
    lines.join("\n")
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

    fn batch_result(outcomes: Vec<Outcome>, sync: SyncReport) -> BatchResult {
        BatchResult { outcomes, sync }
    }

    fn no_sync() -> SyncReport {
        SyncReport {
            state: RowState::Skip,
            line: "Sync · nothing to commit".to_string(),
            error: None,
        }
    }

    #[test]
    fn live_label_full_when_wide_dense_when_narrow() {
        let o = saved("2024-03-15T14:30:00", "Maxi"); // total 100.0, RSD, 1 file

        // Plenty of width → full one-liner (timestamp + currency present).
        let wide = live_label(1, &o, Some(200));
        assert!(wide.contains("2024-03-15 14:30") && wide.contains("RSD"));

        // Narrow → dense form keeps retailer/sum/count, drops time + currency.
        let narrow = live_label(1, &o, Some(24));
        assert!(narrow.contains("Maxi") && narrow.contains("100.00") && narrow.contains('×'));
        assert!(!narrow.contains("RSD") && !narrow.contains("14:30"));

        // Unknown width behaves like wide (no clamping decision here).
        assert_eq!(live_label(1, &o, None), wide);
    }

    #[test]
    fn abbreviate_keeps_head_and_tail() {
        let url = "https://suf.purs.gov.rs/v/?vl=A9kZlongtokenQw3xY9Kp";
        assert_eq!(abbreviate(url), "https://suf…Qw3xY9Kp");
    }

    #[test]
    fn abbreviate_passes_short_urls_through() {
        assert_eq!(abbreviate("http://x/1"), "http://x/1");
    }

    #[test]
    fn dedup_detects_disk_and_session_hits() {
        let mut dedup = Dedup {
            md_contents: vec!["link: \"http://x/1\"".to_string()],
            session: Vec::new(),
        };
        assert!(dedup.contains("http://x/1"));
        assert!(!dedup.contains("http://x/2"));
        dedup.record("http://x/2");
        assert!(dedup.contains("http://x/2"));
    }

    #[test]
    fn report_lists_outcomes_in_input_order_with_sync_line() {
        let outcomes = vec![
            saved("2024-03-15T14:30:00", "Maxi"),
            saved("2024-03-12T09:02:00", "Idea"),
        ];
        let sync = SyncReport {
            state: RowState::Ok,
            line: "Sync · pushed".to_string(),
            error: None,
        };
        let report = render_report(&batch_result(outcomes, sync), true);
        let lines: Vec<&str> = report.lines().collect();
        // Input order: Maxi (#1) before Idea (#2), no date re-sort.
        assert!(lines[0].contains("#1") && lines[0].contains("Maxi"));
        assert!(lines[1].contains("#2") && lines[1].contains("Idea"));
        assert!(lines[2].contains("Sync · pushed"));
        assert_eq!(
            *lines.last().unwrap(),
            "Processed 2 · 2 saved · 0 skipped · 0 failed"
        );
    }

    #[test]
    fn report_surfaces_git_error_beneath_failed_sync() {
        let sync = SyncReport {
            state: RowState::Fail,
            line: "Sync · failed".to_string(),
            error: Some("`git push origin main` failed: auth denied".to_string()),
        };
        let report = render_report(
            &batch_result(vec![saved("2024-03-15T14:30:00", "Maxi")], sync),
            false,
        );
        assert!(report.contains("✗ Sync · failed"));
        assert!(report.contains("auth denied"));
    }

    #[test]
    fn report_pluralises_items() {
        let outcomes = vec![Outcome::Saved {
            date: "2024-03-15T14:30:00".to_string(),
            retailer: "Maxi".to_string(),
            total: 179.98,
            currency: "RSD".to_string(),
            files: 2,
        }];
        assert!(render_report(&batch_result(outcomes, no_sync()), false).contains("2 items"));
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
}
