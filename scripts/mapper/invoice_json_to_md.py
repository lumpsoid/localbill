#!/usr/bin/env python3

import os
import json
import re
import sys
import argparse
from pathlib import Path


def slugify(text: str) -> str:
    """Make a safe filename."""
    text = text.strip().lower()
    text = re.sub(r"[^a-z0-9]+", "-", text)
    return text.strip("-")


def generate_item_markdown(invoice: dict, item: dict) -> str:
    """Convert a single item to one Markdown document."""

    notes = invoice.get("raw_bill_text", "").replace('"', '\\"')

    yaml = [
        "---",
        f"date: '{invoice['date'][:10]}'",
        f'retailer: "{invoice["retailer"]}"',
        f'name: "{item["name"]}"',
        f"quantity: {item['quantity']}",
        f"unit_price: {item['unit_price']}",
        f"price_total: {item['total']}",
        f"currency: {invoice['currency']}",
        f"country: {invoice['country'].lower()}",
        f'link: "{invoice["url"]}"',
        "tags: []",
        f'notes: "{notes}"',
        "---",
        "",
    ]

    return "\n".join(yaml)


def map_if_str(value, fn):
    return fn(value) if isinstance(value, str) else value


def load_invoice(args) -> dict:
    # Explicit --stdin
    if args.stdin:
        raw = sys.stdin.read().strip()
        if not raw:
            raise ValueError("Expected JSON on stdin but received none.")
        return json.loads(raw)

    # Explicit "-"
    if args.input == "-":
        raw = sys.stdin.read().strip()
        if not raw:
            raise ValueError("Expected JSON on stdin but received none.")
        return json.loads(raw)

    # File
    if args.input:
        with open(args.input, "r", encoding="utf-8") as f:
            return json.load(f)

    # Environment variable
    env_json = os.getenv("INVOICE_JSON")
    if env_json:
        return json.loads(env_json)

    # Fail fast
    raise ValueError("No input JSON provided.")


def main():
    parser = argparse.ArgumentParser(
        description="Convert invoice JSON into per-item Markdown files."
    )
    parser.add_argument("--output-dir", help="Output directory (omit to print to stdout)")
    parser.add_argument(
        "input", nargs="?", help="Input JSON file, or '-' to read from stdin"
    )
    parser.add_argument(
        "--stdin", action="store_true", help="Read JSON from stdin explicitly"
    )

    args = parser.parse_args()

    try:
        invoice = load_invoice(args)
    except Exception as e:
        print(f"Failed to load invoice JSON: {e}", file=sys.stderr)
        sys.exit(1)

    invoice_date = map_if_str(
        invoice.get("date"), lambda s: s.replace("-", "").replace(":", "")
    )

    write_files = args.output_dir is not None
    out_dir = None

    if write_files:
        out_dir = Path(args.output_dir)
        out_dir.mkdir(parents=True, exist_ok=True)

    for item in invoice["items"]:
        item_slug = slugify(item["name"])
        filename = f"{invoice_date}-{item_slug}.md"

        md_content = generate_item_markdown(invoice, item)

        if write_files:
            filepath = out_dir / filename
            with open(filepath, "w", encoding="utf-8") as f:
                f.write(md_content)
            print(f"Saved: {filepath}")
        else:
            # Print to standard output, separated cleanly
            print(f'# Filename: {filename}')
            print(md_content)

    if write_files:
        print("Done.")


if __name__ == "__main__":
    main()
