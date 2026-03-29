#!/bin/sh
# Flag files that are too many hops from a root node (no inbound links).
# "Spelunking" — content buried too deep is hard to discover.
#
# Customize MAX_DEPTH below.

MAX_DEPTH=4

python3 -c "
import json, sys
from collections import deque

data = json.load(sys.stdin)
graph = data['graph']

# Build adjacency: source -> [targets]
children = {}
has_inbound = set()
for e in graph['edges']:
    children.setdefault(e['source'], []).append(e['target'])
    has_inbound.add(e['target'])

# Find roots (no inbound links, document type only)
roots = [
    p for p, n in graph['nodes'].items()
    if p not in has_inbound and n['metadata']['type'] == 'document'
]

# BFS from all roots to compute min depth for each node
depth = {}
queue = deque()
for r in roots:
    depth[r] = 0
    queue.append(r)

while queue:
    node = queue.popleft()
    for child in children.get(node, []):
        if child not in depth:
            depth[child] = depth[node] + 1
            queue.append(child)

# Flag nodes exceeding max depth
max_depth = $MAX_DEPTH
for path in sorted(graph['nodes']):
    node = graph['nodes'][path]
    if node['metadata']['type'] != 'document':
        continue
    d = depth.get(path)
    if d is not None and d > max_depth:
        print(json.dumps({
            'message': f'depth {d} exceeds max {max_depth}',
            'node': path,
            'fix': f'{path} is {d} links deep from the nearest root — consider linking to it from a higher-level document'
        }))
    elif d is None and path not in roots:
        # Unreachable from any root (island node in a cycle, etc)
        print(json.dumps({
            'message': 'unreachable from any root',
            'node': path,
            'fix': f'{path} is not reachable from any root document — it may be in an isolated cycle'
        }))
"
