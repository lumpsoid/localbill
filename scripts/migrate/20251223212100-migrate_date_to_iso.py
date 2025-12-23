#!/usr/bin/env python3

import os
import re
import argparse
from datetime import datetime


def replace_date(match, new_timestamp):
    """
    Uses named groups to safely construct the replacement string.
    'prefix' includes the key and the opening quote.
    'quote' is captured to ensure we close with the same character.
    """
    prefix = match.group("prefix")
    quote = match.group("quote")
    return f"{prefix}{new_timestamp}{quote}"


def update_md_dates(directory, dry_run=False):
    # Regex to find the YAML date line: date: 'YYYY-MM-DD'
    # REGEX EXPLANATION:
    # ^(?P<prefix>date:\s*(?P<quote>['"])) -> Group 'prefix' (the key and the start quote)
    # \d{4}-\d{2}-\d{2}                   -> The old date (not captured, just matched)
    # (?P=quote)$                         -> Ensures the closing quote matches the opening one
    # Note: We wrap the backreference in () to make it a reachable group for the function
    yaml_date_pattern = re.compile(
        r"^(?P<prefix>date:\s*(?P<quote>['\"]))\d{4}-\d{2}-\d{2}(?P=quote)$",
        re.MULTILINE,
    )

    # Regex to extract date/time from filename: 20251223T154011
    filename_pattern = re.compile(r"^(\d{8}T\d{6})")

    if not os.path.exists(directory):
        print(f"Error: Directory '{directory}' does not exist.")
        return

    for filename in os.listdir(directory):
        if not filename.endswith(".md"):
            continue

        file_path = os.path.join(directory, filename)

        # 1. Extract date/time from filename
        match = filename_pattern.match(filename)
        if not match:
            print(f"Skipping {filename}: No valid timestamp found in filename.")
            continue

        raw_ts = match.group(1)
        try:
            # Parse 20251223T154011 and format to YYYY-MM-DDTHH:mm:ss
            dt_obj = datetime.strptime(raw_ts, "%Y%m%dT%H%M%S")
            new_date_str = dt_obj.strftime("%Y-%m-%dT%H:%M:%S")
        except ValueError:
            print(f"Skipping {filename}: Timestamp format error.")
            continue

        # 2. Read file content
        with open(file_path, "r", encoding="utf-8") as f:
            content = f.read()

        # 3. Check if YAML date exists and replace it
        if yaml_date_pattern.search(content):
            # The lambda keeps the existing quotes (single or double) from the file
            new_content = yaml_date_pattern.sub(
                lambda m: replace_date(m, new_date_str), content
            )

            if content == new_content:
                print(f"No change needed for {filename}.")
                continue

            if dry_run:
                print(f"[DRY RUN] Would update {filename} to date: '{new_date_str}'")
            else:
                with open(file_path, "w", encoding="utf-8") as f:
                    f.write(new_content)
                print(f"Updated {filename}")
        else:
            print(f"Skipping {filename}: No matching YAML date line found.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(
        description="Update YAML dates in MD files from filenames."
    )
    parser.add_argument("directory", help="Path to the directory containing .md files")
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Show what would happen without modifying files",
    )

    args = parser.parse_args()
    update_md_dates(args.directory, args.dry_run)
