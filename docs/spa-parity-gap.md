# SPA parity gap (issue #22)

The SvelteKit port reimplemented every screen the server-rendered UI had, but
not everything on them. This file records what is missing, and how to see the
current state of it for yourself.

## How this is measured

`crates/session-core/locales/<lang>/*.ftl` is the canonical string corpus for
both halves of the product. A key with no consumer in `crates/**` or
`web/src/**` is a string the old UI rendered and the SPA does not — which,
for a page the SPA *does* have, means a control or a column that did not make
the crossing.

To regenerate the numbers:

```bash
mise run parity-gap
```

It excludes keys built at runtime from a prefix (`settings-f-…`,
`nav-group-…`, `limits-dim-…` and friends), deriving that list from the source
rather than a hardcoded one, so a live dynamic key is never reported as dead.

## Where it stands

At the time of writing: **951 of 1768 keys (54%) have no consumer.**

`setup.ftl` is the control case at **0%** — that page was reimplemented
against the full catalog, and its orphan count went to zero. Every other
surface sits between 25% and 83%, which is the size of the gap rather than a
measurement artefact.

| Surface | Defined | Orphaned | Notable missing capability |
|---|---:|---:|---|
| `pools.ftl` | 63 | 52 (83%) | per-pool allowed-groups (RBAC), GDPR/NDA compliance flags, enforce-limits toggle, offline fallback model, the unknown-model fallbacks section |
| `upstreams.ftl` | 22 | 18 (82%) | the Unassigned-backends section, edit-pool / edit-backend, inactive-model pills, compliance badges |
| `backends.ftl` | 88 | 69 (78%) | 15m/30m/60m activity summary, alias management |
| `admin_users.ftl` | 23 | 18 (78%) | **the impersonation audit log** ("who impersonated whom, when"), the OIDC-groups column |
| `connectors.ftl` | 90 | 69 (77%) | connector setup guidance, the OAuth explainers |
| `tools.ftl` | 16 | 12 (75%) | **the whole precise-location sharing UI** — share / stop / accuracy, which is the browser half of the `get_user_location` tool |
| `rag.ftl` | 206 | 146 (71%) | source editing, sync-token rotation, per-ref logs, include/exclude globs, allowed-groups |
| `render.ftl` | 49 | 34 (69%) | |
| `admin.ftl` | 133 | 90 (68%) | |
| `webhooks.ftl` | 91 | 57 (63%) | |
| `scheduled.ftl` | 95 | 55 (58%) | |
| `feedback.ftl` | 64 | 37 (58%) | **screenshot capture + canvas annotation** (rect / arrow / pen / text / redact / undo / zoom), the "this is a PUBLIC tracker — check for personal data" confirmation, the browser-log and chat-log opt-ins, voice fill-in |
| `usage.ftl` | 58 | 33 (57%) | the **cost** statistic, the by-backend / by-source / by-token filters, quota used + refresh display |
| `chat_render.ftl` | 69 | 39 (57%) | |
| `my_skills.ftl` | 31 | 18 (58%) | |
| `memory.ftl` | 31 | 15 (48%) | |
| `tokens.ftl` | 106 | 51 (48%) | |
| `integrations.ftl` | 59 | 27 (46%) | |
| `nav.ftl` | 54 | 16 (30%) | |
| `limits.ftl` | 43 | 11 (26%) | |
| `groups.ftl` | 20 | 5 (25%) | |
| `settings.ftl` | 181 | 14 (8%) | |

## Why the keys stay

It is tempting to delete them: 951 keys × 6 languages is ~5,700 lines, and
`build.rs` plus `i18n_drift` enforce every one of them in twelve places.

They stay because they are the only remaining record of what these screens
used to do. The old handlers are gone from the tree, so deleting the strings
would erase the specification along with the dead weight, and the gap above
would stop being measurable. Delete a block only when the corresponding
capability has been reimplemented or deliberately dropped — and when it is
deliberately dropped, say so in the commit.
