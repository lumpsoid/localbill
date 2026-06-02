use std::fs;

use crate::cli::{InsertArgs, SyncArgs};
use crate::config::Config;
use crate::error::{Error, Result};
use crate::invoice::{mapper, parser};
use crate::ports::{FailedLog, Network, Platform, QueueStore, Reporter, TransactionStore};

/// The result of attempting to load URLs from a file.
enum LoadedUrls {
    /// At least one URL was found.
    Found(Vec<String>),
    /// The file was read successfully but contained no usable URLs.
    Empty,
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

    if let Some(file_path) = &args.file {
        // The `--file` path is user-supplied and arbitrary, so it is read
        // directly here rather than through a store (the one remaining direct
        // read in the command layer).
        let contents = fs::read_to_string(file_path).map_err(Error::Io)?;
        let urls = match parse_url_list(&contents) {
            LoadedUrls::Empty => {
                reporter.status("No URLs found in file.");
                return Ok(());
            }
            LoadedUrls::Found(urls) => urls,
        };

        reporter.status(&format!("Processing {} URL(s) from file.", urls.len()));
        let mut errors = 0usize;
        for url in &urls {
            if let Err(e) = run_one(url, &args, config, p) {
                reporter.status(&format!("Error processing {url}: {e}"));
                errors += 1;
            }
        }
        if errors > 0 {
            return Err(Error::Parse(format!("{errors} URL(s) failed to process")));
        }
        Ok(())
    } else {
        let url = args.url.as_deref().unwrap_or("").trim().to_string();
        run_one(&url, &args, config, p)
    }
}

pub fn run_one<P: Platform>(url: &str, args: &InsertArgs, config: &Config, p: &P) -> Result<()> {
    let reporter = p.reporter();

    if url.is_empty() {
        return Err(Error::Parse("URL must not be empty".to_string()));
    }

    // ── Duplicate check ───────────────────────────────────────────────────────
    if !args.force && is_duplicate(url, p.transactions())? {
        reporter.status(&format!(
            "Skipped: URL already recorded (use --force to override):\n  {url}"
        ));
        return Ok(());
    }

    // ── Offline → queue ───────────────────────────────────────────────────────
    if !p.network().has_internet() {
        reporter.status("No internet connection – queuing URL for later processing.");
        p.queue().enqueue(url)?;
        reporter.status(&format!("Queued: {url}"));
        // Best-effort offline sync (commits any pending local changes).
        let _ = crate::commands::sync::commit_and_push(
            &config.data_dir,
            Some("Offline"),
            None,
            /*push=*/ false,
            p,
        );
        return Ok(());
    }

    // ── Parse ─────────────────────────────────────────────────────────────────
    reporter.status(&format!("Parsing: {url}"));
    let invoice = match parser::parse(url, p.http()) {
        Ok(inv) => inv,
        Err(e) => {
            reporter.status(&format!("Failed to parse invoice: {e}"));
            p.failed().record(url)?;
            return Err(e);
        }
    };

    // ── Write / dry-run ───────────────────────────────────────────────────────
    if args.dry_run {
        mapper::print_invoice(&invoice, reporter);
    } else {
        let written = mapper::write_invoice(&invoice, p.transactions(), reporter)?;
        reporter.out(&format!("Wrote {} file(s).", written.len()));

        // ── Sync ──────────────────────────────────────────────────────────────
        if !args.no_sync {
            if let Err(e) = crate::commands::sync::run(
                SyncArgs {
                    message: None,
                    no_push: false,
                },
                config,
                p,
            ) {
                reporter.status(&format!("Warning: sync failed: {e}"));
            }
        }
    }

    Ok(())
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
}
