# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`localbill` is a single-binary Rust CLI that parses **Serbian fiscal invoice URLs** (`suf.purs.gov.rs`), explodes each invoice into one Markdown-with-YAML-front-matter file per line item, and stores them in a Git-backed data directory. The Git repo *is* the database — there is no other persistence layer.

## Commands

```bash
cargo build                 # debug build
cargo build --release       # release binary at target/release/localbill
cargo test                  # run all unit tests (no integration/e2e suite)
cargo test parse_date       # run a single test by name substring
cargo test --lib commands::add::tests   # run one module's tests
cargo clippy                # lint
cargo fmt                   # format
```

External dependencies are injected through traits (see **Ports & adapters** below), so the HTTP/git/filesystem/queue paths **are** unit-testable via `src/testing.rs`'s `TestPlatform` and its in-memory fakes (`FakeHttp`, `MemTransactions`, `FakeVcs`, …). Tests still split pure logic from orchestration: pure helpers (date/price parsing, slugify, schema-driven validation, `extract_*` front-matter) get direct unit tests; command flows get `TestPlatform`-driven tests. When adding behavior, keep external I/O behind a port and the decision logic in the command/pure helper, mirroring `commands/add.rs` (pure validators + `Prompt`/`Reporter` ports) and `invoice/parser.rs` (pure `parse_date`/`parse_price`/`extract_token` vs. `try_parse` over `&impl Http`).

## Architecture

### Ports & adapters (dependency injection)

Every external system sits behind a capability trait in `src/ports/mod.rs` (`Http`, `Vcs`, `RemoteReachable`, `Network`, `Clock`, `Prompt`, `Reporter`, `Env`, `TransactionStore`, `QueueStore`, `RemoteQueue`, `FailedLog`, `SchemaSource`). The `Platform` supertrait bundles one concrete adapter per port via associated types. **Dispatch is fully static** — commands are `fn run<P: Platform>(args, &Config, &P)`, monomorphised; no `dyn`/`Box`. Leaf functions take only the narrow trait they need (e.g. `parser::parse(url, http: &impl Http)`).

- `src/adapters/` holds the production impls — each module is the **only** place a given external crate/syscall appears: `http.rs` (`ureq`), `vcs.rs` (`git` subprocess, backs both `Vcs` and `RemoteReachable`), `network.rs` (`TcpStream`), `clock.rs` (`date`), `store.rs` (`std::fs`), `prompt.rs`/`reporter.rs` (stdio), `remote_queue.rs` (`ureq`). `prod.rs::ProdPlatform::new(&Config)` wires them; `main.rs` builds it once and dispatches.
- `src/testing.rs` (`#[cfg(test)]`) holds the fakes and `TestPlatform`.
- Stores are **domain repositories** returning raw `StoredDoc { path, content }` — they never parse front-matter; parsing/projection stays in the commands/`invoice` helpers.
- The error enum is decoupled from `ureq`: `Error::Http(String)`; adapters map crate errors to it.

**Pipeline (the core flow):** `commands/insert.rs` → `invoice/parser.rs` → `invoice/mapper.rs` → `commands/sync.rs`.

1. `parser::parse(url, http)` fetches the invoice HTML via the `Http` port, scrapes fields via CSS selectors (`scraper`), extracts a JWT-style token embedded in inline JS, then POSTs to `https://suf.purs.gov.rs/specifications` to get the line-item JSON. It **retries up to 3×** but only for token/item-fetch failures (string-matching in `parse_with_retries`) — other errors fail fast.
2. `mapper::write_invoice` renders one `.md` per `InvoiceItem` via the `TransactionStore` port. Filenames are `{compactDate}-{slug}.md` with `-01`, `-02` suffixes on collision (pure `unique_name`).
3. `insert` then calls `sync::run` automatically unless `--no-sync`.

**Three core types** (`invoice/mod.rs`): `Invoice` (whole receipt) → `InvoiceItem` (one API line item) → `Transaction` (the persisted YAML shape; used for reading existing files back). These are deliberately separate — don't merge them.

**Commands** (`commands/*.rs`, dispatched from `main.rs` via the `clap` enum in `cli.rs`):
- `add` — interactive entry **driven entirely by `schemas/schema.yaml`**: it walks the schema's `properties`/`required` and prompts per field. Adding a field to the schema changes both `add` prompting and `validate` — no Rust change needed.
- `insert` — parse + save a URL (or a file of URLs via `-f`); queues the URL instead if offline.
- `queue` — local-file queue (`add`/`list`/`remove`/`process`); `process --remote` pulls from / deletes against the HTTP API in config.
- `validate` — checks front-matter against `schema.yaml` using the `jsonschema` crate; YAML is converted to `serde_json::Value` via serde without an intermediate string.
- `report monthly`, `search name`/`search duplicates` — read-only scans over the `TransactionStore`.
- `sync` — `git add/commit/push` against `data_dir`; auto-detects connectivity (`Network` + `RemoteReachable` ports) and commits-without-push when offline. Shells out to the `git` and `date` binaries rather than linking a library.

**Config** (`config.rs`): `load<E: Env>(override, env)` resolves the path (`$XDG_CONFIG_HOME/localbills/config.yaml`, overridable with `--config`), reads the file, then delegates to the pure `parse(file_text, env)` — which is directly unit-tested. **Environment variables always win over file values** (`TRANSACTION_DIR`, `DATA_DIR`, `QUEUE_FILE`, `FAILED_LINKS`, `API_*`, `SCHEMA_FILE`). `data_dir` defaults to `transaction_dir`.

**Errors** (`error.rs`): one crate-wide `Error` enum + `Result<T>` alias. Use the `Error::Parse`/`Error::Config`/`Error::Git`/`Error::Http` string variants for domain errors; `?` converts `io`/`serde_*` errors automatically via the `From` impls. HTTP errors are stringified at the adapter boundary, so the core doesn't depend on `ureq`.

## Conventions specific to this repo

- **No async, no chrono.** HTTP is synchronous `ureq`; timestamps come from shelling out to `date`; dates are handled as ISO-8601 strings, never parsed into a datetime type. Don't introduce `tokio`/`chrono` to "fix" this — it's intentional to keep the binary small (note the `default-features = false` on `jsonschema` and `regex`).
- **All scraped text passes through `sanitize::cyrillic_to_latin`** (Serbian Cyrillic → Latin). Apply it to any new field read from the invoice page or API.
- The retry-vs-fail-fast decision in `parser.rs` is driven by **substring matching on the error message** — if you rename or reword token/item errors, update `parse_with_retries` accordingly.
- `is_duplicate` (in `insert.rs`) detects re-inserts by a literal substring search for the URL across every `.md` doc from the `TransactionStore`.
- When adding a new external dependency, add a port in `ports/mod.rs`, a production adapter in `adapters/`, a fake in `testing.rs`, and a field/accessor on both `ProdPlatform` and `TestPlatform`.

## Commit style

```
type(scope): target change
brief description
```

- **type**: one of `feat`, `refactor`, `fix`, `chore`, `test`.
- **scope**: the feature/area changed, mostly mirroring the directory — e.g. a command name (`add`, `insert`, `sync`, `queue`), `parser`, `mapper`, `config`.
- **target change**: short imperative subject line.
- **brief description**: optional body line(s) for the "why" when the subject isn't self-explanatory.

Examples (from history): `feat(add): add schema form parser for add command`, `refactor: use EnvVar enum instead of free form strings`. Scope may be omitted when the change is repo-wide.

## Legacy / non-Rust assets

- `scripts/` is the **original Python/shell/Perl implementation** (`parser/rs_parser.py`, `mapper/`, `sanitize/sanitize_rs.pl`, etc.) that the Rust binary replaces, plus dated one-off data migrations under `scripts/migrate/` (e.g. SQLite→YAML, date→ISO). Reference only — not built or invoked by the CLI.
- `bin/` holds thin shell wrappers (`insert_link.sh`, `sync_data.sh`, …) and `setup/` has per-platform dependency installers (Arch, macOS, Termux).
