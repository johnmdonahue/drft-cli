#!/bin/sh
# Script parser for Rust doc comments.
# Extracts backtick-wrapped .md file paths from /// and //! comments.
# Paths in comments are repo-relative (e.g., docs/foo.md), but drft
# resolves targets relative to the source file. We compute the
# relative path from source dir to repo root and prepend it.
#
# Usage: echo "src/foo.rs" | ./scripts/parse-rust-docs.sh

read -r filepath

# Compute depth: src/analyses/degree.rs → 2 levels deep
dir=$(dirname "$filepath")
prefix=""
remaining="$dir"
while [ "$remaining" != "." ] && [ -n "$remaining" ]; do
    prefix="../${prefix}"
    remaining=$(dirname "$remaining")
done

grep -n '^\s*///\|^\s*//!' "$filepath" 2>/dev/null | while IFS= read -r line; do
    echo "$line" | grep -oE '`[^`]+\.md`' | while IFS= read -r match; do
        target=$(echo "$match" | sed 's/^`//;s/`$//')
        printf '{"target":"%s%s","type":"comment"}\n' "$prefix" "$target"
    done
done
