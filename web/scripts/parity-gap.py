"""Measure the SPA parity gap: .ftl keys with no consumer in either half.

See docs/spa-parity-gap.md for what the number means and why the keys stay.
Run via `mise run parity-gap`.

The dangerous part is dynamic keys: code builds some at runtime from a prefix
plus a value (`t(\\`nav-group-${name}\\`)`), so a plain grep for the full key
finds nothing and would call a live key dead. Every such prefix has to be
excluded, and this derives them from the source rather than hardcoding a list
someone has to remember to update.
"""
import os
import re
import subprocess

ROOT = os.getcwd()
LOCALES = os.path.join(ROOT, 'crates/session-core/locales')

# --- every key defined in en (the reference language) ----------------------
keys = {}
for name in sorted(os.listdir(os.path.join(LOCALES, 'en'))):
    if not name.endswith('.ftl'):
        continue
    path = os.path.join(LOCALES, 'en', name)
    for line in open(path, encoding='utf-8'):
        if line.startswith(('#', ' ', '\t', '\n')):
            continue
        m = re.match(r'^([a-zA-Z][a-zA-Z0-9_-]*)\s*=', line)
        if m:
            keys[m.group(1)] = name
print('defined keys: %d across %d files' % (len(keys), len(set(keys.values()))))

# --- the haystack -----------------------------------------------------------
hay = subprocess.run(
    ['git', 'grep', '-h', '-I', '--', ':!crates/session-core/locales',
     ':!web/src/lib/locales'],
    capture_output=True, text=True, cwd=ROOT,
).stdout
# git grep with no pattern lists nothing; read the tracked files instead.
files = subprocess.run(['git', 'ls-files'], capture_output=True, text=True,
                       cwd=ROOT).stdout.split()
blobs = []
for f in files:
    if f.startswith('crates/session-core/locales/') or f.startswith('web/src/lib/locales/'):
        continue
    if not f.endswith(('.rs', '.ts', '.svelte', '.mjs', '.js', '.py', '.json', '.md', '.html')):
        continue
    try:
        blobs.append(open(os.path.join(ROOT, f), encoding='utf-8', errors='ignore').read())
    except OSError:
        pass
hay = '\n'.join(blobs)
print('haystack: %d files, %.1f MB' % (len(blobs), len(hay) / 1e6))

# --- dynamic key prefixes, derived from the source --------------------------
# Template literals: t(`prefix-${...}`) in TS/Svelte, format!("prefix-{...}") in Rust.
dynamic = set()
for m in re.finditer(r'[`"]([a-z][a-z0-9-]*-)\$\{', hay):
    dynamic.add(m.group(1))
for m in re.finditer(r'format!\(\s*"([a-z][a-z0-9-]*-)\{', hay):
    dynamic.add(m.group(1))
# Rust `&format!("{prefix}{suffix}")`-style and explicit concatenations are
# rarer; catch the common `"prefix-"` + variable shape too.
for m in re.finditer(r'"([a-z][a-z0-9-]*-)"\s*,?\s*\)?\s*\+', hay):
    dynamic.add(m.group(1))
print('dynamic prefixes found: %s' % sorted(dynamic))

orphans = []
for key, src in sorted(keys.items()):
    if key in hay:
        continue
    if any(key.startswith(p) for p in dynamic):
        continue
    orphans.append((src, key))

by_file = {}
for src, key in orphans:
    by_file.setdefault(src, []).append(key)

print('\nORPHANS: %d of %d (%.0f%%)' % (len(orphans), len(keys), 100 * len(orphans) / len(keys)))
for src in sorted(by_file, key=lambda f: -len(by_file[f])):
    print('  %-22s %d' % (src, len(by_file[src])))

out = os.path.join(os.path.dirname(os.path.abspath(__file__)), 'orphan_keys.txt')
with open(out, 'w') as fh:
    for src, key in orphans:
        fh.write('%s\t%s\n' % (src, key))
print('\nwrote %s' % out)
