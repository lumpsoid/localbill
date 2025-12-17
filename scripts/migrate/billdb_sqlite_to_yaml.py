#!/usr/bin/env python3
"""
Export billing database records to YAML-frontmatter Markdown files.

This script reads invoice and item data from a SQLite database and exports
each transaction as a separate Markdown file with YAML frontmatter.
"""

import sqlite3
from dataclasses import dataclass
from pathlib import Path
from typing import List
import os

import yaml
import re


def clean_notes_for_yaml(notes: str) -> str:
    """
    Clean notes for YAML:
    - Convert CRLF/CR to LF
    - Remove control characters except tab/newline
    - Strip trailing whitespace per line
    """
    if not isinstance(notes, str):
        notes = str(notes)

    notes = notes.replace("\r\n", "\n").replace("\r", "\n")
    notes = re.sub(r"[^\x09\x0A\x0D\x20-\x7E]", "", notes)
    notes = "\n".join(line.rstrip() for line in notes.splitlines())
    return notes


# Custom representer to always dump multi-line strings as block literal
def str_presenter(dumper, data):
    if "\n" in data:
        return dumper.represent_scalar("tag:yaml.org,2002:str", data, style="|")
    return dumper.represent_scalar("tag:yaml.org,2002:str", data)


yaml.add_representer(str, str_presenter)


@dataclass
class TransactionRecord:
    """Represents a single transaction to be exported."""

    date: str
    name: str
    retailer: str
    quantity: int
    price_each: float
    price_total: float
    currency: str
    country: str
    link: str
    tags: List[str]
    notes: str

    def to_dict(self):
        """Convert to dictionary for YAML serialization."""
        return {
            "date": self.date,
            "name": self.name,
            "retailer": self.retailer,
            "quantity": self.quantity,
            "unit_price": self.price_each,
            "price_total": self.price_total,
            "currency": self.currency.upper(),
            "country": self.country,
            "link": self.link,
            "tags": self.tags,
            "notes": clean_notes_for_yaml(self.notes),
        }


class DatabaseConnection:
    """Handle database queries."""

    def __init__(self, db_path: Path):
        self.conn = sqlite3.connect(db_path)
        self.conn.row_factory = sqlite3.Row

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        self.conn.close()

    def fetch_all_invoices(self) -> List[sqlite3.Row]:
        """Fetch all invoices from the database."""
        cursor = self.conn.cursor()
        cursor.execute("SELECT * FROM invoice")
        return cursor.fetchall()

    def fetch_items_for_invoice(self, invoice_id: int) -> List[sqlite3.Row]:
        """Fetch all items for a given invoice."""
        cursor = self.conn.cursor()
        cursor.execute("SELECT * FROM item WHERE invoice_id = ?", [invoice_id])
        return cursor.fetchall()

    def fetch_item_tags(self, item_id: int) -> List[str]:
        """Fetch all tags associated with an item."""
        cursor = self.conn.cursor()
        cursor.execute(
            """
            SELECT tag.tag_name 
            FROM item_tag
            JOIN tag ON item_tag.tag_id = tag.tag_id
            WHERE item_tag.item_id = ?
            """,
            [item_id],
        )
        return [row["tag_name"] for row in cursor.fetchall()]

    def fetch_invoice_tags(self, invoice_id: int) -> List[str]:
        """Fetch all tags associated with an invoice."""
        cursor = self.conn.cursor()
        cursor.execute(
            """
            SELECT tag.tag_name 
            FROM invoice_tag
            JOIN tag ON invoice_tag.tag_id = tag.tag_id
            WHERE invoice_tag.invoice_id = ?
            """,
            [invoice_id],
        )
        return [row["tag_name"] for row in cursor.fetchall()]


class TransactionExporter:
    """Handle exporting transactions to Markdown files."""

    def __init__(self, output_dir: Path):
        self.output_dir = output_dir
        self.output_dir.mkdir(exist_ok=True)

    def export_transaction(self, record: TransactionRecord, filename: str):
        """Write a transaction record to a Markdown file with YAML frontmatter."""
        filepath = self.output_dir / filename

        with open(filepath, "w", encoding="utf-8") as f:
            f.write("---\n")
            yaml.dump(
                record.to_dict(),
                f,
                sort_keys=False,
                allow_unicode=True,
                default_flow_style=False,
            )
            f.write("---\n")


def create_item_transaction(
    invoice: sqlite3.Row, item: sqlite3.Row, tags: List[str]
) -> TransactionRecord:
    """Create a transaction record from an invoice and item."""
    item_name: str = item["item_name"] or ""

    return TransactionRecord(
        date=invoice["invoice_date"],
        retailer=invoice["invoice_name"],
        name=item_name.strip().replace(r"\s+", " "),
        quantity=item["item_quantity"] or 1,
        price_each=item["item_price_one"] or item["item_price"],
        price_total=item["item_price"],
        currency=invoice["invoice_currency"],
        country=invoice["invoice_country"],
        link=invoice["invoice_link"],
        tags=tags,
        notes=invoice["invoice_text"] or "",
    )


def create_invoice_transaction(
    invoice: sqlite3.Row, tags: List[str]
) -> TransactionRecord:
    """Create a transaction record from an invoice without items."""
    return TransactionRecord(
        date=invoice["invoice_date"],
        retailer=invoice["invoice_name"],
        name=invoice["invoice_name"],
        quantity=1,
        price_each=invoice["invoice_price"],
        price_total=invoice["invoice_price"],
        currency=invoice["invoice_currency"],
        country=invoice["invoice_country"],
        link=invoice["invoice_link"],
        tags=tags,
        notes=invoice["invoice_text"] or "",
    )


def process_invoices(db: DatabaseConnection, exporter: TransactionExporter):
    """Process all invoices and export transactions."""
    invoices = db.fetch_all_invoices()

    for invoice in invoices:
        invoice_id = invoice["invoice_id"]
        items = db.fetch_items_for_invoice(invoice_id)

        try:
            if items:
                # Export each item as a separate transaction
                for item in items:
                    item_id = item["item_id"]
                    tags = db.fetch_item_tags(item_id)
                    record = create_item_transaction(invoice, item, tags)
                    filename = f"{invoice['invoice_date']}-{invoice_id}-{item_id}.md"
                    exporter.export_transaction(record, filename)
            else:
                # Export invoice as a single transaction
                tags = db.fetch_invoice_tags(invoice_id)
                record = create_invoice_transaction(invoice, tags)
                filename = f"{invoice['invoice_date']}-{invoice_id}.md"
                exporter.export_transaction(record, filename)
        except Exception as e:
            print(f"Error occured on {filename}: {e}")
            continue


def main():
    """Main entry point."""
    db_path = os.getenv("DB_PATH")
    if db_path is None or len(db_path) == 0:
        print("No DB_PATH provided")
        exit(1)

    if not os.path.exists(db_path):
        print("DB_PATH doesn't exist")
        exit(1)

    output_dir = os.getenv("OUTPUT_DIR")
    if output_dir is None or len(output_dir) == 0:
        print("No OUTPUT_DIR provided")
        exit(1)

    output_dir_path = Path(output_dir)
    if not os.path.exists(output_dir_path):
        print(f"Creating OUTPUT_DIR={output_dir}")
        os.mkdir(output_dir_path)

    with DatabaseConnection(db_path) as db:
        exporter = TransactionExporter(output_dir_path)
        process_invoices(db, exporter)

    print("\n✓ Export complete!")


if __name__ == "__main__":
    main()
