#!/bin/sh
# Enforce kebab-case file naming for all document nodes.
# Receives JGF graph JSON on stdin, emits NDJSON diagnostics on stdout.

python3 -c "
import json, sys, re
data = json.load(sys.stdin)
graph = data['graph']
kebab = re.compile(r'^[a-z0-9]+(-[a-z0-9]+)*\.md$')
for path, node in sorted(graph['nodes'].items()):
    if node['metadata']['type'] != 'document':
        continue
    # Check the filename only (not directory parts)
    filename = path.rsplit('/', 1)[-1]
    if not kebab.match(filename):
        print(json.dumps({
            'message': f'filename does not match kebab-case convention',
            'node': path,
            'fix': f'rename {path} to use kebab-case (lowercase, hyphens, no spaces or underscores)'
        }))
"
