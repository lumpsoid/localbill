//! Interactive command to create a new invoice entry from the configured schema.
//!
//! Thin orchestration only: load the schema, delegate field collection to
//! [`crate::schema_form`] (which owns the schema walking, validation, and the
//! datetime offset/component UX), then render the Markdown front-matter, pick a
//! collision-free filename, persist it, and — unless `--no-sync` — sync.

use serde_yaml::Value;

use crate::cli::{AddArgs, SyncArgs};
use crate::config::Config;
use crate::error::Result;
use crate::invoice::mapper;
use crate::ports::{Platform, Reporter, SchemaSource, Style, Styler, TransactionStore};
use crate::schema_form;

pub fn run<P: Platform>(args: AddArgs, config: &Config, p: &P) -> Result<()> {
    let schema_content = p.schema().load()?;
    let schema: Value = serde_yaml::from_str(&schema_content)?;

    let data = schema_form::collect(&schema, p.prompt(), p.reporter(), p.clock(), p.styler())?;

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

/// Render collected field data as a Markdown file with YAML front-matter.
pub(crate) fn render_markdown(data: &serde_yaml::Mapping) -> Result<String> {
    let yaml_str = serde_yaml::to_string(data)?;
    Ok(format!("---\n{yaml_str}---\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

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
