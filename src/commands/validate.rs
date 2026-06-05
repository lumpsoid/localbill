use std::path::Path;

use crate::cli::ValidateArgs;
use crate::config::Config;
use crate::error::{Error, Result, ValidationError};
use crate::ports::{Platform, Reporter, SchemaSource, StoredDoc, TransactionStore};

pub fn run<P: Platform>(args: ValidateArgs, config: &Config, p: &P) -> Result<()> {
    let schema_content = p.schema().load()?;
    let validator = compile_schema(&schema_content)?;
    let reporter = p.reporter();

    let target = args
        .path
        .clone()
        .unwrap_or_else(|| config.transaction_dir.clone());

    let docs = if target.is_file() {
        vec![p.transactions().read(&target)?]
    } else if target.is_dir() {
        p.transactions().list_at(&target)?
    } else {
        return Err(Error::Config(format!(
            "path does not exist: {}",
            target.display()
        )));
    };

    if docs.is_empty() {
        reporter.status(&format!(
            "No .md / .yaml files found in {}.",
            target.display()
        ));
        return Err(Error::Config("no files to validate".to_string()));
    }

    let errors_only = args.errors_only;
    let continue_on_error = args.continue_on_error || errors_only;

    let mut all_errors: Vec<ValidationError> = Vec::new();

    for doc in &docs {
        match validate_doc(doc, &validator) {
            Ok(errs) if errs.is_empty() => {
                if !errors_only {
                    reporter.out(&format!("ok  {}", doc.path.display()));
                }
            }
            Ok(errs) => {
                if errors_only {
                    reporter.out(&doc.path.display().to_string());
                } else {
                    for e in &errs {
                        reporter.status(&e.to_string());
                    }
                }
                all_errors.extend(errs);
                if !continue_on_error {
                    return Err(Error::Validation(all_errors));
                }
            }
            Err(e) => {
                let ve = ValidationError {
                    path: doc.path.clone(),
                    field: "parse".to_string(),
                    message: e.to_string(),
                };
                if errors_only {
                    reporter.out(&doc.path.display().to_string());
                } else {
                    reporter.status(&ve.to_string());
                }
                all_errors.push(ve);
                if !continue_on_error {
                    return Err(Error::Validation(all_errors));
                }
            }
        }
    }

    if all_errors.is_empty() {
        reporter.out(&format!("\nAll {} file(s) are valid.", docs.len()));
        Ok(())
    } else {
        reporter.status(&format!(
            "\n{} error(s) found in {} file(s).",
            all_errors.len(),
            docs.len()
        ));
        Err(Error::Validation(all_errors))
    }
}

// ── Schema compilation ──────────────────────────────────────────────────────────

pub(crate) fn compile_schema(content: &str) -> Result<jsonschema::Validator> {
    let schema_json = yaml_to_json(content)?;
    jsonschema::validator_for(&schema_json)
        .map_err(|e| Error::Config(format!("Invalid JSON Schema: {e}")))
}

// ── Per-doc validation ──────────────────────────────────────────────────────────

fn validate_doc(
    doc: &StoredDoc,
    validator: &jsonschema::Validator,
) -> Result<Vec<ValidationError>> {
    let yaml_text = extract_front_matter(&doc.content, &doc.path)?;
    let instance = yaml_to_json(yaml_text)?;

    let errors = validator
        .iter_errors(&instance)
        .map(|e| ValidationError {
            path: doc.path.clone(),
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
    parts
        .next()
        .ok_or_else(|| Error::Parse(format!("{}: malformed front-matter", path.display())))
}

/// Parse a YAML string into a `serde_json::Value`.
pub(crate) fn yaml_to_json(yaml: &str) -> Result<serde_json::Value> {
    let yaml_val: serde_yaml::Value = serde_yaml::from_str(yaml)?;
    serde_json::to_value(yaml_val).map_err(Error::Json)
}
