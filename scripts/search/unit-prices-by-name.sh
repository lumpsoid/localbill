#!/bin/bash

# Check if pattern is provided
if [ -z "$1" ]; then
    echo "Error: No pattern provided" >&2
    echo "Usage: $0 PATTERN" >&2
    exit 1
fi

PATTERN="$1"

rg --ignore-case --files-with-matches "^name: .*${PATTERN}" | xargs awk '
/^date:/ { date = $2 }
/^unit_price:/ { price = $2 }
/^name:/ { sub(/^name: /, ""); name = $0 }
ENDFILE { 
    print date, price, name, FILENAME
    date = ""
    price = ""
    name = ""
}
' | sort -r
