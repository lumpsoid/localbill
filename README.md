# localbill

A CLI tool for parsing, storing, and managing Serbian fiscal invoices (receipts) locally. Each invoice item is saved as a Markdown file with YAML front-matter in a Git-backed data directory, giving you a plain-text, version-controlled personal finance log.

## Features

- **Parse** Serbian fiscal invoice URLs and extract all line items automatically
- **Store** each item as a structured `.md` file with YAML front-matter
- **Validate** transaction files against a JSON Schema
- **Queue** URLs for offline processing and sync later
- **Report** monthly spending summaries
- **Search** transactions by product name or find duplicates
- **Sync** the data directory with a Git remote

## Requirements

- [Rust](https://rustup.rs/) 1.70 or later (for building)
- Git (for data sync)

## Installation

### Build from source

```bash
git clone https://github.com/lumpsoid/localbill.git
cd localbill
cargo build --release
```

The binary will be at `target/release/localbill`. Copy it somewhere on your `$PATH`:

```bash
cp target/release/localbill ~/.local/bin/
```

### Optional: platform setup scripts

Scripts for installing system dependencies (Python, Git, jq) are provided for common platforms:

```bash
bash setup/install.sh
```

Supported platforms: Arch Linux, macOS, Termux (Android).

## Configuration

localbill reads its config from `$XDG_CONFIG_HOME/localbills/config` (defaults to `~/.config/localbills/config`). The file uses a simple `KEY=value` format. Environment variables override file values.

Create the config file:

```bash
mkdir -p ~/.config/localbills
```

**Example `~/.config/localbills/config`:**

```bash
# Directory where transaction .md files are stored (must be a git repo for sync)
TRANSACTION_DIR="/home/user/invoices"

# Git-backed data repo (can be the same as TRANSACTION_DIR)
DATA_DIR="/home/user/invoices"

# Local queue file for offline URL storage
QUEUE_FILE="/home/user/.local/share/localbills/queue.txt"

# File for URLs that failed to parse
FAILED_LINKS="/home/user/.local/share/localbills/failed.txt"

# Remote API for fetching queued URLs (optional)
API_HOST="192.168.1.2"
API_PORT="8087"
API_ENDPOINT="/queue"

# Path to the JSON Schema file used by `validate` (optional)
SCHEMA_FILE="/path/to/localbill/schemas/schema.yaml"
```

### Configuration reference

| Key | Default | Description |
|-----|---------|-------------|
| `TRANSACTION_DIR` | `~/localbills-data` | Directory where transaction files are saved |
| `DATA_DIR` | same as `TRANSACTION_DIR` | Git repo root for sync operations |
| `QUEUE_FILE` | `~/.local/share/localbills/queue.txt` | Local queue of pending invoice URLs |
| `FAILED_LINKS` | `~/.local/share/localbills/failed.txt` | URLs that could not be parsed |
| `API_HOST` | `192.168.1.2` | Remote API host for fetching queued URLs |
| `API_PORT` | `8087` | Remote API port |
| `API_ENDPOINT` | `/queue` | Remote API endpoint path |
| `SCHEMA_FILE` | _(none)_ | JSON/YAML schema for `validate` |

You can also pass `--config <PATH>` to any command to override the config file location.

### Setting up the data directory

The data directory must be initialised as a Git repository if you want to use `sync`:

```bash
mkdir -p ~/invoices
cd ~/invoices
git init
git remote add origin <your-remote-url>
```

## Usage

```
localbill [--config <PATH>] <COMMAND>
```

### `insert` — parse and save an invoice

Fetches the invoice page, parses every line item, and writes one `.md` file per item into `TRANSACTION_DIR`.

```bash
localbill insert "https://suf.purs.gov.rs/v/?vl=..."
```

Options:

| Flag | Description |
|------|-------------|
| `--dry-run` | Print parsed output to stdout; do not write files |
| `--no-sync` | Skip the automatic Git sync after writing |
| `--force` | Insert even if the URL has already been recorded |

---

### `queue` — manage the offline queue

Use the queue when you cannot process invoices immediately (e.g. no internet connection).

```bash
# Add a URL to the local queue
localbill queue add "https://suf.purs.gov.rs/v/?vl=..."

# List all queued URLs
localbill queue list

# Process every queued URL (reads the local queue file)
localbill queue process

# Process URLs fetched from the remote API instead
localbill queue process --remote

# Remove a specific URL from the queue
localbill queue remove "https://suf.purs.gov.rs/v/?vl=..."
```

`queue process` options:

| Flag | Description |
|------|-------------|
| `--remote` | Fetch the queue from the remote API instead of the local file |
| `--no-sync` | Skip Git sync after processing each invoice |

---

### `validate` — check transaction files

Validates one file, all files in a directory, or the entire `TRANSACTION_DIR` against `SCHEMA_FILE`.

```bash
# Validate all files in TRANSACTION_DIR
localbill validate

# Validate a specific file or directory
localbill validate /path/to/file.md
localbill validate /path/to/invoices/

# Continue after the first error
localbill validate --continue-on-error

# Print only files that have errors
localbill validate --errors-only
```

---

### `report` — spending summaries

```bash
# Show spending for every month
localbill report monthly

# Show spending for a specific year
localbill report monthly --year 2024

# Show spending for a specific month
localbill report monthly --year 2024 --month 3
```

---

### `search` — find transactions

```bash
# Case-insensitive substring search by product name
localbill search name "bread"
localbill search name "mleko"

# Find invoice URLs that appear in more than one transaction file
localbill search duplicates
```

---

### `sync` — commit and push the data directory

Stages all changes in `DATA_DIR`, creates a commit, and pushes to the Git remote.

```bash
# Sync with an auto-generated commit message
localbill sync

# Append a custom note to the commit message
localbill sync --message "monthly reconciliation"

# Commit locally without pushing
localbill sync --no-push
```

---

## Transaction file format

Each saved transaction is a `.md` file with YAML front-matter:

```markdown
---
date: "2024-03-15T14:30:00"
name: "Mleko 1L"
retailer: "Maxi"
quantity: 2.0
unit_price: 89.99
price_total: 179.98
currency: RSD
country: Serbia
link: "https://suf.purs.gov.rs/v/?vl=..."
tags:
  - dairy
notes: |
  Optional free-text note.
---
```

### Required fields

| Field | Type | Description |
|-------|------|-------------|
| `date` | ISO 8601 datetime | Purchase date and time |
| `name` | string | Product name |
| `retailer` | string | Store or merchant name |
| `quantity` | number ≥ 0 | Number of units purchased |
| `unit_price` | number ≥ 0 | Price per unit |
| `price_total` | number ≥ 0 | Total line-item price |
| `currency` | 3-letter code (e.g. `RSD`) | Currency |
| `country` | string | Country of purchase |

### Optional fields

| Field | Type | Description |
|-------|------|-------------|
| `link` | URI | Source invoice URL |
| `tags` | array of strings | Custom labels |
| `notes` | string | Free-text notes |
| `exchange` | array | Currency exchange details |
| `fees` | array | Additional fees |
| `discounts` | array | Applied discounts |

The full schema is in [`schemas/schema.yaml`](schemas/schema.yaml).

---

## Examples

**Typical daily workflow:**

```bash
# Scan a receipt QR code and insert it immediately
localbill insert "https://suf.purs.gov.rs/v/?vl=..."

# No internet? Queue it for later
localbill queue add "https://suf.purs.gov.rs/v/?vl=..."

# Back online: process the queue
localbill queue process

# Check everything is valid
localbill validate --errors-only

# See how much you spent this month
localbill report monthly --year 2024 --month 3

# Push the data to your remote backup
localbill sync
```

**Auditing your data:**

```bash
# Find all purchases of a product
localbill search name "jogurt"

# Check for accidentally duplicated invoices
localbill search duplicates

# Validate a single file you just edited
localbill validate ~/invoices/2024-03-15-maxi-mleko.md
```

## License

MIT — see [LICENSE](LICENSE).
