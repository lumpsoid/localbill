use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::cli::ValidateArgs;
use crate::config::Config;
use crate::error::{Error, Result, ValidationError};

pub fn run(args: ValidateArgs, config: &Config) -> Result<()> {
    let target = args
        .path
        .unwrap_or_else(|| config.transaction_dir.clone());

    let files = collect_files(&target)?;
    if files.is_empty() {
        eprintln!("No .md / .yaml files found in {}.", target.display());
        return Err(Error::Config("no files to validate".to_string()));
    }

    let errors_only = args.errors_only;
    let continue_on_error = args.continue_on_error || errors_only;

    let mut all_errors: Vec<ValidationError> = Vec::new();

    for path in &files {
        match validate_file(path) {
            Ok(errs) if errs.is_empty() => {
                if !errors_only {
                    println!("ok  {}", path.display());
                }
            }
            Ok(errs) => {
                if errors_only {
                    println!("{}", path.display());
                } else {
                    for e in &errs {
                        eprintln!("{e}");
                    }
                }
                all_errors.extend(errs);
                if !continue_on_error {
                    return Err(Error::Validation(all_errors));
                }
            }
            Err(e) => {
                let ve = ValidationError {
                    path: path.clone(),
                    field: "parse".to_string(),
                    message: e.to_string(),
                };
                if errors_only {
                    println!("{}", path.display());
                } else {
                    eprintln!("{ve}");
                }
                all_errors.push(ve);
                if !continue_on_error {
                    return Err(Error::Validation(all_errors));
                }
            }
        }
    }

    if all_errors.is_empty() {
        println!("\nAll {} file(s) are valid.", files.len());
        Ok(())
    } else {
        eprintln!(
            "\n{} error(s) found in {} file(s).",
            all_errors.len(),
            files.len()
        );
        Err(Error::Validation(all_errors))
    }
}

// ── Per-file validation ───────────────────────────────────────────────────────

fn validate_file(path: &Path) -> Result<Vec<ValidationError>> {
    let raw = fs::read_to_string(path)?;
    let yaml_text = extract_front_matter(&raw, path)?;
    let data: HashMap<String, serde_yaml::Value> = serde_yaml::from_str(yaml_text)?;
    Ok(check_schema(&data, path))
}

/// Extract the YAML block between the first pair of `---` delimiters.
fn extract_front_matter<'a>(text: &'a str, path: &Path) -> Result<&'a str> {
    if !text.starts_with("---") {
        return Err(Error::Parse(format!(
            "{}: no YAML front-matter found",
            path.display()
        )));
    }
    // Split on "---" and take the second segment.
    let mut parts = text.splitn(3, "---");
    parts.next(); // before first ---
    parts.next().ok_or_else(|| {
        Error::Parse(format!(
            "{}: malformed front-matter",
            path.display()
        ))
    })
}

// ── Schema rules ──────────────────────────────────────────────────────────────

/// Required string fields with minimum length 1.
const REQUIRED_STRINGS: &[&str] = &["date", "name", "retailer", "currency", "country"];

/// Required numeric fields with minimum value 0.
const REQUIRED_NUMBERS: &[&str] = &["quantity", "unit_price", "price_total"];

fn check_schema(
    data: &HashMap<String, serde_yaml::Value>,
    path: &Path,
) -> Vec<ValidationError> {
    let mut errors = Vec::new();

    let mut err = |field: &str, msg: &str| {
        errors.push(ValidationError {
            path: path.to_path_buf(),
            field: field.to_string(),
            message: msg.to_string(),
        });
    };

    // Required string fields
    for &field in REQUIRED_STRINGS {
        match data.get(field) {
            None => err(field, "required field is missing"),
            Some(serde_yaml::Value::String(s)) if s.is_empty() => {
                err(field, "must not be empty")
            }
            Some(serde_yaml::Value::String(_)) => {}
            Some(_) => err(field, "must be a string"),
        }
    }

    // Required numeric fields
    for &field in REQUIRED_NUMBERS {
        match data.get(field) {
            None => err(field, "required field is missing"),
            Some(v) => {
                let n = yaml_to_f64(v);
                match n {
                    None => err(field, "must be a number"),
                    Some(f) if f < 0.0 => err(field, "must be >= 0"),
                    _ => {}
                }
            }
        }
    }

    // currency must match ^[A-Z]{3}$
    if let Some(serde_yaml::Value::String(c)) = data.get("currency") {
        if !is_currency_code(c) {
            err("currency", "must be a 3-letter uppercase ISO 4217 code");
        }
    }

    // tags must be an array
    if let Some(v) = data.get("tags") {
        if !matches!(v, serde_yaml::Value::Sequence(_) | serde_yaml::Value::Null) {
            err("tags", "must be an array");
        }
    }

    // link must look like a URI
    if let Some(serde_yaml::Value::String(link)) = data.get("link") {
        if !link.is_empty() && !link.starts_with("http://") && !link.starts_with("https://") {
            err("link", "must be a valid URI (http:// or https://)");
        }
    }

    errors
}

fn yaml_to_f64(v: &serde_yaml::Value) -> Option<f64> {
    match v {
        serde_yaml::Value::Number(n) => n.as_f64(),
        _ => None,
    }
}

fn is_currency_code(s: &str) -> bool {
    s.len() == 3 && s.chars().all(|c| c.is_ascii_uppercase())
}

// ── File collection ───────────────────────────────────────────────────────────

fn collect_files(target: &Path) -> Result<Vec<PathBuf>> {
    if target.is_file() {
        return Ok(vec![target.to_path_buf()]);
    }
    if !target.is_dir() {
        return Err(Error::Config(format!(
            "path does not exist: {}",
            target.display()
        )));
    }
    let mut files: Vec<PathBuf> = fs::read_dir(target)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && matches!(
                    p.extension().and_then(|e| e.to_str()),
                    Some("md") | Some("yaml") | Some("yml")
                )
        })
        .collect();
    files.sort();
    Ok(files)
}
