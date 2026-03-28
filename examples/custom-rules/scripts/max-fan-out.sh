#!/bin/sh
# Flag any node with more than 5 outbound links.
# Receives JGF graph JSON on stdin, emits NDJSON diagnostics on stdout.

MAX=5

python3 -c "
import json, sys
data = json.load(sys.stdin)
graph = data['graph']
counts = {}
for e in graph['edges']:
    counts[e['source']] = counts.get(e['source'], 0) + 1
for node, count in sorted(counts.items()):
    if count > $MAX:
        print(json.dumps({
            'message': f'too many outbound links ({count})',
            'node': node,
            'fix': f'{node} has {count} outbound links (max {$MAX}) — consider splitting into smaller files'
        }))
"
