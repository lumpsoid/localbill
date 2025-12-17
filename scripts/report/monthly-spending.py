#!/usr/bin/env python3

import sys
import glob

def calculate_total(path_glob):
    total = 0.0

    for file in glob.glob(path_glob):
        try:
            with open(file, 'r', encoding='utf-8', errors='ignore') as f:
                for line in f:
                    line = line.strip()
                    if line.startswith("price_total:"):
                        value_str = line.split("price_total:", 1)[1].strip()
                        try:
                            total += float(value_str)
                        except ValueError:
                            pass
                        break
        except Exception:
            pass

    return total


def main():
    if len(sys.argv) != 2:
        print("Usage: monthly-spending.py '<path_glob>'")
        sys.exit(1)

    path_glob = sys.argv[1]
    total = calculate_total(path_glob)
    print(total)


if __name__ == '__main__':
    main()
