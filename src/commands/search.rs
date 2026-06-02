use std::collections::HashMap;

use crate::cli::{SearchArgs, SearchCommand};
use crate::config::Config;
use crate::error::Result;
use crate::ports::{Platform, Reporter, StoredDoc, TransactionStore};

pub fn run<P: Platform>(args: SearchArgs, _config: &Config, p: &P) -> Result<()> {
    match args.command {
        SearchCommand::Name { query } => search_by_name(&query, p),
        SearchCommand::Duplicates => find_duplicates(p),
    }
}

/// Only `.md` docs participate in search.
fn md_docs(docs: Vec<StoredDoc>) -> impl Iterator<Item = StoredDoc> {
    docs.into_iter()
        .filter(|d| d.path.extension().and_then(|e| e.to_str()) == Some("md"))
}

// ── search by product name ────────────────────────────────────────────────────

fn search_by_name<P: Platform>(query: &str, p: &P) -> Result<()> {
    let query_lower = query.to_lowercase();
    let mut results: Vec<(String, String, f64, String)> = Vec::new(); // (date, name, unit_price, file)

    for doc in md_docs(p.transactions().list()?) {
        let fields = extract_fields(&doc.content);
        let name = fields.get("name").cloned().unwrap_or_default();

        if !name.to_lowercase().contains(&query_lower) {
            continue;
        }

        let date = fields.get("date").cloned().unwrap_or_default();
        let unit_price: f64 = fields
            .get("unit_price")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0);
        let filename = doc
            .path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        results.push((date, name, unit_price, filename));
    }

    let reporter = p.reporter();

    if results.is_empty() {
        reporter.out(&format!("No matches for '{query}'."));
        return Ok(());
    }

    // Sort by date descending (newest first).
    results.sort_by(|a, b| b.0.cmp(&a.0));

    reporter.out(&format!(
        "{:<22}  {:>10}  {:<40}  {}",
        "Date", "Unit price", "Name", "File"
    ));
    reporter.out(&"-".repeat(100));
    for (date, name, unit_price, file) in &results {
        reporter.out(&format!(
            "{date:<22}  {unit_price:>10.2}  {name:<40}  {file}"
        ));
    }
    reporter.out(&format!("\n{} result(s).", results.len()));
    Ok(())
}

// ── find duplicate links ──────────────────────────────────────────────────────

fn find_duplicates<P: Platform>(p: &P) -> Result<()> {
    // Map from link URL → list of filenames that contain it.
    let mut link_map: HashMap<String, Vec<String>> = HashMap::new();

    for doc in md_docs(p.transactions().list()?) {
        let fields = extract_fields(&doc.content);
        let link = fields.get("link").cloned().unwrap_or_default();
        if link.is_empty() {
            continue;
        }

        let filename = doc
            .path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        link_map.entry(link).or_default().push(filename);
    }

    let duplicates: Vec<_> = link_map.iter().filter(|(_, v)| v.len() > 1).collect();
    let reporter = p.reporter();

    if duplicates.is_empty() {
        reporter.out("No duplicate invoice URLs found.");
        return Ok(());
    }

    reporter.out(&format!(
        "{} duplicate invoice URL(s) found:\n",
        duplicates.len()
    ));
    for (link, files) in &duplicates {
        reporter.out(&format!("  {link}"));
        for f in files.iter() {
            reporter.out(&format!("    → {f}"));
        }
    }
    Ok(())
}

// ── helpers ───────────────────────────────────────────────────────────────────

/// Extract simple `key: value` pairs from YAML front-matter (first `---` block).
fn extract_fields(content: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();

    let inner = if content.starts_with("---") {
        let mut parts = content.splitn(3, "---");
        parts.next();
        match parts.next() {
            Some(s) => s,
            None => return map,
        }
    } else {
        return map;
    };

    for line in inner.lines() {
        let line = line.trim();
        // Skip block scalar continuations (indented lines).
        if line.starts_with(' ') || line.starts_with('\t') {
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim().to_string();
            let value = value
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string();
            map.insert(key, value);
        }
    }

    map
}
