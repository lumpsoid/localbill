use std::path::PathBuf;

use crate::error::Result;
use crate::invoice::Invoice;
use crate::ports::{Reporter, TransactionStore};

// ── Public API ────────────────────────────────────────────────────────────────

/// Write one Markdown file per item from `invoice` into the transaction store.
/// Returns the list of paths that were created. Output is left to the caller
/// (the `insert` report) so the live progress display stays uncorrupted.
pub fn write_invoice(invoice: &Invoice, store: &impl TransactionStore) -> Result<Vec<PathBuf>> {
    let date_prefix = compact_date(&invoice.date);
    let mut written = Vec::with_capacity(invoice.items.len());

    for item in &invoice.items {
        let base = format!("{date_prefix}-{}", slugify(&item.name));
        let filename = unique_name(store, &base, "md");
        let content = render_markdown(
            invoice,
            &item.name,
            item.quantity,
            item.unit_price,
            item.total,
        );
        let path = store.write_new(&filename, &content)?;
        written.push(path);
    }

    Ok(written)
}

/// Print one Markdown block per item (dry-run mode).
pub fn print_invoice(invoice: &Invoice, reporter: &impl Reporter) {
    let date_prefix = compact_date(&invoice.date);
    for item in &invoice.items {
        let base = format!("{date_prefix}-{}", slugify(&item.name));
        reporter.out(&format!("# filename: {base}.md"));
        let content = render_markdown(
            invoice,
            &item.name,
            item.quantity,
            item.unit_price,
            item.total,
        );
        reporter.out(content.trim_end());
    }
}

// ── Rendering ─────────────────────────────────────────────────────────────────

/// Build the full Markdown document (YAML front-matter only; body is empty).
pub fn render_markdown(
    invoice: &Invoice,
    item_name: &str,
    quantity: f64,
    unit_price: f64,
    price_total: f64,
) -> String {
    // Each line of the receipt is indented by 4 spaces so YAML treats it as a
    // block scalar continuation.
    let indented_notes = invoice
        .raw_bill_text
        .lines()
        .map(|l| format!("    {l}"))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "---\n\
         date: \"{date}\"\n\
         retailer: \"{retailer}\"\n\
         name: \"{name}\"\n\
         quantity: {quantity}\n\
         unit_price: {unit_price}\n\
         price_total: {price_total}\n\
         currency: {currency}\n\
         country: {country}\n\
         link: \"{link}\"\n\
         tags: []\n\
         notes: |\n\
         {notes}\n\
         ---\n",
        date = invoice.date,
        retailer = yaml_escape(&invoice.retailer),
        name = yaml_escape(item_name),
        quantity = quantity,
        unit_price = unit_price,
        price_total = price_total,
        currency = invoice.currency,
        country = invoice.country,
        link = invoice.url,
        notes = indented_notes,
    )
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Convert `"YYYY-MM-DDTHH:MM:SS"` → `"YYYYMMDDTHHMMSS"` for use in filenames.
pub fn compact_date(date: &str) -> String {
    date.replace(['-', ':'], "")
}

/// Convert an arbitrary string into a lowercase, ASCII-only, hyphen-separated slug.
pub fn slugify(text: &str) -> String {
    let mut slug = String::new();
    let mut prev_sep = true; // start as true to trim leading separators

    for ch in text.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_sep = false;
        } else if !prev_sep {
            slug.push('-');
            prev_sep = true;
        }
    }

    // Trim trailing separator
    if slug.ends_with('-') {
        slug.pop();
    }
    slug
}

/// Escape backslashes and double-quotes in a YAML double-quoted string value.
fn yaml_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Return a filename `{base}.{ext}` that does not yet exist in `store`.
/// Appends `-01`, `-02`, … until a free slot is found.
pub fn unique_name(store: &impl TransactionStore, base: &str, ext: &str) -> String {
    let candidate = format!("{base}.{ext}");
    if !store.exists(&candidate) {
        return candidate;
    }
    let mut n = 1u32;
    loop {
        let candidate = format!("{base}-{n:02}.{ext}");
        if !store.exists(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_basic() {
        assert_eq!(slugify("Mlijeko 3.2% masti"), "mlijeko-3-2-masti");
        assert_eq!(slugify("  -- leading --  "), "leading");
    }

    #[test]
    fn compact_date_format() {
        assert_eq!(compact_date("2024-03-15T14:30:00"), "20240315T143000");
    }
}
