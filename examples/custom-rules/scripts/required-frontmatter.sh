#!/bin/sh
# Require specific frontmatter fields in all File nodes.
# Receives JGF graph JSON on stdin, reads files from disk to check frontmatter.
#
# Customize REQUIRED_FIELDS and SKIP_NAMES below.

REQUIRED_FIELDS="title sources"
SKIP_NAMES="README.md CHANGELOG.md CONTRIBUTING.md CLAUDE.md"

python3 -c "
import json, sys, os, re

required = '$REQUIRED_FIELDS'.split()
skip_names = set('$SKIP_NAMES'.split())
data = json.load(sys.stdin)
graph = data['graph']

for path, node in sorted(graph['nodes'].items()):
    if node['metadata']['type'] != 'file':
        continue
    if not os.path.isfile(path):
        continue

    # Skip exempt filenames
    filename = os.path.basename(path)
    if filename in skip_names:
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
