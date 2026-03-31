#!/bin/sh
# Example script parser: extract [[wikilinks]] from files.
#
# Usage in drft.toml:
#
#   [parsers.wikilinks]
#   files = ["*.md"]
#   command = "./examples/custom-parsers/wikilinks.sh"
#
# Protocol:
#   stdin line 1 = JSON options envelope (ignored here)
#   stdin lines 2+ = file paths (one per line)
#   stdout = NDJSON: {"file": "...", "target": "...", "type": "wikilink"}
#
# This is a minimal example. It does not strip code blocks, so
# [[patterns]] inside fenced code will produce false edges.
# For production use, consider handling code blocks or writing
# the parser in a language with a markdown AST library.

# Read and discard the options envelope
IFS= read -r _options

while IFS= read -r filepath; do
    [ -z "$filepath" ] && continue
    # Match [[target]] or [[target|display]] — extract the target portion
    grep -oE '\[\[[^]|]+(\|[^]]+)?\]\]' "$filepath" 2>/dev/null | while IFS= read -r match; do
        # Strip [[ and ]], take part before | if present
        target=$(printf '%s' "$match" | sed 's/^\[\[//;s/\]\]$//;s/|.*//')
        [ -z "$target" ] && continue
        # Append .md if no extension
        case "$target" in
            *.md) ;;
            *) target="${target}.md" ;;
        esac
        printf '{"file":"%s","target":"%s","type":"wikilink"}\n' "$filepath" "$target"
    done
done
