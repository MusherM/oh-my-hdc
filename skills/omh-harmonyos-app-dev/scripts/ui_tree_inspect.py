#!/usr/bin/env python3
"""Inspect a recovered uitest dumpLayout without treating metadata as UI nodes."""
import argparse
import json
import sys
from collections import Counter

WRAPPERS = ('root', 'roots', 'windows', 'hierarchy', 'nodes', 'tree', 'windowTree')


def read_nodes(value):
    if isinstance(value, list):
        return [node for item in value for node in read_nodes(item)]
    if not isinstance(value, dict):
        return []
    attrs = value.get('attributes', value)
    kind = attrs.get('type', attrs.get('componentType')) if isinstance(attrs, dict) else None
    children = read_nodes(value.get('children', []))
    if isinstance(kind, str) and kind.strip():
        properties = {k: v for k, v in attrs.items() if k != 'children'}
        return [{'type': kind, 'attributes': properties, 'children': children}]
    if children:
        return children
    return [node for key in WRAPPERS if key in value for node in read_nodes(value[key])]


def summarize(roots):
    types = Counter()
    deepest = 0
    stack = [(node, 0) for node in roots]
    while stack:
        node, depth = stack.pop()
        types[node['type']] += 1
        deepest = max(deepest, depth)
        stack.extend((child, depth + 1) for child in node['children'])
    return {'node_count': sum(types.values()), 'by_type': dict(sorted(types.items())),
            'max_depth': deepest, 'root_count': len(roots)}


def render_tree(roots):
    lines = []
    def visit(nodes, prefix=''):
        for index, node in enumerate(nodes):
            last = index == len(nodes) - 1
            attrs = node['attributes']
            selected = ['id', 'accessibilityId', 'key', 'text', 'content', 'description',
                        'bounds', 'clickable', 'enabled', 'visible']
            props = ' '.join(f'{key}={json.dumps(attrs[key], ensure_ascii=False)}'
                             for key in selected if key in attrs)
            lines.append(prefix + ('└─ ' if last else '├─ ') + node['type'] +
                         (' ' + props if props else ''))
            visit(node['children'], prefix + ('   ' if last else '│  '))
    visit(roots)
    return '\n'.join(lines)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('layout', help='Local JSON recovered using hdc file recv')
    parser.add_argument('--format', choices=['text', 'json'], default='text')
    args = parser.parse_args()
    try:
        with open(args.layout, encoding='utf-8-sig') as stream:
            roots = read_nodes(json.load(stream))
        if not roots:
            raise ValueError('No typed UI nodes: empty/failed dump or unsupported schema; inspect raw JSON')
        if args.format == 'json':
            print(json.dumps({'summary': summarize(roots), 'tree': roots}, ensure_ascii=False, indent=2))
        else:
            print(render_tree(roots))
    except (OSError, ValueError, RecursionError) as error:
        print(f'UI tree inspection failed: {error}', file=sys.stderr)
        return 2
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
