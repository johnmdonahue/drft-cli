#!/bin/sh
# Require specific frontmatter fields in all document nodes.
# Receives JGF graph JSON on stdin, reads files from disk to check frontmatter.
#
# Checks for: title, sources
# Customize REQUIRED_FIELDS below.

REQUIRED_FIELDS="title sources"

python3 -c "
import json, sys, os, re

required = '$REQUIRED_FIELDS'.split()
data = json.load(sys.stdin)
graph = data['graph']

for path, node in sorted(graph['nodes'].items()):
    if node['metadata']['type'] != 'document':
        continue
    if not os.path.isfile(path):
        continue

    content = open(path).read()

    # Extract frontmatter
    m = re.match(r'^---\n(.*?)\n---', content, re.DOTALL)
    if not m:
        fields = ', '.join(required)
        print(json.dumps({
            'message': 'missing frontmatter',
            'node': path,
            'fix': f'{path} has no YAML frontmatter — add a --- block with required fields: {fields}'
        }))
        continue

    fm = m.group(1)
    missing = [f for f in required if not re.search(rf'^{f}\s*:', fm, re.MULTILINE)]
    if missing:
        names = ', '.join(missing)
        print(json.dumps({
            'message': f'missing frontmatter fields: {names}',
            'node': path,
            'fix': f'{path} is missing required frontmatter fields: {names}'
        }))
"
