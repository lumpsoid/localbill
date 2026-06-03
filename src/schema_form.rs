//! General, schema-driven interactive form.
//!
//! Walks an arbitrary JSON Schema (`properties` + `required`) and collects a
//! validated value for each field through the [`Prompt`], [`Reporter`],
//! [`Clock`] and [`Styler`] ports, returning a [`serde_yaml::Mapping`].
//!
//! It is deliberately decoupled from the `add` command: this module owns *what
//! a schema can express and how it is validated/prompted* (every JSON-Schema
//! scalar, datetime, enum, and nested string/object arrays), while `add` owns
//! orchestration (load schema, render Markdown, name the file, persist, sync).
//! Keeping the two apart makes the schema walking + validation + datetime UX
//! reusable and unit-testable without touching the filesystem or git.

use serde_yaml::{Mapping, Value};
use time::macros::format_description;
use time::{Duration, PrimitiveDateTime};

use crate::error::{Error, Result};
use crate::ports::{Clock, Prompt, Reporter, Style, Styler};

// ── Public entry ──────────────────────────────────────────────────────────────

/// Walk `schema.properties` in order, prompting for each field, and return the
/// collected values. Required fields loop until valid; optional fields accept an
/// empty line to skip (datetime fields treat an empty line as "accept prefill").
pub fn collect(
    schema: &Value,
    prompt: &impl Prompt,
    reporter: &impl Reporter,
    clock: &impl Clock,
    styler: &impl Styler,
) -> Result<Mapping> {
    let properties = schema
        .get("properties")
        .and_then(|v| v.as_mapping())
        .ok_or_else(|| Error::Parse("Schema missing 'properties'".to_string()))?;
    let required_list = required_of(schema);
    let total = properties.len();

    reporter.status(&styler.paint(
        Style::Header,
        &format!("\n  📝 New entry  ({total} fields)\n"),
    ));

    let mut data = Mapping::new();
    for (idx, (key_val, prop_def)) in properties.iter().enumerate() {
        let key = key_val.as_str().unwrap_or("");
        let required = required_list.contains(&key);
        let prefix = format!("[{}/{}] ", idx + 1, total);
        if let Some(value) = prompt_field(
            &prefix, key, prop_def, required, prompt, reporter, clock, styler,
        )? {
            data.insert(key_val.clone(), value);
        }
    }
    Ok(data)
}

// ── Schema walk ────────────────────────────────────────────────────────────────

/// Collect a single field's value, dispatching on its JSON-Schema `type`.
///
/// Returns `None` only for an optional scalar the user chose to skip; arrays and
/// objects are always included so the document structure stays explicit.
#[allow(clippy::too_many_arguments)]
fn prompt_field(
    prefix: &str,
    name: &str,
    prop: &Value,
    required: bool,
    prompt: &impl Prompt,
    reporter: &impl Reporter,
    clock: &impl Clock,
    styler: &impl Styler,
) -> Result<Option<Value>> {
    let type_str = prop
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("string");

    match type_str {
        "array" => {
            reporter.status(&name_line(prefix, name, false, styler));
            let items = prop.get("items");
            let item_type = items
                .and_then(|v| v.get("type"))
                .and_then(|v| v.as_str())
                .unwrap_or("string");
            let seq = if item_type == "object" {
                let item_props = items
                    .and_then(|v| v.get("properties"))
                    .and_then(|v| v.as_mapping());
                let item_required = items.map(required_of).unwrap_or_default();
                prompt_object_array(
                    name,
                    item_props,
                    &item_required,
                    prompt,
                    reporter,
                    clock,
                    styler,
                )?
            } else {
                prompt_string_array(name, prompt, reporter, styler)?
            };
            Ok(Some(Value::Sequence(seq)))
        }
        "object" => {
            reporter.status(&name_line(prefix, name, false, styler));
            let mut obj = Mapping::new();
            let req = required_of(prop);
            if let Some(props) = prop.get("properties").and_then(|v| v.as_mapping()) {
                for (k, prop_def) in props {
                    let key = k.as_str().unwrap_or("");
                    if let Some(v) = prompt_field(
                        "    ",
                        &format!("{name}.{key}"),
                        prop_def,
                        req.contains(&key),
                        prompt,
                        reporter,
                        clock,
                        styler,
                    )? {
                        obj.insert(k.clone(), v);
                    }
                }
            }
            Ok(Some(Value::Mapping(obj)))
        }
        _ => prompt_scalar(
            prefix, name, prop, required, type_str, prompt, reporter, clock, styler,
        ),
    }
}

/// Prompt for a single scalar value (string / number / integer / boolean /
/// datetime), looping until valid. Datetime fields delegate to the dedicated
/// offset/component sub-prompt.
#[allow(clippy::too_many_arguments)]
fn prompt_scalar(
    prefix: &str,
    name: &str,
    prop: &Value,
    required: bool,
    type_str: &str,
    prompt: &impl Prompt,
    reporter: &impl Reporter,
    clock: &impl Clock,
    styler: &impl Styler,
) -> Result<Option<Value>> {
    let format = prop.get("format").and_then(|v| v.as_str());

    // Datetime: the rich prefill/offset/component flow always yields a value.
    if format == Some("datetime") {
        reporter.status(&name_line(prefix, name, required, styler));
        let value = prompt_datetime(prompt, reporter, clock, styler)?;
        reporter.status(&styler.paint(Style::Success, &format!("    ✓ {value}")));
        return Ok(Some(Value::String(value)));
    }

    let pattern = prop.get("pattern").and_then(|v| v.as_str());
    let min_length = prop.get("minLength").and_then(|v| v.as_u64());
    let minimum = prop.get("minimum").and_then(|v| v.as_f64());
    let maximum = prop.get("maximum").and_then(|v| v.as_f64());
    let enum_vals: Option<Vec<&str>> = prop
        .get("enum")
        .and_then(|v| v.as_sequence())
        .map(|s| s.iter().filter_map(|v| v.as_str()).collect());

    let hint = build_hint(
        format,
        pattern,
        minimum,
        maximum,
        enum_vals.as_deref(),
        required,
    );

    if let Some(vals) = &enum_vals {
        for (i, opt) in vals.iter().enumerate() {
            reporter.status(&styler.paint(Style::Hint, &format!("    {}. {opt}", i + 1)));
        }
    }

    loop {
        let raw = prompt.read_line(&field_prompt(prefix, name, required, &hint, styler))?;
        // Enum numeric shortcut: typing "2" selects the second option.
        let input = resolve_enum(&raw, enum_vals.as_deref());

        if input.is_empty() {
            if required {
                reporter
                    .status(&styler.paint(Style::Error, &format!("    ✗ '{name}' is required")));
                continue;
            }
            return Ok(None);
        }

        match type_str {
            "number" => match validate_number_input(&input, minimum, maximum) {
                Ok(n) => return Ok(Some(Value::Number(serde_yaml::Number::from(n)))),
                Err(msg) => reporter.status(&styler.paint(Style::Error, &format!("    ✗ {msg}"))),
            },
            "integer" => match validate_integer_input(&input, minimum, maximum) {
                Ok(n) => return Ok(Some(Value::Number(serde_yaml::Number::from(n)))),
                Err(msg) => reporter.status(&styler.paint(Style::Error, &format!("    ✗ {msg}"))),
            },
            "boolean" => match parse_bool(&input) {
                Some(b) => return Ok(Some(Value::Bool(b))),
                None => reporter.status(&styler.paint(Style::Error, "    ✗ enter y/n")),
            },
            _ => match validate_string_input(
                &input,
                min_length,
                pattern,
                enum_vals.as_deref(),
                format,
            )? {
                Some(msg) => reporter.status(&styler.paint(Style::Error, &format!("    ✗ {msg}"))),
                None => return Ok(Some(Value::String(input))),
            },
        }
    }
}

/// Prompt for an array of plain scalars (e.g. `tags`).
fn prompt_string_array(
    name: &str,
    prompt: &impl Prompt,
    reporter: &impl Reporter,
    styler: &impl Styler,
) -> Result<Vec<Value>> {
    reporter.status(&styler.paint(
        Style::Hint,
        &format!("  {name}: enter items one per line; empty line to finish."),
    ));
    let mut items: Vec<Value> = Vec::new();
    loop {
        let suffix = if items.is_empty() {
            " (or Enter to skip)"
        } else {
            ""
        };
        let label = styler.paint(
            Style::Prompt,
            &format!("  {name}[{}]{suffix}: ", items.len()),
        );
        let input = prompt.read_line(&label)?;
        if input.is_empty() {
            break;
        }
        items.push(Value::String(input));
    }
    Ok(items)
}

/// Prompt for an array of objects (e.g. `exchange`, `fees`, `discounts`),
/// recursing into each item's properties.
#[allow(clippy::too_many_arguments)]
fn prompt_object_array(
    array_name: &str,
    item_props: Option<&Mapping>,
    item_required: &[&str],
    prompt: &impl Prompt,
    reporter: &impl Reporter,
    clock: &impl Clock,
    styler: &impl Styler,
) -> Result<Vec<Value>> {
    let mut items: Vec<Value> = Vec::new();
    loop {
        let q = if items.is_empty() {
            format!("Add a {array_name} entry? [y/N]: ")
        } else {
            format!("Add another {array_name} entry? [y/N]: ")
        };
        let response = prompt.read_line(&styler.paint(Style::Prompt, &q))?;
        if !matches!(response.to_lowercase().as_str(), "y" | "yes") {
            break;
        }

        let mut item = Mapping::new();
        if let Some(props) = item_props {
            for (key_val, prop_def) in props {
                let key = key_val.as_str().unwrap_or("");
                if let Some(value) = prompt_field(
                    "    ",
                    &format!("{array_name}.{key}"),
                    prop_def,
                    item_required.contains(&key),
                    prompt,
                    reporter,
                    clock,
                    styler,
                )? {
                    item.insert(key_val.clone(), value);
                }
            }
        }
        items.push(Value::Mapping(item));
    }
    Ok(items)
}

// ── Datetime sub-prompt ─────────────────────────────────────────────────────────

/// Interactive datetime entry: shows the current timestamp pre-filled and lets
/// the user accept it (Enter), edit a component (`d`/`t`/`dt`), or apply offset
/// expressions like `+1d -2h +30m`.
fn prompt_datetime(
    prompt: &impl Prompt,
    reporter: &impl Reporter,
    clock: &impl Clock,
    styler: &impl Styler,
) -> Result<String> {
    let prefill = current_datetime_prefill(clock);

    // If the clock didn't yield a usable datetime, degrade to a plain prompt so
    // the component slicing below stays panic-free.
    if !looks_like_datetime(&prefill) {
        loop {
            let raw = prompt
                .read_line(&styler.paint(Style::Prompt, "  datetime (YYYY-MM-DDTHH:MM:SS) > "))?;
            if looks_like_datetime(&raw) {
                return Ok(raw);
            }
            reporter.status(&styler.paint(Style::Error, "    ✗ expected YYYY-MM-DDTHH:MM:SS"));
        }
    }

    loop {
        reporter.status(&styler.paint(Style::Hint, &format!("  current: {prefill}")));
        reporter.status(&styler.paint(
            Style::Hint,
            "  Enter accept · d date · t time · dt full · offsets like +1d -2h +30m",
        ));
        let choice = prompt.read_line(&styler.paint(Style::Prompt, "  > "))?;

        match choice.as_str() {
            "" => return Ok(prefill),
            "d" => {
                let current = &prefill[..10];
                let raw = read_component(prompt, reporter, styler, "date (YYYY-MM-DD)", current)?;
                let new_date = if raw.is_empty() {
                    current.to_string()
                } else if looks_like_offset_expr(&raw) {
                    apply_offsets(&prefill, &raw, &prefill)?[..10].to_string()
                } else {
                    raw
                };
                let candidate = format!("{new_date}T{}", &prefill[11..]);
                if looks_like_datetime(&candidate) {
                    return Ok(candidate);
                }
                reporter.status(&styler.paint(Style::Error, "    ✗ invalid date"));
            }
            "t" => {
                let current = &prefill[11..];
                let raw = read_component(
                    prompt,
                    reporter,
                    styler,
                    "time (HH:MM or HH:MM:SS)",
                    current,
                )?;
                let new_time = if raw.is_empty() {
                    current.to_string()
                } else if looks_like_offset_expr(&raw) {
                    apply_offsets(&prefill, &raw, &prefill)?[11..].to_string()
                } else {
                    normalize_time(&raw)
                };
                let candidate = format!("{}T{new_time}", &prefill[..10]);
                if looks_like_datetime(&candidate) {
                    return Ok(candidate);
                }
                reporter.status(
                    &styler.paint(Style::Error, "    ✗ invalid time (use HH:MM or HH:MM:SS)"),
                );
            }
            "dt" => {
                let raw = read_component(
                    prompt,
                    reporter,
                    styler,
                    "full (YYYY-MM-DDTHH:MM:SS)",
                    &prefill,
                )?;
                let candidate = if raw.is_empty() {
                    prefill.clone()
                } else if looks_like_offset_expr(&raw) {
                    apply_offsets(&prefill, &raw, &prefill)?
                } else {
                    raw
                };
                if looks_like_datetime(&candidate) {
                    return Ok(candidate);
                }
                reporter.status(&styler.paint(Style::Error, "    ✗ invalid datetime"));
            }
            _ if looks_like_offset_expr(&choice) => {
                let candidate = apply_offsets(&prefill, &choice, &prefill)?;
                if looks_like_datetime(&candidate) {
                    return Ok(candidate);
                }
                reporter.status(&styler.paint(Style::Error, "    ✗ resulting datetime is invalid"));
            }
            _ => reporter.status(&styler.paint(
                Style::Error,
                "    ✗ type Enter, d, t, dt, or an offset like +1d",
            )),
        }
    }
}

fn read_component(
    prompt: &impl Prompt,
    reporter: &impl Reporter,
    styler: &impl Styler,
    label: &str,
    current: &str,
) -> Result<String> {
    reporter.status(&styler.paint(Style::Hint, &format!("  {label}  [current: {current}]")));
    prompt.read_line(&styler.paint(Style::Prompt, "  > "))
}

// ── Display helpers ─────────────────────────────────────────────────────────────

/// `"[1/8] name *"` — the header line for arrays/objects/datetime fields.
fn name_line(prefix: &str, name: &str, required: bool, styler: &impl Styler) -> String {
    let marker = if required { " *" } else { "" };
    format!(
        "{}{}",
        styler.paint(Style::Hint, prefix),
        styler.paint(Style::Field, &format!("{name}{marker}"))
    )
}

/// `"[1/8] name *  (hint)\n  > "` — the full read prompt for scalar fields.
fn field_prompt(
    prefix: &str,
    name: &str,
    required: bool,
    hint: &str,
    styler: &impl Styler,
) -> String {
    let marker = if required { " *" } else { "" };
    let head = styler.paint(Style::Field, &format!("{name}{marker}"));
    let h = if hint.is_empty() {
        String::new()
    } else {
        styler.paint(Style::Hint, hint)
    };
    format!(
        "{}{head}{h}\n  {}",
        styler.paint(Style::Hint, prefix),
        styler.paint(Style::Prompt, "> ")
    )
}

// ── Pure helpers ────────────────────────────────────────────────────────────────

/// The `required` field names of a schema node.
fn required_of(node: &Value) -> Vec<&str> {
    node.get("required")
        .and_then(|v| v.as_sequence())
        .map(|seq| seq.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default()
}

/// Map a numeric enum shortcut (`"2"`) to its option; otherwise return `input`.
fn resolve_enum(input: &str, enum_vals: Option<&[&str]>) -> String {
    if let Some(vals) = enum_vals {
        if let Ok(n) = input.parse::<usize>() {
            if (1..=vals.len()).contains(&n) {
                return vals[n - 1].to_string();
            }
        }
    }
    input.to_string()
}

fn parse_bool(s: &str) -> Option<bool> {
    match s.to_lowercase().as_str() {
        "y" | "yes" | "true" | "1" => Some(true),
        "n" | "no" | "false" | "0" => Some(false),
        _ => None,
    }
}

/// `"HH:MM"` → `"HH:MM:00"`; otherwise unchanged.
fn normalize_time(t: &str) -> String {
    if t.len() == 5 {
        format!("{t}:00")
    } else {
        t.to_string()
    }
}

/// Build the inline hint string shown next to a field prompt. Empty when there
/// is nothing to show.
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

/// Parse and range-check a floating-point input.
pub(crate) fn validate_number_input(
    input: &str,
    minimum: Option<f64>,
    maximum: Option<f64>,
) -> std::result::Result<f64, String> {
    let n: f64 = input
        .parse()
        .map_err(|_| "Please enter a valid number.".to_string())?;
    range_check(n, minimum, maximum)?;
    Ok(n)
}

/// Parse and range-check a whole-number input.
pub(crate) fn validate_integer_input(
    input: &str,
    minimum: Option<f64>,
    maximum: Option<f64>,
) -> std::result::Result<i64, String> {
    let n: i64 = input
        .parse()
        .map_err(|_| "Please enter a whole number.".to_string())?;
    range_check(n as f64, minimum, maximum)?;
    Ok(n)
}

fn range_check(
    n: f64,
    minimum: Option<f64>,
    maximum: Option<f64>,
) -> std::result::Result<(), String> {
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
    Ok(())
}

/// Validate a string input against schema constraints.
///
/// - `Ok(None)` — valid.
/// - `Ok(Some(message))` — invalid; show the message and re-prompt.
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
            Ok(false) => return Ok(Some(format!("Does not match required pattern: {pat}"))),
            Err(e) => return Err(Error::Parse(format!("Invalid schema pattern '{pat}': {e}"))),
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

/// Compile `pattern` as a regex and test it against `input`. `Err` if the
/// pattern itself is invalid (a schema bug).
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

/// True when every whitespace-separated token is `now` or a signed offset
/// (`+1d`, `-2h`); empty input is not an offset expression.
pub(crate) fn looks_like_offset_expr(s: &str) -> bool {
    let tokens: Vec<&str> = s.split_whitespace().collect();
    !tokens.is_empty()
        && tokens
            .iter()
            .all(|t| *t == "now" || t.starts_with('+') || t.starts_with('-'))
}

/// Apply a chain of offset tokens to `base_dt`. `now` resets to the `now`
/// argument (sourced from the Clock port — never a wall-clock call here, so this
/// stays pure and testable). Units: `d`, `h`, `m`, `s`.
pub(crate) fn apply_offsets(base_dt: &str, expr: &str, now: &str) -> Result<String> {
    let fmt = format_description!("[year]-[month]-[day]T[hour]:[minute]:[second]");
    let parse = |s: &str| {
        PrimitiveDateTime::parse(s, &fmt)
            .map_err(|e| Error::Parse(format!("Cannot parse datetime '{s}': {e}")))
    };

    let mut dt = parse(base_dt)?;
    for token in expr.split_whitespace() {
        if token == "now" {
            dt = parse(now)?;
            continue;
        }
        let (sign, rest) = if let Some(r) = token.strip_prefix('+') {
            (1i64, r)
        } else if let Some(r) = token.strip_prefix('-') {
            (-1, r)
        } else {
            return Err(Error::Parse(format!(
                "Unrecognised offset token: '{token}'"
            )));
        };
        let (num, unit) = rest.split_at(rest.len().saturating_sub(1));
        let quantity: i64 = num
            .parse::<i64>()
            .map(|n| n * sign)
            .map_err(|_| Error::Parse(format!("Invalid offset token: '{token}'")))?;
        dt += match unit {
            "d" => Duration::days(quantity),
            "h" => Duration::hours(quantity),
            "m" => Duration::minutes(quantity),
            "s" => Duration::seconds(quantity),
            _ => {
                return Err(Error::Parse(format!(
                    "Unknown unit in '{token}' — use d, h, m, s"
                )))
            }
        };
    }
    dt.format(&fmt).map_err(|e| Error::Parse(e.to_string()))
}

/// Current local time as `YYYY-MM-DDTHH:MM:SS`, sourced from the Clock port.
fn current_datetime_prefill(clock: &impl Clock) -> String {
    to_datetime_format(&clock.timestamp())
}

/// Convert the Clock's `"YYYY-MM-DD HH:MM:SS"` to the schema's `T`-separated
/// datetime form. Leaves anything else untouched (e.g. a clock failure string).
fn to_datetime_format(ts: &str) -> String {
    match ts.find(' ') {
        Some(10) => format!("{}T{}", &ts[..10], &ts[11..]),
        _ => ts.to_string(),
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{FakeStyler, FixedClock, RecordingReporter, ScriptedPrompt};

    fn schema(yaml: &str) -> Value {
        serde_yaml::from_str(yaml).unwrap()
    }

    fn run(yaml: &str, answers: Vec<&str>) -> Mapping {
        collect(
            &schema(yaml),
            &ScriptedPrompt::with(answers),
            &RecordingReporter::default(),
            &FixedClock, // "2024-01-01 00:00:00"
            &FakeStyler,
        )
        .unwrap()
    }

    // ── collect: end-to-end through the ports ──────────────────────────────────

    #[test]
    fn collect_datetime_accept_prefill_and_string() {
        let data = run(
            "properties:\n  date: {type: string, format: datetime}\n  name: {type: string, minLength: 1}\nrequired: [date, name]\n",
            vec!["", "Coffee"], // accept prefill, then name
        );
        assert_eq!(
            data.get("date").unwrap().as_str().unwrap(),
            "2024-01-01T00:00:00"
        );
        assert_eq!(data.get("name").unwrap().as_str().unwrap(), "Coffee");
    }

    #[test]
    fn collect_datetime_offset_expression() {
        let data = run(
            "properties:\n  date: {type: string, format: datetime}\nrequired: [date]\n",
            vec!["+1d +2h"],
        );
        assert_eq!(
            data.get("date").unwrap().as_str().unwrap(),
            "2024-01-02T02:00:00"
        );
    }

    #[test]
    fn collect_enum_numeric_shortcut() {
        let data = run(
            "properties:\n  kind: {type: string, enum: [percentage, fixed]}\nrequired: [kind]\n",
            vec!["2"],
        );
        assert_eq!(data.get("kind").unwrap().as_str().unwrap(), "fixed");
    }

    #[test]
    fn collect_number_and_boolean_and_integer() {
        let data = run(
            "properties:\n  qty: {type: integer, minimum: 0}\n  price: {type: number}\n  paid: {type: boolean}\nrequired: [qty, price, paid]\n",
            vec!["3", "9.5", "yes"],
        );
        assert_eq!(data.get("qty").unwrap().as_i64().unwrap(), 3);
        assert_eq!(data.get("price").unwrap().as_f64().unwrap(), 9.5);
        assert!(data.get("paid").unwrap().as_bool().unwrap());
    }

    #[test]
    fn collect_skips_empty_optional_scalar() {
        let data = run(
            "properties:\n  name: {type: string, minLength: 1}\n  notes: {type: string}\nrequired: [name]\n",
            vec!["Item", ""], // name, then skip optional notes
        );
        assert!(data.contains_key("name"));
        assert!(!data.contains_key("notes"));
    }

    #[test]
    fn collect_object_array_recurses() {
        let data = run(
            concat!(
                "properties:\n",
                "  fees:\n",
                "    type: array\n",
                "    items:\n",
                "      type: object\n",
                "      required: [fee, amount]\n",
                "      properties:\n",
                "        fee: {type: string}\n",
                "        amount: {type: number}\n",
                "required: []\n",
            ),
            vec!["y", "service", "1.5", "n"], // add one entry, then stop
        );
        let fees = data.get("fees").unwrap().as_sequence().unwrap();
        assert_eq!(fees.len(), 1);
        let first = fees[0].as_mapping().unwrap();
        assert_eq!(first.get("fee").unwrap().as_str().unwrap(), "service");
        assert_eq!(first.get("amount").unwrap().as_f64().unwrap(), 1.5);
    }

    #[test]
    fn collect_string_array_collects_until_empty() {
        let data = run(
            "properties:\n  tags: {type: array, items: {type: string}}\nrequired: []\n",
            vec!["a", "b", ""],
        );
        let tags = data.get("tags").unwrap().as_sequence().unwrap();
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0].as_str().unwrap(), "a");
    }

    // ── apply_offsets ──────────────────────────────────────────────────────────

    #[test]
    fn offsets_plus_one_day() {
        assert_eq!(
            apply_offsets("2026-04-03T14:00:00", "+1d", "2000-01-01T00:00:00").unwrap(),
            "2026-04-04T14:00:00"
        );
    }

    #[test]
    fn offsets_minus_two_hours() {
        assert_eq!(
            apply_offsets("2026-04-03T14:00:00", "-2h", "2000-01-01T00:00:00").unwrap(),
            "2026-04-03T12:00:00"
        );
    }

    #[test]
    fn offsets_multiple_tokens_cross_boundaries() {
        assert_eq!(
            apply_offsets("2026-04-03T00:00:01", "+1d +1h -1s", "x").unwrap(),
            "2026-04-04T01:00:00"
        );
    }

    #[test]
    fn offsets_now_resets_to_now_argument() {
        assert_eq!(
            apply_offsets("2000-01-01T00:00:00", "now", "2026-06-03T14:00:00").unwrap(),
            "2026-06-03T14:00:00"
        );
    }

    #[test]
    fn offsets_now_then_shift() {
        assert_eq!(
            apply_offsets("2000-01-01T00:00:00", "now +1h", "2026-06-03T14:00:00").unwrap(),
            "2026-06-03T15:00:00"
        );
    }

    #[test]
    fn offsets_invalid_unit_errors() {
        assert!(apply_offsets("2026-04-03T00:00:00", "+1y", "x").is_err());
    }

    #[test]
    fn offsets_bad_number_errors() {
        assert!(apply_offsets("2026-04-03T00:00:00", "+xd", "x").is_err());
    }

    // ── looks_like_offset_expr ─────────────────────────────────────────────────

    #[test]
    fn offset_expr_recognises_valid() {
        assert!(looks_like_offset_expr("+1d -2h +30m"));
        assert!(looks_like_offset_expr("now +1d"));
        assert!(looks_like_offset_expr("-1s"));
    }

    #[test]
    fn offset_expr_rejects_invalid() {
        assert!(!looks_like_offset_expr(""));
        assert!(!looks_like_offset_expr("2026-04-03"));
        assert!(!looks_like_offset_expr("foo +1d"));
    }

    // ── to_datetime_format ─────────────────────────────────────────────────────

    #[test]
    fn datetime_format_swaps_space_for_t() {
        assert_eq!(
            to_datetime_format("2024-01-01 00:00:00"),
            "2024-01-01T00:00:00"
        );
    }

    #[test]
    fn datetime_format_leaves_failure_string() {
        assert_eq!(to_datetime_format("unknown"), "unknown");
    }

    #[test]
    fn prefill_is_a_valid_datetime() {
        assert!(looks_like_datetime(&current_datetime_prefill(&FixedClock)));
    }

    // ── looks_like_datetime ────────────────────────────────────────────────────

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

    // ── validate_pattern ───────────────────────────────────────────────────────

    #[test]
    fn pattern_currency_valid() {
        assert!(validate_pattern("USD", "^[A-Z]{3}$").unwrap());
        assert!(validate_pattern("RSD", "^[A-Z]{3}$").unwrap());
    }

    #[test]
    fn pattern_currency_lowercase_rejected() {
        assert!(!validate_pattern("usd", "^[A-Z]{3}$").unwrap());
    }

    #[test]
    fn pattern_invalid_regex_returns_err() {
        assert!(validate_pattern("x", "[invalid").is_err());
    }

    // ── build_hint ─────────────────────────────────────────────────────────────

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

    // ── validate_number_input / validate_integer_input ─────────────────────────

    #[test]
    fn number_valid_no_constraints() {
        assert_eq!(validate_number_input("2.5", None, None).unwrap(), 2.5);
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
    fn number_above_maximum() {
        assert!(validate_number_input("101", None, Some(100.0)).is_err());
    }

    #[test]
    fn integer_rejects_fractional() {
        assert!(validate_integer_input("2.5", None, None).is_err());
    }

    #[test]
    fn integer_range_checked() {
        assert!(validate_integer_input("5", Some(0.0), Some(3.0)).is_err());
        assert_eq!(
            validate_integer_input("2", Some(0.0), Some(3.0)).unwrap(),
            2
        );
    }

    // ── validate_string_input ──────────────────────────────────────────────────

    #[test]
    fn string_too_short() {
        let msg = validate_string_input("x", Some(3), None, None, None)
            .unwrap()
            .unwrap();
        assert!(msg.contains('3'));
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
    fn string_enum_invalid() {
        let msg = validate_string_input("other", None, None, Some(&["percentage", "fixed"]), None)
            .unwrap()
            .unwrap();
        assert!(msg.contains("percentage"));
    }

    // ── resolve_enum / parse_bool / normalize_time ─────────────────────────────

    #[test]
    fn enum_numeric_resolves_in_range() {
        assert_eq!(resolve_enum("2", Some(&["a", "b", "c"])), "b");
    }

    #[test]
    fn enum_numeric_out_of_range_passes_through() {
        assert_eq!(resolve_enum("9", Some(&["a", "b"])), "9");
        assert_eq!(resolve_enum("a", Some(&["a", "b"])), "a");
    }

    #[test]
    fn bool_parsing() {
        assert_eq!(parse_bool("Yes"), Some(true));
        assert_eq!(parse_bool("0"), Some(false));
        assert_eq!(parse_bool("maybe"), None);
    }

    #[test]
    fn time_normalisation_pads_seconds() {
        assert_eq!(normalize_time("14:30"), "14:30:00");
        assert_eq!(normalize_time("14:30:15"), "14:30:15");
    }
}
