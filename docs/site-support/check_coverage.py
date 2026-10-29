"""Check that current product surfaces have explicit published documentation."""

import json
from pathlib import Path
import re
import sys


def check(root):
    docs = root / 'docs'
    manifest = json.loads((docs / 'site-support/coverage.json').read_text())
    published = set(re.findall(r':\s+([\w/.-]+\.md)\s*$', (root / 'mkdocs.yml').read_text(), re.M))
    errors = []
    actual_pages = {str(p.relative_to(root)) for p in (root / 'web/src/routes').rglob('+page.svelte')}
    mapped_pages = {entry['source'] for entry in manifest['ui_pages']}
    for missing in sorted(actual_pages - mapped_pages):
        errors.append(f'UI page needs a documentation mapping: {missing}')
    for stale in sorted(mapped_pages - actual_pages):
        errors.append(f'Documentation mapping names a removed UI page: {stale}')
    for entry in manifest['ui_pages']:
        if entry['documentation'] not in published:
            errors.append(f'UI documentation is not published: {entry["documentation"]}')
    for page in published:
        if not (docs / page).is_file():
            errors.append(f'Published page does not exist: {page}')
    router = (root / 'crates/aiplane/src/rama_server/router.rs').read_text()
    actual_operations = {(method.upper(), route) for method, route in re.findall(
        r'\.with_(get|post|put|delete|patch|head|options)\(\s*"([^"]+)"', router)}
    reference = (docs / 'reference/api.md').read_text()
    documented_operations = set(re.findall(r'\| `([A-Z]+) (/[^`]*)` \|', reference))
    for operation in sorted(actual_operations - documented_operations):
        errors.append(f'Operation missing from exact API index: {operation[0]} {operation[1]}')
    for operation in sorted(documented_operations - actual_operations):
        errors.append(f'API index names an unregistered operation: {operation[0]} {operation[1]}')
    settings = (root / 'crates/aiplane-core/src/server/settings.rs').read_text()
    settings_registry = settings.split('pub static SECTIONS:')[1].split('\n];', 1)[0]
    declared_fields = set(re.findall(r'\b(?:f|f_half|r|f_default)\(\s*"([^"]+)"', settings_registry))
    field_reference = (docs / 'admin/settings-reference.md').read_text()
    documented_fields = set(re.findall(r'^\| `([^`]+)` \|', field_reference, re.M))
    for field in sorted(declared_fields - documented_fields):
        errors.append(f'Settings field needs documentation: {field}')
    for page in published | {'../README.md'}:
        path = docs / page
        if not path.is_file():
            continue
        for target in re.findall(r'\]\(([^\s)]+)\)', path.read_text()):
            if ':' in target or target.startswith(('#', '/')):
                continue
            target_path = target.split('#', 1)[0]
            if target_path and not (path.parent / target_path).exists():
                errors.append(f'Broken relative link in {page}: {target}')
    screenshots = check_screenshots(root, errors)
    return errors, {'ui_pages': len(actual_pages), 'api_operations': len(actual_operations),
                    'settings_fields': len(declared_fields), 'published_pages': len(published),
                    'screenshots': screenshots}


def check_screenshots(root, errors):
    """Every screenshot is used by a page and recorded in the capture manifest
    that `mise run docs-screenshots` and the e2e capture scenarios write, so a
    refresh can reach each one; a manifest entry names an existing image."""
    docs = root / 'docs'
    images = {str(p.relative_to(root)) for p in (docs / 'img').rglob('*.png')} if (docs / 'img').is_dir() else set()
    used = set()
    for page in list(docs.rglob('*.md')) + [root / 'README.md']:
        if not page.is_file():
            continue
        for target in re.findall(r'\]\(([^\s)]+\.png)\)', page.read_text()):
            used.add(str((page.parent / target).resolve().relative_to(root.resolve())))
    manifest_path = docs / 'site-support/captures.json'
    captured = {entry['file'] for entry in json.loads(manifest_path.read_text())} if manifest_path.is_file() else set()
    for image in sorted(images - used):
        errors.append(f'Screenshot is not used by any page: {image}')
    for image in sorted(images - captured):
        errors.append(f'Screenshot has no capture entry in docs/site-support/captures.json: {image}')
    for entry in sorted(captured - images):
        errors.append(f'Capture entry names a missing screenshot: {entry}')
    return len(images)


if __name__ == '__main__':
    errors, counts = check(Path(__file__).resolve().parents[2])
    print(json.dumps(counts, sort_keys=True))
    if errors:
        print('\n'.join(errors), file=sys.stderr)
        sys.exit(1)
