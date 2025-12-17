#!/usr/bin/env python3

from pathlib import Path
import os
import sys
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
    target_env = os.getenv("TARGET")
    if not target_env:
        print("TARGET environment variable is not set", file=sys.stderr)
        return 1
    target = Path(target_env)

    schema_env = os.getenv("SCHEMA")
    if not schema_env:
        print("SCHEMA environment variable is not set", file=sys.stderr)
        return 1
    schema = load_yaml(Path(schema_env))

    yaml_files = list(iter_yaml_files(target))
    if not yaml_files:
        print(f"No YAML files found in {target}", file=sys.stderr)
        return 1

    continue_on_error = os.getenv("CONTINUE_ON_ERROR", "0") == "1"
    print_error_files_only = os.getenv("PRINT_ERROR_FILES_ONLY", "0") == "1"

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
