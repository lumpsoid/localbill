#!/usr/bin/env python3

import argparse
import sys
from pathlib import Path

import yaml
from jsonschema import Draft202012Validator, FormatChecker


def load_yaml(path: Path):
    with path.open("r", encoding="utf-8") as f:
        return yaml.safe_load(f)


def load_yaml_front_matter(path: Path):
    """
    Extract YAML front matter from a Markdown file.
    Only the first '---' block is parsed.
    """
    text = path.read_text(encoding="utf-8")
    if not text.startswith("---"):
        raise ValueError(f"No YAML front matter found in {path}")

    # Split on `---` and take the first block after the first `---`
    parts = text.split("---", 2)
    if len(parts) < 2:
        raise ValueError(f"Malformed YAML front matter in {path}")

    yaml_content = parts[1]
    return yaml.safe_load(yaml_content)


def validate(data: dict, schema: dict, source: Path) -> list:
    """Validate data against schema and return list of errors (if any)."""
    validator = Draft202012Validator(schema, format_checker=FormatChecker())
    errors = sorted(validator.iter_errors(data), key=lambda e: e.path)

    error_messages = []
    for error in errors:
        location = ".".join(map(str, error.path)) or "root"
        error_messages.append(f"{source} → {location}: {error.message}")
    return error_messages


def iter_yaml_files(target: Path):
    if target.is_file():
        yield target
    elif target.is_dir():
        for path in sorted(target.iterdir()):
            if path.suffix.lower() in {".yaml", ".yml", ".md"} and path.is_file():
                yield path
    else:
        raise SystemExit(f"TARGET does not exist: {target}")


def main() -> int:
    parser = argparse.ArgumentParser(description="Transaction processor")

    # Positional parameter: dir
    # No prefix means it is required by default
    parser.add_argument("dir", type=Path, help="The transaction directory")

    # Named/Optional-style flag: --schema
    # Setting required=True makes the flag mandatory despite the '--' prefix
    parser.add_argument("--schema", required=True, help="The schema definition")

    # Boolean Flags
    parser.add_argument(
        "-c",
        "--continue",
        dest="continue_on_error",
        action="store_true",
        help="Continue processing even if an error occurs",
    )

    parser.add_argument(
        "-e",
        "--errors-only",
        dest="print_error_files_only",
        action="store_true",
        help="Only print filenames that contain errors",
    )

    args = parser.parse_args()

    target = Path(args.dir)
    schema = load_yaml(Path(args.schema))

    yaml_files = list(iter_yaml_files(target))
    if not yaml_files:
        print(f"No YAML files found in {target}", file=sys.stderr)
        return 1

    continue_on_error = args.continue_on_error
    print_error_files_only = args.print_error_files_only

    has_errors = False

    for yaml_file in yaml_files:
        try:
            data = load_yaml_front_matter(yaml_file)
            errors = validate(data, schema, yaml_file)
            if errors:
                has_errors = True
                if print_error_files_only:
                    print(yaml_file)
                else:
                    for msg in errors:
                        print(msg, file=sys.stderr)
                if not continue_on_error:
                    return 1
            else:
                if not print_error_files_only:
                    print(f"{yaml_file} is valid")
        except (yaml.YAMLError, ValueError) as exc:
            has_errors = True
            if print_error_files_only:
                print(yaml_file)
            else:
                print(f"{yaml_file}: invalid YAML ({exc})", file=sys.stderr)
            if not continue_on_error:
                return 1

    return 1 if has_errors else 0


if __name__ == "__main__":
    sys.exit(main())
