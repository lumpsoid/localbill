#!/usr/bin/env python3

import os
import re
import argparse


def fix_notes_format(directory_path, dry_run=False):
    """
    Finds notes specifically starting with "=+ ФИСКАЛНИ РАЧУН" in quotes
    and converts them to YAML block scalar format (|).
    """

    # Explanation:
    # notes:\s*"     -> Matches 'notes:' followed by opening quote
    # [=+\s]+        -> Matches any sequence of '=' or '+' or spaces (the header)
    # ФИСКАЛНИ РАЧУН -> The required anchor text
    # .*?            -> Everything else (non-greedy)
    # "              -> The closing quote
    notes_pattern = re.compile(r'notes:\s*"([=+\s]*ФИСКАЛНИ РАЧУН.*?)"', re.DOTALL)

    if not os.path.isdir(directory_path):
        print(f"Error: The directory '{directory_path}' does not exist.")
        return

    updated_count = 0

    for filename in os.listdir(directory_path):
        if filename.endswith(".md"):
            file_path = os.path.join(directory_path, filename)

            with open(file_path, "r", encoding="utf-8") as f:
                content = f.read()

            def replacer(match):
                # match.group(1) is the content inside the quotes
                raw_text = match.group(1)

                # Clean up escaped quotes and escaped newlines
                clean_text = raw_text.replace('\\"', '"').replace("\\n", "\n")

                # Split and indent
                lines = clean_text.splitlines()
                indented_text = "\n".join(f"    {line.rstrip()}" for line in lines)

                return f"notes: |\n{indented_text}"

            # Only replaces if the pattern (starting with the header) is found
            new_content = notes_pattern.sub(replacer, content)

            if new_content != content:
                if not dry_run:
                    with open(file_path, "w", encoding="utf-8") as f:
                        f.write(new_content)
                    print(f"Updated: {filename}")
                else:
                    print(f"[DRY RUN] Would update: {filename}")
                updated_count += 1

    print(f"\nFinished. Files modified: {updated_count}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(
        description="Convert Fiscal Receipt notes to YAML block scalars if they start with the header."
    )
    parser.add_argument(
        "directory", type=str, help="Path to the directory containing Markdown files."
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Show which files would be updated without changing them.",
    )

    args = parser.parse_args()
    fix_notes_format(args.directory, dry_run=args.dry_run)
