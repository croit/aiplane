"""Convert the Fluent catalogs into TypeScript catalogs for the SPA.

The six languages are years of real translation work that the server-rendered
stack accumulated. The SPA reproduces the same surfaces, so the strings map
almost one to one — converting is right and re-translating would be throwing
them away.

Fluent features actually used in this corpus (verified, not assumed): simple
`key = value`, `{ $var }` placeholders, and three selector blocks (two plural,
one a kind switch). Everything else in Fluent is unused, so the parser handles
exactly that and shouts if it meets anything it does not understand.
"""
import json
import os
import re
import sys

SRC = 'crates/session-core/locales'
OUT = 'web/src/lib/locales'
LANGS = ['en', 'de', 'fr', 'es', 'ru', 'zh']

KEY_RE = re.compile(r'^([a-zA-Z][a-zA-Z0-9_-]*)\s*=\s*(.*)$')
SELECTOR_OPEN_RE = re.compile(r'^\{\s*\$(\w+)\s*->\s*$')
VARIANT_RE = re.compile(r'^\s*\*?\[(\w+)\]\s*(.*)$')


def placeholders(text: str) -> str:
    """`{ $name }` -> `{name}`, and `{ "…" }` -> its contents.

    Fluent's own spacing is not part of the message, so the value is stripped
    — which is exactly why Fluent has the string-literal form: `{ " " }` is
    how an author writes a space they *do* want kept at the edge. Expanding
    literals after the strip is therefore load-bearing, not incidental. Get
    the order wrong and `scheduled-next-runs-prefix` renders as
    `Next runs:{ " " }` in the browser, braces and all.
    """
    text = re.sub(r'\{\s*\$(\w+)\s*\}', r'{\1}', text).strip()
    return re.sub(r'\{\s*"((?:[^"\\]|\\.)*)"\s*\}', lambda m: m.group(1), text)


def parse(path: str):
    """-> {key: str | {variant: str}}"""
    out = {}
    lines = open(path, encoding='utf-8').read().split('\n')
    i = 0
    while i < len(lines):
        line = lines[i]
        if not line.strip() or line.lstrip().startswith('#'):
            i += 1
            continue
        m = KEY_RE.match(line)
        if not m:
            i += 1
            continue
        key, rest = m.group(1), m.group(2)
        sel = SELECTOR_OPEN_RE.match(rest.strip())
        if sel:
            variants = {}
            i += 1
            while i < len(lines) and not lines[i].strip().startswith('}'):
                v = VARIANT_RE.match(lines[i])
                if v:
                    variants[v.group(1)] = placeholders(v.group(2))
                i += 1
            i += 1
            out[key] = variants
        else:
            out[key] = placeholders(rest)
            i += 1
    return out


def main():
    os.makedirs(OUT, exist_ok=True)
    catalogs = {}
    for lang in LANGS:
        merged = {}
        d = os.path.join(SRC, lang)
        for f in sorted(os.listdir(d)):
            if f.endswith('.ftl'):
                merged.update(parse(os.path.join(d, f)))
        catalogs[lang] = merged
        print(f'{lang}: {len(merged)} keys')

    # Every catalog must carry the same keys; report drift rather than hide it.
    base = set(catalogs['en'])
    for lang in LANGS[1:]:
        missing = base - set(catalogs[lang])
        extra = set(catalogs[lang]) - base
        if missing:
            print(f'  {lang} MISSING {len(missing)}: {sorted(missing)[:5]}')
        if extra:
            print(f'  {lang} EXTRA {len(extra)}: {sorted(extra)[:5]}')

    header = (
        '// Generated from crates/session-core/locales/<lang>/*.ftl by the\n'
        '// ftl-to-ts converter, then hand-extended with SPA-only strings.\n'
        '//\n'
        '// Keys keep their Fluent names so a string can be traced back to the\n'
        '// catalog it came from, and so the server and the client can name the\n'
        '// same message identically.\n'
        '//\n'
        '// A value may be a plural form: an object keyed by Intl.PluralRules\n'
        "// categories ('one', 'few', 'many', 'other'), selected by `args.count`.\n"
    )
    for lang in LANGS:
        body = json.dumps(catalogs[lang], ensure_ascii=False, indent=1, sort_keys=True)
        with open(f'{OUT}/{lang}.ts', 'w', encoding='utf-8') as f:
            f.write(f'{header}\nimport type {{ Catalog }} from \'../i18n.svelte\';\n\n'
                    f'export const {lang}: Catalog = {body};\n')
        print(f'wrote {OUT}/{lang}.ts')


main()
