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

    let yaml_str = serde_yaml::to_string(&data)?;
    let markdown = format!("---\n{yaml_str}---\n");

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

// ── Field-level prompts ───────────────────────────────────────────────────────

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

    // Assemble a compact hint shown alongside the field name.
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
    if let Some(vals) = &enum_vals {
        hints.push(format!("one of: {}", vals.join(" | ")));
    }
    if !required {
        hints.push("optional".to_string());
    }
    let hint_str = if hints.is_empty() {
        String::new()
    } else {
        format!(" ({})", hints.join(", "))
    };

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
            "number" => match input.parse::<f64>() {
                Ok(n) => {
                    if let Some(min) = minimum {
                        if n < min {
                            eprintln!("  Value must be >= {min}");
                            continue;
                        }
                    }
                    if let Some(max) = maximum {
                        if n > max {
                            eprintln!("  Value must be <= {max}");
                            continue;
                        }
                    }
                    return Ok(Some(Value::Number(serde_yaml::Number::from(n))));
                }
                Err(_) => {
                    eprintln!("  Please enter a valid number.");
                    continue;
                }
            },
            _ => {
                // string (and any other type treated as string)
                if let Some(min_len) = min_length {
                    if input.len() < min_len as usize {
                        eprintln!("  Must be at least {min_len} character(s).");
                        continue;
                    }
                }
                if let Some(pat) = pattern {
                    match validate_pattern(&input, pat) {
                        Ok(false) => {
                            eprintln!("  Does not match required pattern: {pat}");
                            continue;
                        }
                        Err(e) => {
                            return Err(Error::Parse(format!(
                                "Invalid schema pattern '{pat}': {e}"
                            )));
                        }
                        Ok(true) => {}
                    }
                }
                if let Some(vals) = &enum_vals {
                    if !vals.contains(&input.as_str()) {
                        eprintln!("  Must be one of: {}", vals.join(", "));
                        continue;
                    }
                }
                if format == Some("datetime") && !looks_like_datetime(&input) {
                    eprintln!("  Expected datetime format: YYYY-MM-DDTHH:MM:SS");
                    continue;
                }
                return Ok(Some(Value::String(input)));
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

// ── Validators ────────────────────────────────────────────────────────────────

/// Compile `pattern` as a regex and test it against `input`.
/// Returns `Err` if the pattern itself is invalid (schema bug).
fn validate_pattern(input: &str, pattern: &str) -> std::result::Result<bool, regex::Error> {
    let re = regex::Regex::new(pattern)?;
    Ok(re.is_match(input))
}

/// Minimal datetime check: at least 19 chars with `T` at position 10.
fn looks_like_datetime(s: &str) -> bool {
    s.len() >= 19 && s.as_bytes().get(10) == Some(&b'T')
}
