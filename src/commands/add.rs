//! Interactive command to create a new invoice entry from the configured schema.
//!
//! Thin orchestration only: load the schema, delegate field collection to
//! [`crate::schema_form`] (which owns the schema walking, validation, and the
//! datetime offset/component UX), then render the Markdown front-matter, pick a
//! collision-free filename, persist it, and — unless `--no-sync` — sync.

use std::path::PathBuf;

use serde_yaml::Value;

use crate::cli::{AddArgs, SyncArgs};
use crate::config::Config;
use crate::error::{Error, Result, ValidationError};
use crate::invoice::mapper;
use crate::ports::{Platform, Prompt, Reporter, SchemaSource, Style, Styler, TransactionStore};
use crate::schema_form;

pub fn run<P: Platform>(args: AddArgs, config: &Config, p: &P) -> Result<()> {
    let schema_content = p.schema().load()?;
    let schema: Value = serde_yaml::from_str(&schema_content)?;

    let data = match args.json.as_deref() {
        Some(src) => build_from_json(src, &schema, &schema_content, p)?,
        None => schema_form::collect(&schema, p.prompt(), p.reporter(), p.clock(), p.styler())?,
    };

    let date_str = data
        .get("date")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let name_str = data.get("name").and_then(|v| v.as_str()).unwrap_or("item");

    let date_prefix = mapper::compact_date(date_str);
    let slug = mapper::slugify(name_str);
    let base = format!("{date_prefix}-{slug}");

    let markdown = render_markdown(&data)?;
    let reporter = p.reporter();

    if args.dry_run {
        reporter.status(&format!("\n# filename: {base}.md"));
        reporter.out(markdown.trim_end());
        return Ok(());
    }

    let filename = mapper::unique_name(p.transactions(), &base, "md");
    let path = p.transactions().write_new(&filename, &markdown)?;
    reporter.out(
        &p.styler()
            .paint(Style::Success, &format!("Saved: {}", path.display())),
    );

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

    Ok(())
}

/// Build the field mapping from a JSON payload, validating it against the schema.
///
/// `src` is either the JSON text itself or `"-"` to read the whole blob from stdin.
/// On any schema violation, each error is reported via [`Reporter::status`] and an
/// [`Error::Validation`] is returned (no file is written). On success the fields are
/// emitted in schema-property order so JSON entries render identically to interactive ones.
fn build_from_json<P: Platform>(
    src: &str,
    schema: &Value,
    schema_content: &str,
    p: &P,
) -> Result<serde_yaml::Mapping> {
    let payload = if src == "-" {
        p.prompt().read_all()?
    } else {
        src.to_string()
    };

    let instance: serde_json::Value = serde_json::from_str(&payload)?;
    let obj = instance
        .as_object()
        .ok_or_else(|| Error::Parse("expected a JSON object of field values".to_string()))?;

    let validator = crate::commands::validate::compile_schema(schema_content)?;
    let errors: Vec<ValidationError> = validator
        .iter_errors(&instance)
        .map(|e| ValidationError {
            path: PathBuf::from("<json>"),
            field: e.instance_path().to_string(),
            message: e.to_string(),
        })
        .collect();

    if !errors.is_empty() {
        let reporter = p.reporter();
        for e in &errors {
            reporter.status(&e.to_string());
        }
        return Err(Error::Validation(errors));
    }

    // Emit fields in schema-property order; `additionalProperties: false` plus the
    // passing validation above guarantee `obj` holds no keys outside `properties`.
    let mut data = serde_yaml::Mapping::new();
    if let Some(props) = schema.get("properties").and_then(|v| v.as_mapping()) {
        for (key, _) in props.iter() {
            if let Some(name) = key.as_str() {
                if let Some(field) = obj.get(name) {
                    data.insert(
                        Value::String(name.to_string()),
                        serde_yaml::to_value(field)?,
                    );
                }
            }
        }
    }
    Ok(data)
}

/// Render collected field data as a Markdown file with YAML front-matter.
pub(crate) fn render_markdown(data: &serde_yaml::Mapping) -> Result<String> {
    let yaml_str = serde_yaml::to_string(data)?;
    Ok(format!("---\n{yaml_str}---\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{test_config, ScriptedPrompt, StrSchema, TestPlatform};

    const SCHEMA: &str = "\
type: object
additionalProperties: false
required: [date, name, quantity]
properties:
  date: { type: string }
  name: { type: string, minLength: 1 }
  quantity: { type: number, minimum: 0 }
";

    fn add_args(json: Option<&str>) -> AddArgs {
        AddArgs {
            json: json.map(String::from),
            dry_run: false,
            no_sync: true,
        }
    }

    fn platform() -> TestPlatform {
        TestPlatform {
            schema: StrSchema(SCHEMA.into()),
            ..TestPlatform::default()
        }
    }

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

    #[test]
    fn valid_json_writes_file_in_schema_order() {
        let p = platform();
        // Keys deliberately out of schema order to prove we re-order them.
        let json = r#"{"quantity": 2, "name": "Milk", "date": "2026-06-05T10:00:00"}"#;
        run(add_args(Some(json)), &test_config(), &p).unwrap();

        let docs = p.transactions.docs.borrow();
        assert_eq!(docs.len(), 1, "exactly one file written");
        assert_eq!(
            docs[0].path,
            std::path::PathBuf::from("/mem/20260605T100000-milk.md")
        );

        let body = &docs[0].content;
        let date_at = body.find("date:").unwrap();
        let name_at = body.find("name:").unwrap();
        let qty_at = body.find("quantity:").unwrap();
        assert!(
            date_at < name_at && name_at < qty_at,
            "fields in schema order"
        );
    }

    #[test]
    fn missing_required_field_returns_validation_error_and_writes_nothing() {
        let p = platform();
        let json = r#"{"name": "Milk", "quantity": 1}"#; // no `date`
        let err = run(add_args(Some(json)), &test_config(), &p).unwrap_err();
        assert!(matches!(err, Error::Validation(_)));
        assert!(p.transactions.docs.borrow().is_empty(), "no file written");
    }

    #[test]
    fn wrong_type_returns_validation_error() {
        let p = platform();
        let json = r#"{"date": "2026-06-05T10:00:00", "name": "Milk", "quantity": "lots"}"#;
        let err = run(add_args(Some(json)), &test_config(), &p).unwrap_err();
        assert!(matches!(err, Error::Validation(_)));
        assert!(p.transactions.docs.borrow().is_empty());
    }

    #[test]
    fn malformed_json_returns_json_error() {
        let p = platform();
        let err = run(add_args(Some("{not json")), &test_config(), &p).unwrap_err();
        assert!(matches!(err, Error::Json(_)));
        assert!(p.transactions.docs.borrow().is_empty());
    }

    #[test]
    fn non_object_json_returns_parse_error() {
        let p = platform();
        let err = run(add_args(Some("[1, 2, 3]")), &test_config(), &p).unwrap_err();
        assert!(matches!(err, Error::Parse(_)));
        assert!(p.transactions.docs.borrow().is_empty());
    }

    #[test]
    fn dry_run_valid_json_writes_nothing() {
        let p = platform();
        let json = r#"{"date": "2026-06-05T10:00:00", "name": "Milk", "quantity": 1}"#;
        let mut args = add_args(Some(json));
        args.dry_run = true;
        run(args, &test_config(), &p).unwrap();
        assert!(
            p.transactions.docs.borrow().is_empty(),
            "dry-run writes nothing"
        );
    }

    #[test]
    fn dash_reads_json_from_stdin() {
        let json = r#"{"date": "2026-06-05T10:00:00", "name": "Milk", "quantity": 1}"#;
        let p = TestPlatform {
            schema: StrSchema(SCHEMA.into()),
            prompt: ScriptedPrompt::with_stdin(json),
            ..TestPlatform::default()
        };
        run(add_args(Some("-")), &test_config(), &p).unwrap();
        assert_eq!(p.transactions.docs.borrow().len(), 1);
    }
}
