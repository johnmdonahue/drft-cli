#!/bin/sh
# Script parser for Rust doc comments.
# Extracts markdown-style link targets from /// and //! comments.
# Links use standard relative paths (e.g., ../../docs/foo.md).
#
# Usage: echo "src/foo.rs" | ./scripts/parse-rust-docs.sh

read -r filepath

grep -n '^\s*///\|^\s*//!' "$filepath" 2>/dev/null | while IFS= read -r line; do
    # Extract markdown link targets: [text](path.md)
    echo "$line" | grep -oE '\]\([^)]+\.md\)' | while IFS= read -r match; do
        # Strip ]( and )
        target=$(echo "$match" | sed 's/^\](//;s/)$//')
        printf '{"target":"%s","type":"comment"}\n' "$target"
    done
done
