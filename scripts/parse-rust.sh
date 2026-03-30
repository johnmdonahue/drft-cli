#!/bin/sh
# Script parser for Rust doc comments (batch mode).
# Reads file paths from stdin (one per line), extracts markdown-style
# link targets from /// and //! comments, emits NDJSON with file field.
#
# Usage: printf "src/foo.rs\nsrc/bar.rs\n" | ./scripts/parse-rust.sh

while IFS= read -r filepath; do
    [ -z "$filepath" ] && continue
    grep -n '^\s*///\|^\s*//!' "$filepath" 2>/dev/null | while IFS= read -r line; do
        echo "$line" | grep -oE '\]\([^)]+\.md\)' | while IFS= read -r match; do
            target=$(echo "$match" | sed 's/^\](//;s/)$//')
            printf '{"file":"%s","target":"%s","type":"comment"}\n' "$filepath" "$target"
        done
    done
done
