# Documentation coverage and verification

The manual describes the current implementation. The publication list in
`mkdocs.yml` selects the user, administration, agent, operations and reference
pages. Existing architecture/design records remain available in the repository;
they are not automatically included in the published manual or LLM exports.

## What is inventoried

| Surface | Inventory and guard |
|---|---|
| Browser pages | Every application screen has an explicit mapping to a published task guide. |
| HTTP operations | Every registered method/path has its own row in the [API index](api.md), including debug-only fixture operations. |
| Operator settings | Every field declared by `settings::SECTIONS` appears in the [settings reference](../admin/settings-reference.md). |
| Built-in tools | The [tool inventory](../tools-inventory.md) is checked against real tool IDs and documented dynamic families. |
| Machine-readable text | Published sources are copied and concatenated by the same build hook used for the website; tests check publication boundaries and build identity. |
| Images | A private capture manifest records screenshot provenance for maintainers. |

Run `mise run docs-check` and `mise run build-docs`. The first checks mappings,
exact operations, settings fields and relative links. The second checks
site navigation, rendered links and generated exports. The existing Rust
tool-inventory and API-index tests run with the gateway suite.

## What the checks prove

The checks prove that inventoried entries have documentation. UI-page and API
checks also detect removed entries; settings checks detect missing entries,
and removal of a settings field needs a content review. They do not prove every
sentence, provider behaviour or production workflow. Each guide was checked
against the running features and their current permissions, prerequisites and
limits.

The documentation change verifies local static serving, setup access, directory
links, caching, traversal boundaries, generated text and screenshots. External
identity providers, hosted models and optional services require their own
working installations; synthetic browser fixtures do not prove those services
work in production. No universal accuracy percentage is inferred from route
counts or successful builds.

## Keep coverage current

When a product surface changes, update its guide and reference in the same
change. A new browser page needs an explicit mapping; a new operation needs
an exact API-index row; a new settings field needs a reference entry; a new
tool needs its inventory row or dynamic-family documentation. Preserve the
conditions and limits that determine what users can actually do.

Refresh screenshots when the control they explain changes. Capture the
relevant component from a synthetic `dev-ui` instance, inspect the image and
put it next to the instructions it supports. Do not use real user data.

Every image under `docs/img/` must be used by a page and recorded in
`docs/site-support/captures.json`; the coverage checker fails on an unused
image, an image without a capture entry and an entry without an image. So
each image has a way to be refreshed: `mise run docs-screenshots` seeds its
state through the API and shoots the images it owns, updating only their
entries, and the scenario images that need a real flow (OAuth, delegation, a
team member) come from their e2e tests with a `DOCS_*_CAPTURE` path set
(`e2e/README.md`).

## Implementation sources

- [`Coverage checker`](../site-support/check_coverage.py)
- [`Publication configuration`](../../mkdocs.yml)
- [`API drift guard`](../../crates/aiplane/tests/it/readme_routes.rs)
- [`Tool drift guard`](../../crates/aiplane/tests/it/tools_inventory.rs)
- [`Export tests`](../site-support/test_exports.py)
- [`Screenshot capture`](../../e2e/docs-screenshots.mjs)
