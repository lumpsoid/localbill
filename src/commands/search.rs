use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::cli::{SearchArgs, SearchCommand};
use crate::config::Config;
use crate::error::Result;

pub fn run(args: SearchArgs, config: &Config) -> Result<()> {
    match args.command {
        SearchCommand::Name { query } => search_by_name(&query, &config.transaction_dir),
        SearchCommand::Duplicates => find_duplicates(&config.transaction_dir),
    }
}

// ── search by product name ────────────────────────────────────────────────────

fn search_by_name(query: &str, dir: &Path) -> Result<()> {
    let query_lower = query.to_lowercase();
    let mut results: Vec<(String, String, f64, String)> = Vec::new(); // (date, name, unit_price, file)

    for entry in fs::read_dir(dir)?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }

        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };

        let fields = extract_fields(&content);
        let name = fields.get("name").cloned().unwrap_or_default();

        if !name.to_lowercase().contains(&query_lower) {
            continue;
        }

        let date = fields.get("date").cloned().unwrap_or_default();
        let unit_price: f64 = fields
            .get("unit_price")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0);
        let filename = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        results.push((date, name, unit_price, filename));
    }

    if results.is_empty() {
        println!("No matches for '{query}'.");
        return Ok(());
    }

    // Sort by date descending (newest first).
    results.sort_by(|a, b| b.0.cmp(&a.0));

    println!("{:<22}  {:>10}  {:<40}  {}", "Date", "Unit price", "Name", "File");
    println!("{}", "-".repeat(100));
    for (date, name, unit_price, file) in &results {
        println!("{date:<22}  {unit_price:>10.2}  {name:<40}  {file}");
    }
    println!("\n{} result(s).", results.len());
    Ok(())
}

// ── find duplicate links ──────────────────────────────────────────────────────

fn find_duplicates(dir: &Path) -> Result<()> {
    // Map from link URL → list of filenames that contain it.
    let mut link_map: HashMap<String, Vec<String>> = HashMap::new();

    for entry in fs::read_dir(dir)?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }

        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };

        let fields = extract_fields(&content);
        let link = fields.get("link").cloned().unwrap_or_default();
        if link.is_empty() {
            continue;
        }

        let filename = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        link_map.entry(link).or_default().push(filename);
    }

    let duplicates: Vec<_> = link_map.iter().filter(|(_, v)| v.len() > 1).collect();

    if duplicates.is_empty() {
        println!("No duplicate invoice URLs found.");
        return Ok(());
    }

    println!("{} duplicate invoice URL(s) found:\n", duplicates.len());
    for (link, files) in &duplicates {
        println!("  {link}");
        for f in files.iter() {
            println!("    → {f}");
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
