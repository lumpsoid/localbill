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
    """Convert a single item to one Markdown document with a block scalar for notes."""

    # Get the notes and prepare for indentation
    raw_notes = invoice.get("raw_bill_text", "")

    # Indent every line of the notes by 4 spaces
    # This ensures it stays valid within the YAML block scalar
    indented_notes = "\n".join(f"    {line}" for line in raw_notes.splitlines())

    yaml = [
        "---",
        f'date: "{invoice["date"]}"',
        f'retailer: "{invoice["retailer"]}"',
        f'name: "{item["name"]}"',
        f"quantity: {item['quantity']}",
        f"unit_price: {item['unit_price']}",
        f"price_total: {item['total']}",
        f"currency: {invoice['currency']}",
        f"country: {invoice['country'].lower()}",
        f'link: "{invoice["url"]}"',
        "tags: []",
        "notes: |",
        indented_notes,
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


def num_pad(n: int, width: int = 2) -> str:
    return str(n).zfill(width)


def main():
    parser = argparse.ArgumentParser(
        description="Convert invoice JSON into per-item Markdown files."
    )
    parser.add_argument(
        "--output-dir", help="Output directory (omit to print to stdout)"
    )
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

    if not invoice["success"]:
        print(invoice, file=sys.stderr)
        sys.exit(1)

    if write_files:
        out_dir = Path(args.output_dir)
        out_dir.mkdir(parents=True, exist_ok=True)

    extension = "md"
    for item in invoice["items"]:
        item_slug = slugify(item["name"])
        base_filename = f"{invoice_date}-{item_slug}"

        md_content = generate_item_markdown(invoice, item)

        if write_files:
            counter = 0
            while True:
                if counter == 0:
                    filename = base_filename
                else:
                    filename = f"{base_filename}-{num_pad(counter)}"

                basename = f"{filename}.{extension}"
                filepath = out_dir / basename

                if not filepath.exists():
                    break

                counter += 1

            with open(filepath, "w", encoding="utf-8") as f:
                f.write(md_content)
            print(f"Saved: {filepath}")
        else:
            print(f"# Base filename: {base_filename}")
            print(md_content)

    if write_files:
        print("Done.")


if __name__ == "__main__":
    main()
