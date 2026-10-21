import importlib.util
from pathlib import Path
import tempfile
import unittest


spec = importlib.util.spec_from_file_location('coverage_check', Path(__file__).with_name('check_coverage.py'))
coverage = importlib.util.module_from_spec(spec)
spec.loader.exec_module(coverage)


class CoverageTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.write('mkdocs.yml', 'nav:\n  - Guide: guide.md\n')
        self.write('docs/guide.md', '# Guide\n')
        self.write('web/src/routes/+page.svelte', '')
        self.write('docs/site-support/coverage.json', '{"ui_pages":[{"source":"web/src/routes/+page.svelte","documentation":"guide.md"}]}')
        self.write('crates/aiplane/src/rama_server/router.rs', '.with_get("/", handler)')
        self.write('docs/reference/api.md', '| `GET /` | handler |\n')
        self.write('crates/aiplane-core/src/server/settings.rs', 'pub static SECTIONS: &[SectionSpec] = &[f("chat.enabled", Kind::Bool), f_default("chat.voice.directive", Kind::TextArea, "built-in")\n];')
        self.write('docs/admin/settings-reference.md', '| `chat.enabled` | Boolean |\n| `chat.voice.directive` | Text |\n')

    def write(self, relative, text):
        target = self.root / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text)

    def test_complete_surface_mapping_passes(self):
        errors, counts = coverage.check(self.root)
        self.assertEqual(errors, [])
        self.assertEqual(counts['ui_pages'], 1)

    def test_new_ui_page_requires_an_explicit_published_guide(self):
        self.write('web/src/routes/new/+page.svelte', '')
        errors, _ = coverage.check(self.root)
        self.assertIn('UI page needs a documentation mapping: web/src/routes/new/+page.svelte', errors)

    def test_path_prefix_does_not_cover_new_operation(self):
        self.write('crates/aiplane/src/rama_server/router.rs', '.with_get("/", handler).with_post("/new", handler)')
        errors, _ = coverage.check(self.root)
        self.assertIn('Operation missing from exact API index: POST /new', errors)

    def test_new_setting_and_broken_link_are_reported(self):
        self.write('docs/admin/settings-reference.md', '')
        self.write('docs/guide.md', '[Missing](missing.md)\n')
        errors, _ = coverage.check(self.root)
        self.assertIn('Settings field needs documentation: chat.enabled', errors)
        self.assertIn('Settings field needs documentation: chat.voice.directive', errors)
        self.assertIn('Broken relative link in guide.md: missing.md', errors)


if __name__ == '__main__':
    unittest.main()
