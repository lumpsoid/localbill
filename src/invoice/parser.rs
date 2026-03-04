use scraper::{Html, Selector};

use crate::error::{Error, Result};
use crate::invoice::{Invoice, InvoiceItem};
use crate::sanitize::cyrillic_to_latin;

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
    AppleWebKit/537.36 (KHTML, like Gecko) Chrome/134.0.0.0 Safari/537.36";

const SPECIFICATIONS_URL: &str = "https://suf.purs.gov.rs/specifications";

/// Parse a Serbian fiscal invoice URL, retrying up to `max_attempts` times
/// when the token endpoint returns an error (tokens can expire mid-request).
pub fn parse(url: &str) -> Result<Invoice> {
    let agent = ureq::AgentBuilder::new().user_agent(USER_AGENT).build();
    parse_with_agent(url, &agent, 3)
}

fn parse_with_agent(url: &str, agent: &ureq::Agent, max_attempts: u32) -> Result<Invoice> {
    let mut last_err = Error::Parse("max attempts exhausted".to_string());

    for attempt in 1..=max_attempts {
        if attempt > 1 {
            eprintln!("Attempt {attempt}: retrying after token failure…");
            std::thread::sleep(std::time::Duration::from_secs(1));
        }

        match try_parse(url, agent) {
            Ok(inv) => return Ok(inv),
            Err(e) => {
                let msg = e.to_string();
                // Retry only for token / item-fetch failures; fail fast otherwise.
                if msg.contains("fetch items") || msg.contains("token") || msg.contains("Token") {
                    eprintln!("Attempt {attempt}: {e}");
                    last_err = e;
                } else {
                    return Err(e);
                }
            }
        }
    }

    Err(last_err)
}

fn try_parse(url: &str, agent: &ureq::Agent) -> Result<Invoice> {
    let body = agent
        .get(url)
        .call()?
        .into_string()
        .map_err(Error::Io)?;

    let doc = Html::parse_document(&body);

    let invoice_number = sel_text(&doc, "#invoiceNumberLabel")?;
    let retailer = cyrillic_to_latin(&sel_text(&doc, "#shopFullNameLabel")?);
    let date_raw = sel_text(&doc, "#sdcDateTimeLabel")?;
    let price_raw = sel_text(&doc, "#totalAmountLabel")?;
    // The receipt pre-block lives inside a Bootstrap collapse panel.
    let raw_bill_text = sel_text(&doc, "#collapse3 > div > pre").unwrap_or_default();

    let date = parse_date(&date_raw)?;
    let total_price = parse_price(&price_raw)?;

    let token = extract_token(&body)?;
    let items = fetch_items(agent, &invoice_number, &token)?;

    Ok(Invoice {
        invoice_number,
        retailer,
        date,
        total_price,
        currency: "RSD".to_string(),
        country: "serbia".to_string(),
        url: url.to_string(),
        raw_bill_text,
        items,
    })
}

// ── HTML helpers ─────────────────────────────────────────────────────────────

fn sel_text(doc: &Html, selector: &str) -> Result<String> {
    let sel = Selector::parse(selector)
        .map_err(|e| Error::Parse(format!("invalid CSS selector '{selector}': {e:?}")))?;
    doc.select(&sel)
        .next()
        .map(|el| el.text().collect::<String>().trim().to_string())
        .ok_or_else(|| Error::Parse(format!("element not found: {selector}")))
}

/// Extract the JWT-style view-model token embedded in the page's inline JS.
/// The original Python used: `viewModel\.Token\('(.*)'\);`
fn extract_token(html: &str) -> Result<String> {
    const NEEDLE: &str = "viewModel.Token('";
    for line in html.lines() {
        if let Some(start) = line.find(NEEDLE) {
            let rest = &line[start + NEEDLE.len()..];
            if let Some(end) = rest.find("');") {
                return Ok(rest[..end].to_string());
            }
        }
    }
    Err(Error::Parse("Token not found in page script".to_string()))
}

// ── Items API ─────────────────────────────────────────────────────────────────

fn fetch_items(
    agent: &ureq::Agent,
    invoice_number: &str,
    token: &str,
) -> Result<Vec<InvoiceItem>> {
    let body = format!(
        "invoiceNumber={}&token={}",
        percent_encode(invoice_number),
        percent_encode(token)
    );

    let response: serde_json::Value = agent
        .post(SPECIFICATIONS_URL)
        .set("Content-Type", "application/x-www-form-urlencoded")
        .send_string(&body)?
        .into_json()?;

    if !response["success"].as_bool().unwrap_or(false) {
        return Err(Error::Parse("Failed to fetch invoice items".to_string()));
    }

    let items = response["items"]
        .as_array()
        .ok_or_else(|| Error::Parse("Missing 'items' array in API response".to_string()))?
        .iter()
        .map(|v| {
            Ok(InvoiceItem {
                name: string_field(v, "name"),
                quantity: float_field(v, "quantity"),
                unit_price: float_field(v, "unitPrice"),
                total: float_field(v, "total"),
                gtin: string_field(v, "gtin"),
                label: string_field(v, "label"),
                label_rate: float_field(v, "labelRate"),
                tax_base_amount: float_field(v, "taxBaseAmount"),
                vat_amount: float_field(v, "vatAmount"),
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(items)
}

fn string_field(v: &serde_json::Value, key: &str) -> String {
    cyrillic_to_latin(v[key].as_str().unwrap_or(""))
}

fn float_field(v: &serde_json::Value, key: &str) -> f64 {
    v[key].as_f64().unwrap_or(0.0)
}

// ── Parsing helpers ───────────────────────────────────────────────────────────

/// Convert Serbian date `"DD.MM.YYYY. HH:MM:SS"` to ISO 8601 `"YYYY-MM-DDTHH:MM:SS"`.
fn parse_date(s: &str) -> Result<String> {
    let s = s.trim();
    let (date_part, time_part) = s
        .split_once(' ')
        .ok_or_else(|| Error::Parse(format!("expected space in date string: '{s}'")))?;

    // "15.03.2024." – the trailing dot must be stripped first.
    let date_part = date_part.trim_end_matches('.');
    let segments: Vec<&str> = date_part.split('.').collect();
    if segments.len() != 3 {
        return Err(Error::Parse(format!(
            "expected DD.MM.YYYY in date part: '{date_part}'"
        )));
    }
    let (dd, mm, yyyy) = (segments[0], segments[1], segments[2]);
    Ok(format!("{yyyy}-{mm}-{dd}T{time_part}"))
}

/// Parse a European-formatted number: `"1.234,56"` → `1234.56`.
fn parse_price(s: &str) -> Result<f64> {
    let cleaned = s.trim().replace('.', "").replace(',', ".");
    cleaned
        .parse::<f64>()
        .map_err(|_| Error::Parse(format!("cannot parse price: '{s}'")))
}

/// Percent-encode characters that are not URL-safe (RFC 3986 unreserved set).
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_conversion() {
        assert_eq!(
            parse_date("15.03.2024. 14:30:00").unwrap(),
            "2024-03-15T14:30:00"
        );
    }

    #[test]
    fn price_parsing() {
        assert_eq!(parse_price("1.234,56").unwrap(), 1234.56);
        assert_eq!(parse_price("99,00").unwrap(), 99.0);
    }

    #[test]
    fn token_extraction() {
        let html = "var x = 1;\nviewModel.Token('abc123');\nvar y = 2;";
        assert_eq!(extract_token(html).unwrap(), "abc123");
    }
}
