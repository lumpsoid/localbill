use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::cli::{ReportArgs, ReportCommand};
use crate::config::Config;
use crate::error::Result;

pub fn run(args: ReportArgs, config: &Config) -> Result<()> {
    match args.command {
        ReportCommand::Monthly { year, month } => monthly(&config.transaction_dir, year, month),
    }
}

// ── monthly ───────────────────────────────────────────────────────────────────

fn monthly(dir: &Path, filter_year: Option<u32>, filter_month: Option<u32>) -> Result<()> {
    // BTreeMap keeps months in sorted order automatically.
    let mut totals: BTreeMap<String, f64> = BTreeMap::new();
    let mut grand_total = 0.0f64;
    let mut count = 0usize;

    for entry in fs::read_dir(dir)?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }

        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };

        let Some((date, price)) = extract_date_and_price(&content) else {
            continue;
        };

        // date is "YYYY-MM-DDTHH:MM:SS" or similar; we only need YYYY-MM.
        let Some(ym) = date.get(..7) else { continue };
        let (yr, mo) = match parse_year_month(ym) {
            Some(v) => v,
            None => continue,
        };

        if filter_year.is_some_and(|y| y != yr) {
            continue;
        }
        if filter_month.is_some_and(|m| m != mo) {
            continue;
        }

        *totals.entry(ym.to_string()).or_insert(0.0) += price;
        grand_total += price;
        count += 1;
    }

    if totals.is_empty() {
        println!("No transactions found.");
        return Ok(());
    }

    // Header
    println!("{:<10}  {:>12}", "Month", "Total (RSD)");
    println!("{}", "-".repeat(26));
    for (ym, total) in &totals {
        println!("{ym:<10}  {:>12.2}", total);
    }
    println!("{}", "-".repeat(26));
    println!("{:<10}  {:>12.2}  ({count} items)", "TOTAL", grand_total);

    Ok(())
}

// ── helpers ───────────────────────────────────────────────────────────────────

/// Extract the `date` and `price_total` values from the YAML front-matter of a
/// Markdown file without pulling in a full YAML parser for this hot path.
fn extract_date_and_price(content: &str) -> Option<(String, f64)> {
    let mut date: Option<String> = None;
    let mut price: Option<f64> = None;

    // Only scan inside the front-matter block (between the first two `---`).
    let inner = if content.starts_with("---") {
        let mut parts = content.splitn(3, "---");
        parts.next(); // empty before first ---
        parts.next()? // the YAML block
    } else {
        return None;
    };

    for line in inner.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("date:") {
            date = Some(
                rest.trim()
                    .trim_matches('"')
                    .trim_matches('\'')
                    .to_string(),
            );
        } else if let Some(rest) = line.strip_prefix("price_total:") {
            price = rest.trim().parse::<f64>().ok();
        }
        if date.is_some() && price.is_some() {
            break;
        }
    }

    Some((date?, price?))
}

fn parse_year_month(ym: &str) -> Option<(u32, u32)> {
    // Expected "YYYY-MM"
    let (y, m) = ym.split_once('-')?;
    Some((y.parse().ok()?, m.parse().ok()?))
}
