# The string corpus, and what the SPA port left behind

`crates/session-core/locales/<lang>/*.ftl` is the canonical string corpus for
both halves of the product: the server renders a few strings itself (tool
prompts, the OAuth callback pages, proxy errors) and the SPA renders the rest.

**Every key must have a consumer.** `mise run parity-gap` fails otherwise, and
it runs as part of `verify` and `ci`. Add a key in the same change that uses
it; remove it in the same change that stops.

Keys built at runtime from a prefix (`settings-f-…`, `nav-group-…`,
`limits-dim-…`) are excluded automatically — the scan derives that list from
the source rather than hardcoding one, so a live dynamic key is never
reported as dead. If yours still shows up, the scan did not recognise the
shape; it looks for `` t(`prefix-${…}`) `` and `format!("prefix-{…}")`.

## What was removed, and why it matters

Issue #22 replaced the server-rendered UI with the SvelteKit SPA. It
reimplemented every screen, but not everything on them, and the leftover
strings were the evidence: **951 of 1768 keys (54%) had no consumer.**

`setup.ftl` was the control case at 0% — that page was reimplemented against
the full catalog, and its orphan count went to zero. Every other surface sat
between 25% and 83%. That is not dead weight; it is the size of a
feature gap.

Those keys are now deleted (the corpus is 817 keys and the guard holds it
there), so **git history is the record**. To see what a screen used to
render:

```bash
git log --diff-filter=D -p -- crates/session-core/locales/en/pools.ftl
```

The capabilities the strings described, which the SPA does **not** have:

| Surface | Was | Missing |
|---|---:|---|
| `pools` | 83% | per-pool allowed-groups (RBAC), GDPR/NDA compliance flags, enforce-limits toggle, offline fallback model, the unknown-model fallbacks section |
| `upstreams` | 82% | the Unassigned-backends section, edit-pool / edit-backend, inactive-model pills, compliance badges |
| `backends` | 78% | 15m/30m/60m activity summary, alias management |
| `admin_users` | 78% | **the impersonation audit log** (who impersonated whom, when), the OIDC-groups column |
| `connectors` | 77% | connector setup guidance, the OAuth explainers |
| `tools` | 75% | **the precise-location sharing UI** — share / stop / accuracy, the browser half of the `get_user_location` tool |
| `rag` | 71% | source editing, sync-token rotation, per-ref index logs, include/exclude globs, allowed-groups |
| `feedback` | 58% | **screenshot capture and canvas annotation** (rect / arrow / pen / text / redact / undo / zoom), the "this is a PUBLIC tracker — check for personal data" confirmation, the browser-log and chat-log opt-ins, voice fill-in |
| `usage` | 57% | the **cost** statistic, the by-backend / by-source / by-token filters, quota used + refresh display |
| `admin`, `webhooks`, `scheduled`, `tokens`, `chat_render`, `render`, `my_skills`, `integrations`, `memory`, `nav`, `limits`, `groups` | 25–68% | assorted columns, filters, empty states and inline help |

Rebuilding any of these starts by reading the deleted block for that file and
the handler beside it in the same commit range.
