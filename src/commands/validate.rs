use std::fs;
use std::path::{Path, PathBuf};

use crate::cli::ValidateArgs;
use crate::config::Config;
use crate::error::{Error, Result, ValidationError};

pub fn run(args: ValidateArgs, config: &Config) -> Result<()> {
    let schema_path = config.schema_file.as_ref().ok_or_else(|| {
        Error::Config(
            "SCHEMA_FILE is not set. Add it to your config file or set the SCHEMA_FILE \
             environment variable."
                .to_string(),
        )
    })?;

    let validator = load_schema(schema_path)?;

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
        match validate_file(path, &validator) {
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

// ── Schema loading ────────────────────────────────────────────────────────────

fn load_schema(path: &Path) -> Result<jsonschema::Validator> {
    let content = fs::read_to_string(path).map_err(|e| {
        Error::Config(format!("Cannot read schema file {}: {e}", path.display()))
    })?;

    let schema_json = yaml_to_json(&content)?;

    jsonschema::validator_for(&schema_json).map_err(|e| {
        Error::Config(format!(
            "Invalid JSON Schema in {}: {e}",
            path.display()
        ))
    })
}

// ── Per-file validation ───────────────────────────────────────────────────────

fn validate_file(
    path: &Path,
    validator: &jsonschema::Validator,
) -> Result<Vec<ValidationError>> {
    let raw = fs::read_to_string(path)?;
    let yaml_text = extract_front_matter(&raw, path)?;
    let instance = yaml_to_json(yaml_text)?;

    let errors = validator
        .iter_errors(&instance)
        .map(|e| ValidationError {
            path: path.to_path_buf(),
            field: e.instance_path().to_string(),
            message: e.to_string(),
        })
        .collect();

    Ok(errors)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Extract the YAML block between the first pair of `---` delimiters.
fn extract_front_matter<'a>(text: &'a str, path: &Path) -> Result<&'a str> {
    if !text.starts_with("---") {
        return Err(Error::Parse(format!(
            "{}: no YAML front-matter found",
            path.display()
        )));
    }
    let mut parts = text.splitn(3, "---");
    parts.next(); // empty slice before first ---
    parts.next().ok_or_else(|| {
        Error::Parse(format!("{}: malformed front-matter", path.display()))
    })
}

/// Parse a YAML string into a `serde_json::Value`.
///
/// `serde_yaml::Value` implements `Serialize`, so we go through serde's data
/// model without an intermediate JSON string.
fn yaml_to_json(yaml: &str) -> Result<serde_json::Value> {
    let yaml_val: serde_yaml::Value = serde_yaml::from_str(yaml)?;
    serde_json::to_value(yaml_val).map_err(Error::Json)
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
