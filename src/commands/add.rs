//! Interactive command to create a new invoice entry based on the configured schema.
//!
//! Reads `schema_file` from config, walks every property in the JSON Schema, and
//! prompts the user for a value.  Required fields loop until valid input is given;
//! optional fields accept an empty line to skip.  The collected values are written
//! as a Markdown file with YAML front-matter into `transaction_dir`.

use std::io::{self, Write};

use serde_yaml::Value;

use crate::cli::AddArgs;
use crate::config::Config;
use crate::error::{Error, Result};
use crate::invoice::mapper;

// ── Entry point ───────────────────────────────────────────────────────────────

pub fn run(args: AddArgs, config: &Config) -> Result<()> {
    let schema_path = config.schema_file.as_ref().ok_or_else(|| {
        Error::Config(
            "schema_file is not configured; set schema_file in config.yaml".to_string(),
        )
    })?;

    let schema_content = std::fs::read_to_string(schema_path)?;
    let schema: Value = serde_yaml::from_str(&schema_content)?;

    eprintln!("Adding a new invoice entry interactively.");
    eprintln!("Press Enter to skip optional fields.\n");

    let data = prompt_from_schema(&schema)?;

    let date_str = data
        .get("date")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let name_str = data
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("item");

    let date_prefix = mapper::compact_date(date_str);
    let slug = mapper::slugify(name_str);
    let base = format!("{date_prefix}-{slug}");

    let markdown = render_markdown(&data)?;

    if args.dry_run {
        eprintln!("\n# filename: {base}.md");
        print!("{markdown}");
    } else {
        std::fs::create_dir_all(&config.transaction_dir)?;
        let path = mapper::unique_path(&config.transaction_dir, &base, "md");
        std::fs::write(&path, &markdown)?;
        println!("Saved: {}", path.display());

        if !args.no_sync {
            if let Err(e) = crate::commands::sync::run(
                crate::cli::SyncArgs {
                    message: None,
                    no_push: false,
                },
                config,
            ) {
                eprintln!("Warning: sync failed: {e}");
            }
        }
    }

    Ok(())
}

// ── Pure helpers ──────────────────────────────────────────────────────────────

/// Render collected field data as a Markdown file with YAML front-matter.
pub(crate) fn render_markdown(data: &serde_yaml::Mapping) -> Result<String> {
    let yaml_str = serde_yaml::to_string(data)?;
    Ok(format!("---\n{yaml_str}---\n"))
}

/// Build the inline hint string shown next to a field prompt.
///
/// Returns an empty string when there is nothing to show.
pub(crate) fn build_hint(
    format: Option<&str>,
    pattern: Option<&str>,
    minimum: Option<f64>,
    maximum: Option<f64>,
    enum_vals: Option<&[&str]>,
    required: bool,
) -> String {
    let mut hints: Vec<String> = Vec::new();
    if let Some(fmt) = format {
        hints.push(format!("format: {fmt}"));
    }
    if let Some(pat) = pattern {
        hints.push(format!("pattern: {pat}"));
    }
    if let Some(min) = minimum {
        hints.push(format!("min: {min}"));
    }
    if let Some(max) = maximum {
        hints.push(format!("max: {max}"));
    }
    if let Some(vals) = enum_vals {
        hints.push(format!("one of: {}", vals.join(" | ")));
    }
    if !required {
        hints.push("optional".to_string());
    }
    if hints.is_empty() {
        String::new()
    } else {
        format!(" ({})", hints.join(", "))
    }
}

/// Parse and range-check a number input string.
///
/// Returns the parsed `f64` on success, or a user-facing error message on failure.
pub(crate) fn validate_number_input(
    input: &str,
    minimum: Option<f64>,
    maximum: Option<f64>,
) -> std::result::Result<f64, String> {
    let n: f64 = input
        .parse()
        .map_err(|_| "Please enter a valid number.".to_string())?;
    if let Some(min) = minimum {
        if n < min {
            return Err(format!("Value must be >= {min}"));
        }
    }
    if let Some(max) = maximum {
        if n > max {
            return Err(format!("Value must be <= {max}"));
        }
    }
    Ok(n)
}

/// Validate a string input against schema constraints.
///
/// Returns:
/// - `Ok(None)` — input is valid.
/// - `Ok(Some(message))` — input is invalid; show the message and re-prompt.
/// - `Err(e)` — the schema itself is broken (e.g. invalid regex); propagate.
pub(crate) fn validate_string_input(
    input: &str,
    min_length: Option<u64>,
    pattern: Option<&str>,
    enum_vals: Option<&[&str]>,
    format: Option<&str>,
) -> Result<Option<String>> {
    if let Some(min_len) = min_length {
        if input.len() < min_len as usize {
            return Ok(Some(format!("Must be at least {min_len} character(s).")));
        }
    }
    if let Some(pat) = pattern {
        match validate_pattern(input, pat) {
            Ok(true) => {}
            Ok(false) => {
                return Ok(Some(format!("Does not match required pattern: {pat}")));
            }
            Err(e) => {
                return Err(Error::Parse(format!("Invalid schema pattern '{pat}': {e}")));
            }
        }
    }
    if let Some(vals) = enum_vals {
        if !vals.contains(&input) {
            return Ok(Some(format!("Must be one of: {}", vals.join(", "))));
        }
    }
    if format == Some("datetime") && !looks_like_datetime(input) {
        return Ok(Some(
            "Expected datetime format: YYYY-MM-DDTHH:MM:SS".to_string(),
        ));
    }
    Ok(None)
}

/// Compile `pattern` as a regex and test it against `input`.
///
/// Returns `Err` if the pattern itself is invalid (schema bug).
pub(crate) fn validate_pattern(
    input: &str,
    pattern: &str,
) -> std::result::Result<bool, regex::Error> {
    let re = regex::Regex::new(pattern)?;
    Ok(re.is_match(input))
}

/// Minimal datetime check: at least 19 chars with `T` at position 10.
pub(crate) fn looks_like_datetime(s: &str) -> bool {
    s.len() >= 19 && s.as_bytes().get(10) == Some(&b'T')
}

// ── Schema-driven prompt loop ─────────────────────────────────────────────────

/// Walk the schema `properties` in order and collect user input for each field.
fn prompt_from_schema(schema: &Value) -> Result<serde_yaml::Mapping> {
    let properties = schema
        .get("properties")
        .and_then(|v| v.as_mapping())
        .ok_or_else(|| Error::Parse("Schema missing 'properties'".to_string()))?;

    let required_list: Vec<&str> = schema
        .get("required")
        .and_then(|v| v.as_sequence())
        .map(|seq| seq.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    let mut data = serde_yaml::Mapping::new();

    for (key_val, prop_def) in properties {
        let key = key_val.as_str().unwrap_or("");
        let is_required = required_list.contains(&key);
        let field_type = prop_def
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("string");

        match field_type {
            "array" => {
                let items_def = prop_def.get("items");
                let item_type = items_def
                    .and_then(|v| v.get("type"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("string");

                let values = if item_type == "object" {
                    let item_props = items_def
                        .and_then(|v| v.get("properties"))
                        .and_then(|v| v.as_mapping());
                    let item_required: Vec<&str> = items_def
                        .and_then(|v| v.get("required"))
                        .and_then(|v| v.as_sequence())
                        .map(|s| s.iter().filter_map(|v| v.as_str()).collect())
                        .unwrap_or_default();
                    prompt_object_array(key, item_props, &item_required)?
                } else {
                    prompt_string_array(key)?
                };

                // Always include array fields so the structure is explicit.
                data.insert(key_val.clone(), Value::Sequence(values));
            }
            _ => {
                if let Some(value) = prompt_scalar_field(key, prop_def, is_required)? {
                    data.insert(key_val.clone(), value);
                }
            }
        }
    }

    Ok(data)
}

// ── Field-level prompts (I/O shells around pure validators) ───────────────────

fn read_input(prompt: &str) -> io::Result<String> {
    print!("{prompt}");
    io::stdout().flush()?;
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    Ok(line.trim().to_string())
}

/// Prompt for a single scalar value (string or number), returning `None` if the
/// field is optional and the user pressed Enter.
fn prompt_scalar_field(name: &str, prop: &Value, required: bool) -> Result<Option<Value>> {
    let field_type = prop
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("string");
    let format = prop.get("format").and_then(|v| v.as_str());
    let pattern = prop.get("pattern").and_then(|v| v.as_str());
    let min_length = prop.get("minLength").and_then(|v| v.as_u64());
    let minimum = prop.get("minimum").and_then(|v| v.as_f64());
    let maximum = prop.get("maximum").and_then(|v| v.as_f64());
    let enum_vals: Option<Vec<&str>> = prop
        .get("enum")
        .and_then(|v| v.as_sequence())
        .map(|s| s.iter().filter_map(|v| v.as_str()).collect());

    let hint_str = build_hint(
        format,
        pattern,
        minimum,
        maximum,
        enum_vals.as_deref(),
        required,
    );

    loop {
        let input = read_input(&format!("{name}{hint_str}: ")).map_err(Error::Io)?;

        if input.is_empty() {
            if required {
                eprintln!("  '{name}' is required – please enter a value.");
                continue;
            } else {
                return Ok(None);
            }
        }

        match field_type {
            "number" => match validate_number_input(&input, minimum, maximum) {
                Ok(n) => return Ok(Some(Value::Number(serde_yaml::Number::from(n)))),
                Err(msg) => {
                    eprintln!("  {msg}");
                    continue;
                }
            },
            _ => {
                match validate_string_input(&input, min_length, pattern, enum_vals.as_deref(), format)? {
                    Some(msg) => {
                        eprintln!("  {msg}");
                        continue;
                    }
                    None => return Ok(Some(Value::String(input))),
                }
            }
        }
    }
}

/// Prompt for an array of plain strings (e.g. `tags`).
fn prompt_string_array(name: &str) -> Result<Vec<Value>> {
    let mut items: Vec<Value> = Vec::new();
    eprintln!("  {name}: enter items one by one; leave empty to finish.");
    loop {
        let label = if items.is_empty() {
            format!("  {name}[0] (or Enter to skip): ")
        } else {
            format!("  {name}[{}]: ", items.len())
        };
        let input = read_input(&label).map_err(Error::Io)?;
        if input.is_empty() {
            break;
        }
        items.push(Value::String(input));
    }
    Ok(items)
}

/// Prompt for an array of objects (e.g. `exchange`, `fees`, `discounts`).
fn prompt_object_array(
    array_name: &str,
    item_props: Option<&serde_yaml::Mapping>,
    item_required: &[&str],
) -> Result<Vec<Value>> {
    let mut items: Vec<Value> = Vec::new();
    loop {
        let prompt = if items.is_empty() {
            format!("Add a {array_name} entry? [y/N]: ")
        } else {
            format!("Add another {array_name} entry? [y/N]: ")
        };
        let response = read_input(&prompt).map_err(Error::Io)?;
        if !matches!(response.to_lowercase().as_str(), "y" | "yes") {
            break;
        }

        let mut item = serde_yaml::Mapping::new();
        if let Some(props) = item_props {
            for (key_val, prop_def) in props {
                let key = key_val.as_str().unwrap_or("");
                let is_req = item_required.contains(&key);
                if let Some(value) =
                    prompt_scalar_field(&format!("  {array_name}.{key}"), prop_def, is_req)?
                {
                    item.insert(key_val.clone(), value);
                }
            }
        }
        items.push(Value::Mapping(item));
    }
    Ok(items)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── looks_like_datetime ───────────────────────────────────────────────────

    #[test]
    fn datetime_valid() {
        assert!(looks_like_datetime("2024-03-15T14:30:00"));
    }

    #[test]
    fn datetime_with_fractional_seconds() {
        assert!(looks_like_datetime("2024-03-15T14:30:00.123"));
    }

    #[test]
    fn datetime_too_short() {
        assert!(!looks_like_datetime("2024-03-15"));
    }

    #[test]
    fn datetime_wrong_separator() {
        assert!(!looks_like_datetime("2024-03-15 14:30:00"));
    }

    // ── validate_pattern ─────────────────────────────────────────────────────

    #[test]
    fn pattern_currency_valid() {
        assert_eq!(validate_pattern("USD", "^[A-Z]{3}$").unwrap(), true);
        assert_eq!(validate_pattern("RSD", "^[A-Z]{3}$").unwrap(), true);
    }

    #[test]
    fn pattern_currency_lowercase_rejected() {
        assert_eq!(validate_pattern("usd", "^[A-Z]{3}$").unwrap(), false);
    }

    #[test]
    fn pattern_currency_wrong_length() {
        assert_eq!(validate_pattern("US", "^[A-Z]{3}$").unwrap(), false);
        assert_eq!(validate_pattern("USDD", "^[A-Z]{3}$").unwrap(), false);
    }

    #[test]
    fn pattern_invalid_regex_returns_err() {
        assert!(validate_pattern("x", "[invalid").is_err());
    }

    // ── build_hint ────────────────────────────────────────────────────────────

    #[test]
    fn hint_empty_when_no_constraints_and_required() {
        assert_eq!(build_hint(None, None, None, None, None, true), "");
    }

    #[test]
    fn hint_optional_when_not_required() {
        assert_eq!(
            build_hint(None, None, None, None, None, false),
            " (optional)"
        );
    }

    #[test]
    fn hint_includes_format() {
        assert_eq!(
            build_hint(Some("datetime"), None, None, None, None, true),
            " (format: datetime)"
        );
    }

    #[test]
    fn hint_includes_pattern() {
        assert_eq!(
            build_hint(None, Some("^[A-Z]{3}$"), None, None, None, true),
            " (pattern: ^[A-Z]{3}$)"
        );
    }

    #[test]
    fn hint_includes_min_max() {
        assert_eq!(
            build_hint(None, None, Some(0.0), Some(100.0), None, true),
            " (min: 0, max: 100)"
        );
    }

    #[test]
    fn hint_includes_enum() {
        assert_eq!(
            build_hint(None, None, None, None, Some(&["percentage", "fixed"]), true),
            " (one of: percentage | fixed)"
        );
    }

    #[test]
    fn hint_combines_all() {
        let h = build_hint(
            Some("uri"),
            None,
            None,
            None,
            None,
            false,
        );
        assert_eq!(h, " (format: uri, optional)");
    }

    // ── validate_number_input ─────────────────────────────────────────────────

    #[test]
    fn number_valid_no_constraints() {
        assert_eq!(validate_number_input("3.14", None, None).unwrap(), 3.14);
    }

    #[test]
    fn number_integer_string() {
        assert_eq!(validate_number_input("5", None, None).unwrap(), 5.0);
    }

    #[test]
    fn number_not_a_number() {
        assert!(validate_number_input("abc", None, None).is_err());
    }

    #[test]
    fn number_below_minimum() {
        assert!(validate_number_input("-1", Some(0.0), None).is_err());
    }

    #[test]
    fn number_at_minimum() {
        assert_eq!(validate_number_input("0", Some(0.0), None).unwrap(), 0.0);
    }

    #[test]
    fn number_above_maximum() {
        assert!(validate_number_input("101", None, Some(100.0)).is_err());
    }

    #[test]
    fn number_at_maximum() {
        assert_eq!(
            validate_number_input("100", None, Some(100.0)).unwrap(),
            100.0
        );
    }

    // ── validate_string_input ─────────────────────────────────────────────────

    #[test]
    fn string_valid_no_constraints() {
        assert!(validate_string_input("hello", None, None, None, None)
            .unwrap()
            .is_none());
    }

    #[test]
    fn string_too_short() {
        let msg = validate_string_input("x", Some(3), None, None, None)
            .unwrap()
            .unwrap();
        assert!(msg.contains("3"));
    }

    #[test]
    fn string_meets_min_length() {
        assert!(validate_string_input("abc", Some(3), None, None, None)
            .unwrap()
            .is_none());
    }

    #[test]
    fn string_pattern_match() {
        assert!(
            validate_string_input("USD", None, Some("^[A-Z]{3}$"), None, None)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn string_pattern_no_match() {
        let msg = validate_string_input("usd", None, Some("^[A-Z]{3}$"), None, None)
            .unwrap()
            .unwrap();
        assert!(msg.contains("pattern"));
    }

    #[test]
    fn string_invalid_pattern_returns_err() {
        assert!(validate_string_input("x", None, Some("[bad"), None, None).is_err());
    }

    #[test]
    fn string_enum_valid() {
        assert!(
            validate_string_input("fixed", None, None, Some(&["percentage", "fixed"]), None)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn string_enum_invalid() {
        let msg = validate_string_input("other", None, None, Some(&["percentage", "fixed"]), None)
            .unwrap()
            .unwrap();
        assert!(msg.contains("percentage"));
    }

    #[test]
    fn string_datetime_valid() {
        assert!(
            validate_string_input("2024-03-15T14:30:00", None, None, None, Some("datetime"))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn string_datetime_invalid() {
        let msg = validate_string_input("2024-03-15", None, None, None, Some("datetime"))
            .unwrap()
            .unwrap();
        assert!(msg.contains("datetime"));
    }

    // ── render_markdown ───────────────────────────────────────────────────────

    #[test]
    fn render_markdown_wraps_yaml_in_front_matter() {
        let mut map = serde_yaml::Mapping::new();
        map.insert(
            Value::String("name".into()),
            Value::String("Test Item".into()),
        );
        let md = render_markdown(&map).unwrap();
        assert!(md.starts_with("---\n"));
        assert!(md.ends_with("---\n"));
        assert!(md.contains("name: Test Item"));
    }
}
