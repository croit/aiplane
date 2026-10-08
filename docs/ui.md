# Web UI

The UI is a **SvelteKit SPA** (Svelte 5 runes, `adapter-static`) that lives in `web/`, is compiled ahead of time by Vite into plain static files, and is served **from the root `/`** by the Rust binary. It talks to AIplane exclusively over the session-authenticated JSON API at `/api/v0/*`, and streams chat over a JSON-SSE event protocol. Styling is [daisyUI v5](https://daisyui.com/) on Tailwind v4.

There is **no Node at runtime**: still one container, one process, one port. The container image contains the gateway binary plus the built `build/` directory; `AIPLANE_STATIC_DIR` points the binary at it.

The whole stack:

| Layer | Tech | Lives in |
|---|---|---|
| HTTP server / router | rama 0.3 | `crates/aiplane/src/rama_server/router.rs` |
| Static SPA hosting | hand-rolled rama handler over `tokio::fs` | `crates/aiplane/src/rama_server/spa.rs` |
| UI framework | SvelteKit 3 / Svelte 5 (runes), Vite | `web/` |
| API contract | OpenAPI 3.1, generated from the router and the handlers' wire types | `GET /openapi.json` (`crates/aiplane/src/rama_server/openapi/`) |
| API client | one `fetch` helper + hand-declared shapes | `web/src/lib/api.ts` |
| Chat streaming | JSON events over SSE | `crates/session-core/src/chat_json.rs` ↔ `web/src/lib/chat-protocol.ts` |
| Styling | Tailwind v4 + daisyUI v5 | `web/src/app.css` |
| Markdown rendering | `marked` + `dompurify`, client-side | `web/src/lib/markdown.ts` |

## How it is served

`crates/aiplane/src/rama_server/spa.rs` serves the build directory named by the `AIPLANE_STATIC_DIR` environment variable. It is deliberately hand-rolled rather than a `tower-http::ServeDir`, because the dependency policy keeps the server stack rama-only (see [`dependencies.md`](dependencies.md)); the logic is small and unit-tested end to end.

What it does, and why:

- **Mounted at the root, registered last.** `GET /` and `GET /{*name}` are the final two routes in `router.rs`. rama matches in registration order, so a catch-all registered any earlier would swallow the API, proxy and auth routes. Anything added after them would be unreachable.
- **History fallback.** A client route like `/settings/tokens` has no file on disk. The server falls back to `index.html` for the SPA's top-level route allowlist (`SPA_ROUTES` in `crates/aiplane/src/rama_server/spa.rs`); unknown paths return 404. Add a new top-level client route to that list or a hard load will fail while client navigation works.
- **Cache policy by kind.** SvelteKit emits content-hashed asset filenames, whose bytes never change at a given URL — those get `public, max-age=31536000, immutable`. `index.html` and `sw.js` get `no-cache`, or an update would never reach a browser; the manifest gets a short revalidating max-age.
- **Traversal guard.** The relative path is normalised *on its own* before being joined to the root, so a leading `..` with nothing to consume is refused. Normalising after the join would let the root's own components absorb the `..` — safe, but it would silently turn the guard into a no-op.
- **Case is preserved.** rama lowercases only the *matched* path for route lookup; the `Request` handed to the handler keeps the original URI, so `req.uri().path()` still carries the case of a content-hashed filename. (Same precedent as `retrieve_model` — see [the note in `router.rs`](../crates/aiplane/src/rama_server/router.rs).)
- **503, not 404, when undeployed.** No `AIPLANE_STATIC_DIR` (or a missing directory) answers `503` with a message naming the variable. That is deliberately distinct from the router's 404 so an operator can tell "the SPA build was never copied in" from "that path does not exist". The rest of AIplane is unaffected.

`crates/aiplane/tests/it/spa_routes.rs` pins the *wiring* (the catch-all really reaches this handler, proven by the 503); `spa.rs`'s own unit tests pin the serve behaviour against a temp directory.

### The first-run gate

`rama_server::first_run` redirects the whole surface to the setup wizard until setup completes. Because the wizard *is* the SPA, `serves_before_setup` allowlists the SPA's static shell — `/_app/*`, `/assets/*`, `/icons/*`, `favicon.*`, `manifest.webmanifest`, `sw.js`, `robots.txt`, `pcm-recorder.js` — plus `/setup*` and `/auth/callback`. Gating a JavaScript module request would 303 it to an HTML page and leave the wizard blank. Those files are unauthenticated static bytes; the SPA's own API calls self-protect with 401/403.

## Layout of `web/`

```
web/
├── vite.config.ts        adapter-static config + the dev-server proxy
├── package.json          SPA build toolchain (see docs/dependencies.md)
├── src/
│   ├── app.html          the shell: manifest link, icons, pre-paint theme script
│   ├── app.css           Tailwind entry + the two daisyUI theme blocks
│   ├── lib/
│   │   ├── api.ts             the fetch helper + typed wrappers + ApiError
│   │   ├── admin-client.ts    same transport for the admin surfaces
│   │   ├── chat-protocol.ts   the SSE event fold — framework-free, unit-tested
│   │   ├── chat.svelte.ts     reactive conversation controller (EventSource lifecycle)
│   │   ├── page-titles.ts     production-compatible static/dynamic title routing
│   │   ├── page-title.ts      dynamic page-title override shared with the shell
│   │   ├── session.svelte.ts  identity from GET /api/v0/me
│   │   ├── sidebar.svelte.ts  conversation list + search + mobile drawer
│   │   ├── feedback.svelte.ts feedback widget state (dialog, voice, submit)
│   │   ├── feedback-capture.ts     console + network ring buffers, redacted
│   │   ├── feedback-screenshot.ts  snapdom / getDisplayMedia page capture
│   │   ├── feedback-annotator.ts   canvas annotator (rect/arrow/pen/text/redact)
│   │   ├── push.svelte.ts     Web Push opt-in (device-local state)
│   │   ├── voice.svelte.ts    voice-conversation orchestration
│   │   ├── voice-recorder.ts  the shared recorder (web/shared/) as Blobs + worded errors
│   │   ├── markdown.ts        marked → DOMPurify → {@html}
│   │   ├── ui-variants.ts     class strings for components/ui/ (see Theming)
│   │   ├── components/ui/     Modal, ChoiceCard, SegmentedControl, ChipToggle,
│   │   │                      AiSuggestion, StatusPill, StepIndicator
│   │   └── usage-types.ts
│   └── routes/
│       ├── +layout.svelte     app shell: nav, sidebar, theme, sign-out, feedback
│       │                      (the feedback FAB + dialog mount HERE so they
│       │                       survive client-side navigation)
│       ├── +layout.ts         prerender = false, ssr = false
│       ├── +page.svelte       dashboard
│       ├── login/ chat/ chat/[id]/ inbox/ tokens/ tools/ memory/ scheduled/
│       ├── webhooks/ skills/ integrations/ usage/ setup/ rag/ rag/profiles/
│       └── admin/             +layout.svelte + models, upstreams, users, groups,
│                              tokens, limits, settings, skills, connectors,
│                              comfyui
└── static/               copied verbatim into the build output
    ├── manifest.webmanifest, sw.js, robots.txt
    ├── favicon.svg, icons/*.png
    └── pcm-recorder.js   AudioWorklet processor (its own JS realm — not bundled);
                          the gateway embeds the same file for the widget
                          (/api/v0/embed/recorder.js)
```

`+layout.ts` sets `prerender = false` and `ssr = false`. Prerendering would bake the anonymous shell into every route, and this is a private surface: the identity render would flash "signed out" on first paint anyway, and no per-route HTML should be emitted for an authed page.

The desktop sidebar keeps route navigation and conversation history as separate
regions. Route groups never scroll: Workspace is open by default, Account and
Admin are collapsed, and the user's choices persist for one year in the
`nav_sections` cookie. The conversation list receives the remaining height and
is the sidebar's only vertical scroll region. This preserves access to the
identity controls and avoids nested scrollbars when several route groups are
open.

The authenticated page shell owns horizontal padding and gives every route the
full remaining width beside the sidebar. Route roots must not add page-sized
`max-w-5xl` / `max-w-6xl` containers or repeat the shell padding; narrower
measures still belong on prose, forms, dialogs, and other content whose own
readability requires them. The chat canvas measures its conversation container,
not the browser window: on desktop it stays below 46% and 768 px while reserving
at least 560 px for chat, and on smaller screens it remains a full-screen panel.
The chat route is bounded to the viewport. Its transcript and canvas scroll
independently, while the composer stays visible as a full-width footer beneath
both regions; the document itself must not become the chat scroll container.

The conversation header carries the chat model, voice model and spoken-reply
voice selectors at every width: inline beside the actions from `sm` up, on a
full-width row of their own beneath them below it. It is one set of controls,
not a mobile copy, so the options and the selected model are the same on both.

### Bounded viewport

A page that is a conversation fills the window rather than scrolling as a
page: `boundedViewport(url)` (`lib/viewport.ts`, unit-tested) names them —
the chat and an agent's Try it → Test chat. For those the shell's `main`
does not scroll and its content is `h-full`; the page hands that height down
a flex column (`min-h-0 flex-1` at every level), so only the message list
scrolls and the composer stays in view whatever the header above it takes.
No page guesses the space above it with a `calc(100dvh - …)`.
`chat-viewport-layout.test.ts` pins the chain.

The personal pages use one shared `SectionTabs` navigation component with
path-backed tabs. `/tools` contains built-in tool controls (including location
sharing), `/tools/integrations` the user's MCP connections, `/tools/skills`
their private skills, and `/tools/browser` the setup of the browser-control
extension: live status through `browser-bridge.ts`, the store link, the
`.zip` download, and the steps (see [`browser-control.md`](browser-control.md#for-users)). `/settings` contains the account summary;
`/settings/notifications`, `/settings/memory`, and `/settings/tokens` keep the
corresponding personal controls separate. The memory page's Preferences card
takes what it says about standing context from `GET /api/v0/memories` →
`preferences`: `in_context` is the same `recall` gate the chat driver applies
(`openai_driver::preferences_in_context`), and the count and character limits
are the driver's own constants, so the card cannot promise more than a turn
sends. The skills and notifications tabs
follow their optional feature switches. The OAuth callback and connect/retry POST endpoints stay
under `/integrations/*`; they redirect back to `/tools/integrations`.
The built-in tools tab shows the full registered catalog. A tool that lacks
required storage, an indexer, or push is disabled with a reason; admins also
see unregistered image, GeoIP, and sandbox tools as read-only rows linking to
the matching operator setup page. These placeholder rows are not offered to
the model or persisted as user tool preferences.

The admin sidebar has one **Access & limits** entry. Its four path-backed tabs
keep `/admin/users`, `/admin/tokens`, `/admin/groups` and `/admin/limits` at
their existing URLs, with their existing forms and APIs. The Limits tab follows
the optional limits feature switch. Group grant and identity subtabs remain
inside the Groups tab. `/admin/settings` stays a separate sidebar page.

The Notifications tab shows device controls when Web Push initialized. If the
feature is configured but its sender failed to initialize, the tab shows an
unavailable message and an admin link to operator settings rather than an empty
page.

Within `/settings/tokens`, `?tab=tokens` manages tokens and `?tab=guides`
contains client setup guides. Each token is a card (`ManagedTokenCard`) with
three tiles — **Models**, **Tools**, **Budget** — that show the current state
(GDPR/NDA exposure and the dearest token price; tool use and the MCP ask
policy; the tightest quota with its spend) and open one dialog each. Every
dialog edits a draft and commits on Save; nothing on the card saves on a
click. Rotate and Revoke sit in the card's ⋯ menu. The model dialog is
`TokenModelPicker`, shared with the operator restriction on `/admin/tokens`:
models grouped by kind, each with its GDPR/NDA badges and price (an alias is
priced as its target), filters for compliance and price, and a warning when
the selection reaches a model without GDPR or NDA cover. The data comes from
`/api/v0/tokens/details`: `models` is the catalog
(`UpstreamRegistry::model_catalog_for` plus `model_defaults` prices),
`owner_limits` the owner's own in-force limits, and each token's
`quota_status` its quotas with what has been spent against them. A newly minted or rotated secret stays above
these inner tabs until the page is left, so switching to a guide does not hide
the one-time value. The guide's `client` query parameter selects OpenCode,
Claude Code, Pi, Oh My Pi, or Python; code examples use the browser's current origin so they
also work on self-hosted domains. All guide instructions live in the six Fluent
catalogs.

Long or data-driven choices use the shared `SearchableSelect` combobox instead
of a native select. It searches labels, stored values, descriptions, keywords and
badges; supports arrow keys, Enter and Escape; and bounds long result lists to
their own scroll region. Model choices render the model id left-aligned and show
compact GDPR and NDA status badges right-aligned on every row. Green check badges
mean the model is cleared; red crossed-out badges mean it is restricted. The
trigger looks and focuses like any other field (field border, primary focus
ring), and keyboard-active options use a subtle surface change. In forms it takes
the default size like the fields around it; `size="sm"` is for toolbars and filter bars. Small
closed enums such as hour/day/week or on/off remain native selects because a
search field would add friction without improving discovery.

The same component in `multiple` mode backs every multi-value **grant** field —
a group's tools and skills, and the `allowed_groups` on a pool, a RAG collection
and an MCP connector. The panel stays open while picking, the trigger summarises
("3 selected"), and the selection logic lives in `multi-select.ts` so it is
testable without a DOM. A field gets a picker when its value set is closed and
the server knows it; a field whose set is open keeps free text with suggestions.
`oidc_values` on a group is the open case and deliberately stays typed: its
suggestions come from claim values seen on past logins, so the list is empty on a
fresh install, and pre-creating the admin group's mapping before anyone has
logged in is the day-one setup step. The grant wildcard `*` is a row of its own
rather than "every box checked" — it resolves against the live registry at call
time, so expanding it into today's ids would silently narrow the grant.

The root layout owns the document title through the route registry in
`page-titles.ts`; data-driven pages publish their resolved name through the
shared override in `page-title.ts`. Conversation metadata is refreshed
after both sidebar changes and final turn events, so an asynchronous generated
title cannot update the sidebar while leaving the header or browser history
stale.

## The JSON API and its contract

Every dynamic thing the SPA does is a `/api/v0/*` call — over 220 operations
across more than 150 paths. `GET /openapi.json` serves the OpenAPI 3.1 document
for them, generated at runtime; there is no detached contract file to copy into
the container or keep in sync.

The document has two sources, checked against each other:

- **Which operations exist** is read from `router.rs` itself, so the document
  lists exactly what is mounted, with each path's parameters.
- **What each operation takes and answers** comes from its declaration in
  `crates/aiplane/src/rama_server/openapi/` (one module per area: account,
  admin, agents, chat, rag, workspace). A declaration names the credential
  (`x-aiplane-access` plus the OpenAPI `security` requirement: the session
  cookie, the embed visitor token, the setup claim, or none), the request
  body and query types, each success status with its body, and the error
  statuses. The schemas are derived with `schemars` from the very types the
  handler deserializes and serializes, so a field renamed in Rust is renamed
  in the document. Every error status answers with the one envelope,
  `shared::api::ErrorEnvelope` (`{"error":{"message","type","code",…}}`), which
  every `/api/v0` refusal is built from (`json_error`). A body-taking operation
  also lists `413`, which its body cap answers.

`every_registered_route_is_declared_exactly_once` fails when a route in
`router.rs` has no declaration, or a declaration has no route, so a new route
cannot ship undescribed. A new route therefore needs a declaration, and its
handler's request and response types need `#[derive(schemars::JsonSchema)]`;
an ad-hoc `json!` body has no schema to derive, so give the response a type.

Every refusal is the shared error envelope (`json_error`), with the status its
meaning calls for: 404 for a resource that does not exist or that the caller
may not see, 502 when a backend the gateway relies on (an upstream, the object
store, the issue tracker) failed, 503 when the feature is not configured. A
`200` never carries a failure.

**Operations without a derived schema.** A payload with no wire type to derive
from is declared with a reason, served as `x-aiplane-schema-unsupported`, and a
test requires it to be listed here:

- `POST /api/v0/transcriptions` — the success body is the transcription
  backend's own answer, relayed unchanged; the gateway has no type for it.

**How calls are actually made today.** Every request goes through one helper, `request<T>()` in `lib/api.ts`: a same-origin `fetch` that sends the session cookie, parses the error envelope once, and throws an `ApiError` carrying the status, `code`, the server's sentence (`serverMessage`), a validator's per-field `issues` and the raw body (`detail`). Nothing else re-parses the envelope: the agent views (`parseSpecError`), the inbox and `lib/admin-client.ts` (the same transport for the admin views, flattening the error into a plain `Error`) read those fields. On the server every refusal is `json_error`, or `json_error_with` when it carries fields of its own (`issues`, `failing`). `lib/api.ts` then exposes the `api.*` wrappers routes call, with their response shapes declared by hand against `shared::api`.

There is deliberately no generated client. One shipped briefly (`client.ts` over an `openapi-typescript` `schema.d.ts`) and was removed: nothing ever imported it, so it was 6k generated lines and two npm dependencies standing in for a migration that never happened. If typed calls become worth it, generate them from the live `/openapi.json` document and migrate `api.ts` deliberately, rather than leaving a second unused client in the tree.

A 401 means "signed out": `+layout.svelte` turns "`me` is null after load" into a redirect to the standalone `/login` card carrying the route the user actually wanted. The card starts `/auth/login` only after the user chooses **Continue with OIDC**, and never bounces `/setup` (which runs before any account exists).

`GET /api/v0/build` is public because both `/login` and the authenticated
sidebar must offer the corresponding source before or after a session exists.
It returns the runtime `AIPLANE_SOURCE_URL` and the binary's exact version/git
label; the reusable `SourceLink` component renders that metadata in both places.

The model administration read model lives at `GET /api/v0/admin/models`; model
override writes use `PUT /api/v0/admin/models` and
`DELETE /api/v0/admin/models/{name}`. Feature defaults and web-search settings
are independent resources at `PUT /api/v0/admin/model-defaults` and
`PUT /api/v0/admin/search-settings`. Keep those mutation paths at the top level
of the admin namespace: rama treats sibling parameterized route shapes as one
route, so placing both below `/api/v0/admin/models/*` can dispatch a literal
settings path through the wrong handler.

`GET /api/v0/admin/skills` is the global-skill master-detail read model. Each
skill includes its stripped Markdown body, bundled file paths, direct group
grants, and groups inheriting access through the `*` grant; the response also
reports the configured source directory and whether it is accessible. Upload,
delete, and grant replacement use the same resource, while
`GET /api/v0/admin/skills/{name}/archive` packages the selected global skill
for download. Grant writes accept only existing gateway groups and never copy
an inherited all-skills grant into a per-skill row.

`GET /api/v0/admin/connectors` is the complete connector-catalog read model:
identity and presentation metadata, endpoint, scope, authentication and OAuth
discovery configuration, encrypted-secret presence, access groups, audit and
enablement state, built-in provenance, setup readiness, and the deployment's
exact OAuth redirect URI. `PUT /api/v0/admin/connectors` creates or replaces a
definition, while the key-specific toggle and delete routes manage its
lifecycle. Enabling is rejected while required OAuth setup is missing, and a
blank secret on replacement keeps the existing encrypted value. Audited
connectors expose their newest 200 tool calls at
`GET /api/v0/admin/connectors/{key}/audit`.

`GET /api/v0/comfyui/catalog` supplies the operator page with the effective
worker URL and catalog directory, execution timeout and poll interval, the
complete model-facing workflow/parameter schemas, and the newest 20 persisted
jobs. Job rows retain terminal output filenames or failure details so the page
is useful for diagnosis rather than only catalog reloads. A successful
`POST /api/v0/comfyui/reload` atomically swaps the workflow snapshot and the SPA
refreshes this read model without restarting AIplane.

### Routes that are not `/api/v0`

Public triggers and provider authorization round trips use these routes outside `/api/v0`:

| Route | Why it is not under `/api/v0` |
|---|---|
| `GET /hooks/{secret}`, `POST /hooks/{secret}`, `POST /hooks/rag/{token}` | Public triggers. The URL *is* the credential; a third party (a file host's webhook, a cron line) calls them. |
| `GET /rag/{id}/connect`, `GET /rag/oauth/callback` | RAG source OAuth round trip. The redirect URI is registered with an external provider, so the path is not ours to change. |
| `POST /integrations/{key}/connect`, `POST /integrations/{key}/retry`, `GET /integrations/callback` | Per-user MCP connector OAuth, same shape. |

Provider callback failures render a small standalone, localized HTML document:
the provider detail is escaped, the recovery action returns to the SPA, and the
normal shell is deliberately absent because the callback can fail before the
SPA or a usable session exists.

## Chat streaming: the JSON event protocol

Live chat is a single SSE stream:

```
GET /api/v0/chat/sessions/{id}/events
```

Each frame is `event: <name>` plus one JSON `data:` line carrying `{type, …}`. The server side is `crates/session-core/src/chat_json.rs` (`ChatEvent`); the client side is `web/src/lib/chat-protocol.ts`, which folds events into a `ConversationState`.

| Event | Payload | Meaning |
|---|---|---|
| `snapshot` | `live_turn_id?`, `turns[]` | Full session state, sent once on attach. Rebuilt from the DB. |
| `turn_delta` | `turn_id`, `text_delta`, `full?` | Text appended to the assistant turn's content. |
| `reasoning_delta` | `turn_id`, `text_delta`, `full?` | Same, for the reasoning trace. |
| `tool_call_started` | `turn_id`, `tool_call_id`, `name`, `arguments` | The model invoked a tool. `arguments` is the model's raw JSON string. |
| `tool_call_done` | `turn_id`, `tool_call_id`, `status`, `output?` | A tool call reached a terminal status. The output is the **full** payload — truncating is a display decision and belongs to the client. |
| `turn_finalized` | `turn_id`, `status`, `error_message?`, `error_code?`, `model?`, `duration_ms` | Terminal: no further deltas for this turn. `error_code: loop_exhausted` means every retry of a looping call looped too; the SPA shows `chat-loop-exhausted` in the reader's language instead of the English `error_message`. |
| `attempt` | `turn_id`, `attempt` (`seq`, `effort`, `retry_effort`, `stop_reason`, `reasoning`, `content`) | A model call of the turn looped and is retried at `retry_effort`; its partial output left the answer. Sent before the `full` deltas that rewind the answer. A snapshot's rows carry the same list as `attempts`. Never sent to embed visitors. |
| `suspended` | `turn_id`, `request_id`, `kind`, `message?`, `tool_call_id?`, `tool?`, `options[]`, `expires_at` | The turn paused at a tool call that waits for a decision, and its worker is gone. Ends the stream, but the turn is **not** over. `kind` is `approval`, `secure_input` or `human_answer`; `options` lists the decisions a resume may carry (`allow_once` / `deny` / `value`). The same object rides on the turn in a snapshot as `suspension`, with the turn's `status` `suspended`. A visitor of an agent conversation gets it without `tool_call_id` and `tool`, and with `options` narrowed to the decisions that are theirs (empty for an approval, which staff give). |
| `steer` | `turn_id`, `id`, `text`, `status` | A mid-turn interjection appeared or reached its outcome. One event for both, merged on `id`, so a client attaching late ends up in the same state as one that watched from the start. |
| `sidebar_changed` | — | Session metadata changed (title generated, pin toggled). Deliberately payload-free: the list endpoint is the source of truth, so the client refetches. |
| `info` | `message` | Transient notice, rendered as a dismissible banner (e.g. a vision fallback). |
| `tool_prompt` | a `ToolPromptEvent` | Human-in-the-loop prompt — `ask_user`, a location request, or a tool confirmation — plus its `Hide` counterpart when it is answered, times out, or the turn ends. |
| `idle` | — | No live worker for this session; nothing more will arrive. |

Invariants worth knowing before you touch either side:

- **The DB is the replayer.** There is no `Last-Event-ID` replay. A client that reconnects simply re-attaches, and the first `snapshot` — rebuilt from SQLite — subsumes anything missed. The worker runs to completion independently of any HTTP listener and writes its progress to the DB as it goes, so closing a tab mid-stream loses nothing.
- **Deltas append, unless `full: true`.** `full` marks a cursor reset: the row was rewritten and `text_delta` carries the *whole* text, so the client must replace its buffer rather than append. A client cannot detect a rewrite on its own, which is why the server says so. There are no delete events by design.
- **Flushes are coalesced** to ≥120 ms per subscriber with a trailing flush, so the final state always lands. Each flush reads one turn, not the conversation.
- **A suspended turn is answered over JSON, not the stream.** `POST /api/v0/chat/sessions/{id}/turns/{turn_id}/resume` takes `{"decision": "allow_once" | "deny" | "value", "value"?: …, "request_id"?: …}`; owner-only, `202` once the turn's worker runs again, after which the client re-attaches. `409 not_suspended` when the turn is not waiting or `request_id` names an older pause, `400 decision_not_offered` for a decision its `options` lack, `409 turn_in_progress` when no slot is free. A message sent into a paused conversation is refused with `409 decision_pending`, and `…/cancel` on it gives the decision up; the chat page keeps the draft in its composer but will not send it meanwhile, and says why (`pausedTurn`, `chat-composer-paused`). The chat page draws a paused turn as the [suspension card](#suspension-card), led by `chat-waiting-*` and showing the waiting call's arguments from the transcript; only the owner gets its buttons. A person's paused run (a scheduled action or a webhook) is answered from the [inbox](#inbox), which calls this same resume.
- **The stream ends at `turn_finalized` / `suspended` / `idle`.** The server closes there, so `chat.svelte.ts` closes the `EventSource` too, and fires `onTurnFinalized` on `turn_finalized` and `suspended` alike (the turn stopped running) — letting it auto-reconnect would loop snapshot/idle forever on a quiet session. `applyEvent` folds `suspended` into the turn (`status: suspended`, `suspension`), as a snapshot's row carries it. After a submit (or any suspected change) `attach()` reopens, and the fresh snapshot is the replay.
- **An interrupted worker leaves a terminal turn.** The shared worker harness catches driver and tool panics, records the assistant turn and any still-running tool calls as `errored`, and broadcasts `Finalized`; its caller then releases the worker slot. On attach, if the owner has no live worker, the events handler also errors any already-present `in_progress` assistant turns before sending the snapshot. It targets only turn IDs read before rechecking the worker registry, so a newly starting turn cannot be mistaken for an orphan. Viewers of a shared conversation never perform this recovery because they cannot see the owner's worker in their own registry lookup.
- **Every scheduled or webhook run links its chat and ends terminal.** A run's history row gets its `session_id` the moment the chat is opened (webhooks at `record_run_start`, schedules via `attach_run_session`, which also moves the action's `last_session_id` so a reusing schedule keeps its thread); closing a run with no session never erases that link. The scheduler wraps each run in a panic guard, so the row is always closed through `record`. A process that dies mid-run leaves rows pending; `sweep_interrupted_runs` closes them at startup as `error` — before the scheduler's first tick and before the server accepts webhook fires, so nothing live can be caught by it — and corrects the list row when the orphan was the newest run. The runs lists join `chat_sessions` and report `chat_deleted`; list rows get `last_chat_deleted`, and the SPA shows "chat deleted" rather than linking to a 404.
- **A reusing schedule or webhook can continue in any of its owner's chats.** Reuse always meant "append to `last_session_id`"; the form's *Continue in* picker (`LinkedChatPicker`) sets that pointer directly through `linked_session_id` on create/update — an id links that chat, `""` lets the next run open a fresh one, an absent field leaves it alone (a run may have moved it since the form opened). The server accepts only the caller's own chat, and only with `reuse_conversation` on. The picker starts on `last_session_id` even before reuse is switched on, because that is where a reusing run would continue; a deleted one reads as "a new chat", which is what the next run does.
- **The public embed stream speaks the same frames, buffered.** `GET /api/v0/embed/events` (visitor token, read with `fetch` streaming) sends `snapshot`, then either `idle` or — once the running turn is terminal — its whole answer as one `turn_delta` with `full: true` and `turn_finalized`. When the conversation waits for a decision, or the running turn pauses, the stream ends with `suspended` (the visitor view above) instead. A request staff answer has empty `options`: the widget shows that it waits and re-attaches every 10 s until the answer arrives. The visitor answers a `secure_input` with `POST /api/v0/embed/resume {request_id, decision, value?}` (the widget's masked code field) and re-attaches. A host page's signed identity token goes to `POST /api/v0/embed/identity {token}`. A message sent meanwhile is queued (`placement: "queued"`) and shows in the snapshot's `waiting_turn_ids`. Tool calls, reasoning and unfinished answers are stripped from what a visitor receives. See [`agent-visitors.md`](agent-visitors.md) and [agent-run suspend](agent-hil.md#suspend-and-resume).
- **Markdown is the wire format.** The server sends text; the client renders it (`marked` → `DOMPurify` → `{@html}`). Model output is untrusted input like any other, so the sanitise step is not optional.

The rest of the conversation surface is ordinary JSON. `GET /api/v0/chat/landing`
resolves the caller's latest conversation and creates one only when the caller
has none. `GET …/sessions/{id}` returns the session and turns plus
`compacted_up_to_seq` and the conversation's attachment-marker-derived
`assets[]`; the SPA uses those fields for the compaction boundary and canvas
asset browser. Attachment markers are reconciled with those canonical assets by
turn and filename before rendering, so stale marker URLs do not produce duplicate
or unavailable cards. Relative Markdown image sources resolve through the same
asset set and are suppressed when the attachment gallery already renders that
image. `POST …/messages` submits multipart data when there are attachments, and
`POST …/turns/{turn_id}/edit` accepts that same multipart
shape so editing does not lose the composer's attachment support. The other
mutations include `…/cancel`, `…/fork`, `…/share`, `…/effort`,
`…/documents/*` (canvas and version history), `…/export.md`,
`…/export.pdf`, and `…/turns/{turn_id}/retry`.

## The composer during a turn

The composer stays usable while an answer streams, and **Enter always means
send**. Where the message lands is the server's decision, because it is the
only party that knows whether a worker is running:

| Placement | When | What the user sees |
|---|---|---|
| `started` | nothing was running in this conversation | an ordinary turn |
| `folded` | a turn was already running here | the text appears inside that answer as an addition, and goes into its prompt at the next tool round — every pending addition at once, not one per round |
| `queued` | this user's other conversations hold every parallel slot | the message sits in the transcript marked "sent — waiting for a free slot", with a *take back* action |

`POST …/messages` answers `202` with that `placement`. There is no client-side
outbox: a waiting message is a **user turn with no answer after it**, which is
both what the transcript renders and what the scheduler reads. It therefore
survives a closed tab, shows up on another device, and cannot be sent twice by
two browsers.

Why not a new `chat_turns.status`: that column is read in 66 places (export,
the FTS triggers, compaction, webhooks, history replay, the startup sweep), and
a fifth variant would have to be right in all of them. "No answer after it"
needs no new state. What a waiting turn needs in order to *start* later — the
model, the voice flag, the caller's IP — lives in `chat_pending_turns`, a work
queue whose rows are deleted the moment a worker claims one.

The scheduler (`start_pending_turns` in `pages/chat/mod.rs`) runs at every
moment a turn can become startable: a turn finishing, a conversation being
cancelled, a message being queued, and the process coming back. No timer, no
polling. Claiming is `DELETE … RETURNING`, so two schedulers racing cannot
start the same turn twice — and queueing kicks it, because the slot can free up
between "at capacity" and the row landing.

A page watching a waiting message is told when it starts: the events stream
does **not** end at `idle` for a conversation with something queued. It stays
open, waits for the registry's `WorkerStarted` announcement
(`stream_until_started`), and hands over to the live turn with a snapshot that
names it — `live_turn_id` is what tells the client a turn is streaming, so
without that second snapshot the page would receive deltas while still
believing it is idle. Bounded at five minutes, after which the stream ends as
`idle` like any quiet conversation. Which messages are waiting travels in the
snapshot (`waiting_turn_ids`) rather than being inferred from the transcript's
shape: a turn whose assistant row failed to insert looks identical and would
spin forever.

An addition only reaches the model if another round follows. One that arrives
while the closing answer is being written does not, and at finalize the server
turns it into the next message itself — in the same place, whether or not a
browser is open. The transcript says which happened: `pending`, `delivered`,
`resent`, or `discarded`. Only `delivered` notes replay in the history a later
turn sees; a `resent` one is already there as its own user turn.

**Interrupt and re-aim** is the deterministic version: send, then stop. Sending
folds the text into the running turn, stopping guarantees no round will carry
it, and the finalize path re-queues it as the next message. The cancelled
partial answer stays in the transcript *and* in the model's replayed history,
marked as interrupted, so the second attempt continues from what the reader
saw. Cancelling is cooperative — the worker notices between upstream chunks —
so a slot frees when the turn actually ends, not when the button is pressed.

## Parallel conversations

One worker per conversation is a hard invariant: two would interleave writes
into the same transcript. How many *conversations* one user may stream at once
is an operator setting — `chat.turns.max_parallel` under Chat in
`/admin/settings`, default `1`, read at submit time so a change takes effect on
the next message.

The registry (`session-core/src/workers.rs`) keys on `(user id, session id)`.
Past the ceiling nothing is refused any more: the message is persisted and
waits. Retry and edit still refuse with `409` — regeneration rewrites history a
running turn is reading, so there is nothing sensible to queue.

It is also where **agent conversations' turns** run (`agents::embed::claim`,
see [agent-visitors.md](agent-visitors.md)), keyed by the principal
that owns the conversation and uncapped per principal — one agent answers many
visitors at once. So "is a turn running", cancel and shutdown's
`cancel_all`/drain cover every turn from one place. Two registry operations
exist for them, generic in themselves: `hand_over(user, session, from, to)`
points a worker at the next turn it produces without letting go of the
conversation, and `holds` / `cancel_turn` act on one turn only, never the
conversation's next. When a worker leaves the registry (`clear`) its channel
carries a last `TurnUpdate::Released`, after `Finalized`. `Finalized` says the
turn's *row* is final; `Released` says the worker is gone. A chat stream
treats the two alike, but an agent's caller may hold its worker past
`Finalized` (until the output filter has ruled), and a subscriber that must
see only the settled answer waits for `Released`. `subscribe(user, session)`
takes the subscription under the registry lock, so it either sees the worker
or is sure to hear it go.

`GET …/sessions/{id}/capabilities` is the conversation's complete capability
read model. Each built-in tool, connected integration tool, and skill carries
its group, description, ordering metadata, and explicit `off` / `auto` / `on`
state. The picker searches and groups this response; it does not reconstruct
capabilities from unrelated endpoints.

The capability picker is a full-screen dialog at every viewport size. Search
is global, state filters expose `off` / `auto` / `on` in words, and the desktop
category rail becomes a labelled category select on narrow screens. Changes
save immediately; category-level controls use the same mutation path as an
individual tool and preserve `can_disable` by falling back to `auto`.

The list itself — group rail, search, one row per resource — is
`components/capabilities/CapabilityBrowser.svelte` (logic in
`capability-picker.ts`), shared with the agent setup's *Knowledge &
abilities* step. The caller brings each row's control (the chat: off / auto /
on; an agent: an on/off toggle), what shows under it, a filter and the order.
A row shows the resource's own title and description and nothing in place of
a missing description: who may maintain the resource (`editable`) sees "No
description" and a *Configure* link to its `config_url`, everyone else only
the name.

`GET /api/v0/usage` is the complete usage-dashboard read model. Period,
scope, source, backend, and token filters are query parameters so a view is
reconstructable from its URL. The response includes the effective scope and
admin capability, metrics state, viewer timezone, totals, pricing gaps,
in-force limits, and user/token/backend/source/model breakdowns. The backend
clamps `scope=all` for non-admin users; the client reflects the effective
scope rather than trusting the requested one.

`GET /api/v0/admin/limits` is the operator limit-policy read model: rules plus
the complete role, user, token-with-owner, and model option sets needed to
assign them safely. The SPA keeps those identifiers behind labelled selects,
shows the policy resolution and metering rules beside the editor, and preserves
the production table's separate dimension, window, and locale-formatted value
columns. `POST /api/v0/admin/limits` creates or upserts a selected rule; with
the existing rule's `id`, it edits that row in place. `DELETE
/api/v0/admin/limits/{id}` removes it.

`GET /api/v0/admin/settings` carries the declarative settings specification
into the SPA: category and section membership, effective values, field kinds
and spans, feature enablement, valid models for model fields, secret-presence
flags, restart-pending keys, and whether AIplane still needs its first
backend. The category is a bookmarkable `?tab=` value. Disabled feature cards
keep their master toggle visible and fold the remaining fields; each section
saves independently through `POST /api/v0/admin/settings`, while write-only
secrets use the explicit `/api/v0/admin/settings/clear` action to return to the
built-in default.

`chat-protocol.ts` is deliberately framework-free — no Svelte, no DOM — so the fold is unit-testable under `node --test` (`chat-protocol.test.ts`, run by `mise run test-web`) and `chat.svelte.ts` stays a thin reactive wrapper around it. Keep it that way: wire behaviour that can only be tested through a browser is wire behaviour nobody tests.

## Agent builder

`/agents` (sidebar: Workspace → Agents) is visible only when `GET /api/v0/me`
reports `can_manage_agents`; everything behind it is the `/api/v0/agents/*`,
`/api/v0/system-principals/*` and `/api/v0/agent-resources` surface described
in [`agents.md`](agents.md#agent-definition). The data layer and the pure helpers
are `web/src/lib/agents.ts` (unit-tested in `agents.test.ts`); the components
are in `web/src/lib/components/agents/`.

- **`/agents`**: the agents shared with the caller, and a dialog to create one.
  A new agent opens in the setup assistant (`/agents/{id}/setup/start`).
- **`/agents/{id}`** (`AgentShell` from the route's layout, `AgentWorkbench` as
  the page): header with live/draft badges, the open-points pill and Save
  draft / Publish / Delete, and four tabs. `?tab=` keeps the tab in the URL and
  `?sub=` the panel inside it:
  - **Setup**: the plain-language overview ([Agent setup](#agent-setup)), or
    with the *Advanced editor* switch (`?view=advanced`) the expert panels
    below — *Builder*, *Canvas*, *JSON*, *Grants* — on the same buffer.
  - **Try it**: *Test chat* and *Tests*.
  - **Insights**: *Analytics* and *Activity*.
  - **Settings**: *Versions* and *Sharing* (shares at `respond`, `read` or
    `write`, notification channels, embed keys).

  The panels:
  - *Builder*: collapsible sections. **Main agent** (model, orchestration and
    response instructions, tools and skills from the agent's grants, per-tool
    permission and `bind` rows, budget), **State slots**, **Routes and
    sub-agents** (router, and per route: description, gate, sub-agent picker,
    task, route binds) and **Settings** (profile, `finish` schema, tool
    unavailability, publish settings). Gates use a recursive structured editor
    (`CondEditor`: all / any / not / slot check with `set`, `eq`, `in`,
    `provenance`, `max_age`). `finish.schema` is edited as JSON, and verifiers,
    `human` routes and `subject` slot schemas only in the JSON tab.
  - *Canvas*: the agent's fixed topology drawn from the spec
    (`AgentCanvas`; the layout, path mapping and test-path logic are the pure
    `web/src/lib/agent-canvas.ts`, unit-tested in `agent-canvas.test.ts`).
    Columns are main agent, one gate per route, the route, and its target
    (sub-agent, `human`, or any other kind key such as `a2a` or `loop`, drawn
    generically by that key); rows follow route order. The main agent node also lists the spec's `verifiers` as badges (id and kind, any kind). It is not a free-form
    graph: nodes cannot be moved or wired, and nothing is stored but the spec.
    Nodes are HTML buttons (daisyUI `card`, `badge`) over an inline SVG of
    edges, laid out by `layoutCanvas` with fixed sizes, so the same spec always
    draws the same way. The canvas scrolls horizontally inside its own box.
    - *Editing.* Selecting a node opens a side panel (below the canvas on
      narrow screens) with the form editors for that part: the main agent node
      the main agent form, state slots and router; a gate node the structured
      gate editor; a route node its name and description; a target node the
      sub-agent picker, task and binds (`RouteEditor` with `part`, the same
      component the Builder tab renders whole). "Add route" and "Remove route"
      use `addRoute` / `removeRoute` / `renameRoute` from `agents.ts`, the
      same helpers the Builder uses, and the canvas binds the same `spec`
      buffer as the Builder and JSON tabs, so an edit in one shows in the
      others and the unit tests pin canvas edit == form edit.
    - *Validation.* `issuesByNode` maps each 422 issue `path` to a node
      (`main`/`state`/`router`/`verifiers` to the main node,
      `routes.<r>.when…` to the gate, `.agent`/`.human`/`.task`/`.bind` to the
      target, the rest of `routes.<r>` to the route). Nodes with issues get an
      error border and count; the panel lists the node's issues. Paths no node
      shows (`profile`, `finish`, `publish`) stay in the header list.
    - *Last test turn.* The Test chat reports its latest debug view to the
      workbench, and the canvas shows each gate open or closed, the picked
      route (green ring and edges), and the sub-agents that ran. Nothing shows
      until a turn happened in this page visit.
    - *Accessibility.* Every node is a focusable button (Enter or Space opens
      the panel, which takes focus), Escape closes it and returns focus to the
      node, and transitions are off under `prefers-reduced-motion`.
  - *JSON*: the whole spec; Apply replaces the builder's buffer.
  - *Grants*: the principal's grants with revoke, and a grant form whose
    suggestions come from `/api/v0/agent-resources`. The server's refusal
    (`grant_exceeds_manager`, unknown resource) is shown verbatim.
  - *Test chat*: see below.
  - *Tests*: stored test cases and suite runs (`TestsPanel`, see
    [`agent-builder.md`](agent-builder.md#evaluation)). A case form edits the conversation
    script (visitor messages, and trusted slot writes as `host` or
    `verifier:<id>` between them) and the deterministic expectations (last turn
    finished, output-filter outcome, route chosen or none, gates with the slots
    they still miss, sub-agents and tools called or not, bound values, answer
    contains or not) plus an optional rubric. The source selector runs the saved
    draft (saving an unsaved buffer first) or any published version; the run
    shows each case as a Goal / Plan / Action card with every check, what was
    expected and what happened, and the conversation. The rubric verdict is a
    separate dashed block labelled as model-judged, and never colours pass or
    fail. A warning shows when the draft or the cases changed since the latest
    draft run, because the publish guard
    (`publish.require_passing_tests`, a checkbox in Settings) would refuse it.
    The pure half, `web/src/lib/agent-tests.ts` (form model to and from the
    stored case, run state), is unit-tested.
  - *Versions*: draft vs live, the publish blockers (`publish_issues`), every
    snapshot with its JSON, and "make live" (rollback).
  - *Analytics*: what the agent did over the last 7, 30 or 90 days, for all
    versions or one (`GET /api/v0/agents/{id}/analytics`, see
    [`agent-activity-log.md`](agent-activity-log.md#analytics)). `AnalyticsPanel` shows stat tiles
    (conversations, messages, sub-agent runs, gate refusals, output-filter
    blocks, limit refusals, human handoffs, model calls, tokens, cost when
    priced), a per-day bar chart with a metric switch, and breakdown tables
    (routes chosen, closed gates by route and missing slot, why sub-agent runs
    did not finish, output-filter actions, limit refusals by kind). The chart
    is a plain inline SVG (`fill-primary`, daisyUI tokens): no chart library.
    Its pure half, `web/src/lib/agent-analytics.ts`, is unit-tested. Route,
    slot and reason names are shown as the identifiers they are.
  - *Activity*: the agent's activity log (`GET /api/v0/agents/{id}/activity`,
    see [`agent-activity-log.md`](agent-activity-log.md)). `ActivityPanel` lists every
    conversation's events newest first, or one conversation oldest first,
    grouped by turn (a sub-agent's run is its own group, badged
    *sub-agent*); each event is a daisyUI `collapse` whose title is the kind
    (an identifier, shown as `code`), time, round, duration and a one-line
    summary, and whose body is the full detail as JSON — a model exchange's
    whole request and answer, a tool call's arguments and result. Filters:
    conversation, a kind group, a day range; "Load more" follows
    `next_cursor`. *Export JSONL* links the export with the same filters;
    *Verify chain* shows the verify result as an alert. The pure half,
    `web/src/lib/agent-activity.ts` (query, kind groups, turn grouping,
    summaries), is unit-tested.
  - *Sharing*: shares with access change and revoke; `share_needs_agent_manager`
    and `last_writer` are shown verbatim. A `read` share sees everything
    read-only (the editor is a disabled `fieldset`).
- **Validation errors by path.** The 422 `invalid_agent_spec` carries
  `{path, message}` issues (`main.tools[0]`, `routes.billing.when.all[1].slot`).
  `FieldIssues` shows the messages for exactly its field's path under the
  field, each section's badge counts the issues beneath it
  (`issuesUnder`), and the header lists them all. Issues are those of the last
  Save/Publish; the Versions tab separately lists what blocks publishing.
- **Editing buffer.** The spec is a `$state` object bound by the inputs;
  `ensureShape` gives every container a home and `cleanSpec` drops blanks
  before a save (`when` and `schema` are treated as opaque). Both clone through
  JSON because `structuredClone` refuses a Svelte proxy.
- **Test chat** (`TestChat`, `DebugPanel`). A streamed conversation like
  any other: `POST /api/v0/agents/{id}/test/messages` (`sendTestMessage`)
  runs the **saved draft** in the background, so an unsaved buffer is
  flagged with a Save button, and a `ConversationController` opened on
  `GET …/test/{session}/events` (`createConversationController(id,
  eventsUrl)`) folds the frames, exactly as the chat page does. The
  transcript and composer are `StreamedChat` (below). Whenever no turn runs,
  each answer that stopped and has no debug view yet (`turnsToRead`) gets one
  from `GET …/test/{session}/turns/{turn}/debug` (`testTurnView`) — a pause
  just answered only once the stream shows it past that request
  (`settleAnswered`), and a `409 turn_in_progress` is read again when the
  turn settles rather than shown (`readFailure`) — and a fresh attach follows, so an answer the output filter rewrote shows as a
  visitor would get it. "Show what happened" under an answer shows its
  debug: slots with value and provenance, each by its label from the spec
  (`slotLabel`; the hand-off and identity slots by catalog name) with the id
  in small print, each route's gate with what keeps it closed (`gateHint`:
  the unmet condition in catalog words with the slot's label, the server's
  message only for a condition without a slot), a strict scope's topic-guard
  verdict, the routing decision, sub-agent calls with outcome, and tool-call
  decisions; the newest answer's is shown otherwise. Slots and gates are the
  conversation's as they are when the view is read. "New conversation"
  forgets the conversation.
- **It fills the window.** Try it → Test chat is a
  [bounded viewport](#bounded-viewport) like the chat: the agent page passes
  the height down a flex column (`AgentShell` → the tab's content →
  `TestChat` → the chat and "Behind the reply" grid), the transcript scrolls,
  and the composer stays in view at every zoom and in every language; no
  offset is guessed (`chat-viewport-layout.test.ts`).
- **A paused test turn.** An answer whose turn is `suspended` shows the
  [suspension card](#suspension-card) under it, led by what it waits for
  (`waitingLead(kind, 'test')`), with the call's arguments from the
  transcript. The answer goes to `POST
  /api/v0/agents/{id}/conversations/{session}/turns/{turn}/resume` (`202`);
  the chat attaches again and the same turn continues on the stream, and its
  debug view is read anew once it stops, so a verifier's slot or a gate the
  decision opened shows. In a test conversation the manager may answer a
  `secure_input` too. A hand-off to a person (`human_answer`) shows its
  context as the inbox would (the debug view's `handoff`), and a line saying
  that in a live conversation it lands in the Inbox of the agent's managers
  and responders. The card sits in the bubble's column of the daisyUI `chat`
  grid (`col-start-2`).
- **`StreamedChat`** (`components/chat/`) is the compact conversation the
  test chat and the [agent architect](#agent-architect) share: the turns of a
  controller as chat bubbles (tool calls, markdown, the working spinner, the
  error), following the end with `chat-autoscroll`'s rules while the reader
  is there, and the composer (Enter sends, Shift+Enter breaks the line). What
  differs comes in as snippets: the architect's Undo row and dictation, the
  test chat's status line, debug button and suspension card.
- **Embed keys.** The Sharing panel's *Embed keys* card lists the agent's keys
  (name, origins, who created them, revoked or not) from `GET
  /api/v0/agents/{id}/embed-keys`, revokes one, and creates one from a name
  and an origin per line. The server returns the key only in the create
  response, so the card shows the ready-to-paste `<script>` tag
  (`embedSnippet`, pointing at this gateway's `/embed.js`) once, right then.
  The widget itself is [`embed.md`](embed.md).
- **Test conversations have no history list.** They are stored with
  `agent_version = 0` and swept by retention like any other.

### Agent setup

The way a manager who is not technical sets an agent up. It edits the
same spec as the advanced editor and never shows it: every step reads its
plain-language model out of the spec and writes it back. The pure half is
`web/src/lib/agent-setup.ts` (unit-tested in `agent-setup.test.ts`, including
that every step round-trips through the advanced editor's
`ensureShape`/`cleanSpec`); the components are in
`web/src/lib/components/agents/setup/`.

**Nothing about an existing resource is made up.** A model, voice,
connector, skill, knowledge base, tool or agent is shown by its own data —
the title and description its source keeps (the `items` and `models` of
`GET /api/v0/agent-resources`, the agents list), or its reference as it is
when the viewer cannot read more (an agent not shared with them, a grant they
do not hold). There are no stand-in descriptions, no ids made readable on the
client and no placeholder names; a missing description is fixed at the
resource (e.g. `/rag/<id>/edit`), which the abilities step links to for who
may edit it. Labels and instructions about the setup itself are ordinary
catalog strings.

- **One buffer for every page.** `routes/agents/[id]/+layout.svelte` mounts
  `AgentShell`, which creates the `AgentWorkspace`
  (`lib/agent-workspace.svelte.ts`: detail, versions, resources, the spec
  buffer, save, publish, grant helpers) and hands it down through context. The
  overview, the assistant and the advanced editor all bind the same
  `ws.spec`, so moving between them keeps unsaved edits; a reload loads the
  saved draft.
- **Overview** (`SetupOverview`, the Setup tab): one row per section — task &
  tone, topics, knowledge & abilities, information to collect, identity
  check, hand-offs, website — with a one-line summary (`summary`), a
  `StatusPill` (*Done*, *Open* when the checklist has an item for it,
  *Optional*) and *Edit*. Beside it the pre-publish checklist
  (`SetupChecklist`, from `checklist`) whose *Fix* opens the step that fixes
  the item, and a sketch of the widget (`WidgetPreview`: the name and, under a
  strict scope, the refusal an off-topic question gets). Publish in the header
  is disabled while a blocking item is open; a missing website is advisory.
  Server `publish_issues` join the checklist by path (`stepForPath`); one no
  step reaches links to the advanced editor.
- **Edit = the same step in a centred `Modal`**, sized to the step. It binds a
  copy of the buffer; *Apply* puts it back and saves the draft. A refused save
  keeps the modal open with the server's reasons, and *Cancel* then restores
  the buffer as it was. Never a side drawer.
- **The assistant** (`SetupAssistant`) is its own route,
  `/agents/{id}/setup/{step}` with `step` one of `start`, `basics`, `scope`,
  `abilities`, `slots`, `identity`, `routes`, `site`, `review`: deep-linkable,
  the browser's back goes one step back, a reload lands on the same step, and
  the footer (Back / Step n of 9 / Next or Done) stays at the bottom on a
  phone. A `StepIndicator` jumps between steps. *Next*, a jump and *Done* save
  the draft first and stay on the step when the save is refused, which is what
  makes a reload safe.
- **Steps** (`SetupStep` picks one; each is a component shared by both hosts):

  | Step | What the person sees | What it writes |
  |---|---|---|
  | Start | Template cards (Website FAQ, Customer support with identity check, Qualify leads, Internal helper, Start blank) and a scenario field with *Suggest a setup* (below) | the template's spec (`agent-templates.json`, texts from the catalog), keeping the name and the model already chosen; asks before replacing a set-up agent |
  | Task & tone | name, what the agent does, tone chips, answer language, free text, *Model* | `profile.display`, `main.instructions.orchestration`; `main.instructions.response` as one fixed English line per chip and language (`TONE_LINES`) plus the free text, so lines no chip stands for survive; `main.model` (unset = the gateway default) |
  | Topics | topic chips, the answer for other topics, *Enforce strictly* | `scope`; an empty scope is removed, `classifier_model` kept |
  | Knowledge & abilities | the chat picker's `CapabilityBrowser` over the `items` of `GET /api/v0/agent-resources` (knowledge bases, tools, connectors, skills with their own titles and descriptions, linked to their edit pages for who may edit them), an on/off toggle each; switched-on and AI-suggested rows first; what the agent holds but the manager does not is listed by its reference under "Granted by someone else" | grants (below; every tool id of a catalog entry) and `main.tools`, `main.skills`; one collection binds `rag_search.collection` as a constant, several add `rag_list_collections` |
  | Information to collect | label + friendly kind (text, longer text, e-mail, phone, customer number, order number, date, number, whole number, yes/no, choice) | `state.<key>` with `SLOT_SHAPES[kind]`, `set_by: [llm]`, the label as `description` (a slot without one is shown by its key as it is), the row's position as `order` (the server hands object keys back sorted, so the list order lives there); a new row's key follows its label (`identFrom`); a slot of any other shape shows as "advanced" and is kept |
  | Identity check | four `ChoiceCard`s: none, code by e-mail, signed in on your website, customer number + name | `verifiers.identity` (`mcp_code` with a connector, `host_jwt` HS256 with issuer, audience and a generated secret, `lookup` with a tool) and `state.verified` (`subject`, `set_by` the verifier or `host`), plus the slots it reads; switching method moves the hand-off gates' `provenance` along; *none* is refused while a hand-off needs a confirmed identity |
  | Hand-offs | sentences: "When it is about [topic] and [always / all details are collected / the identity is confirmed / both], hand over to [a person / Specialist: X]", plus "Otherwise … [hand over to a person / end politely]"; the topic field grows with its text | one route per rule: `when: {all: [{slot: topic, eq}, {slot: request, set: true}, ({slot: <detail>, set: true} per slot of the details step; a route whose `set` leaves name only some of the details is not a rule but a kept route), ({slot: verified, provenance})]}` — saving the details step, or the identity step adding the slots it reads, regates a rule that waits for them (`withDetails`) — `agent` + `task: "Request about {topic}: {request}"` + `bind` derived from the specialist's live spec (`deriveBind`), or `human: {}`; the fallback is route `fallback` on `request` set; `state.topic` (enum of the topics) and `state.request`; `router.order` rules, other routes, fallback. Routes of any other shape are kept and counted. With a hand-off to a person, *Announce a hand-off to a person* offers what exists — push when the gateway sends it (`GET /api/v0/push/config`), Slack / Discord once the agent has a channel of that service (the agent's `ChannelsPanel`, embedded below it, adds and removes channels through the same routes as the inbox settings) — and writes it as each person route's own `human.notify` (`Rule.notify`, `Handoffs.fallbackNotify`): one choice for all of them while they agree (`sharedNotify` / `notifyEverywhere`), one per route once they differ, so no route is rewritten unasked; everything on is written as no list, which the run reads as every channel (`notifyOn` / `setNotify`) |
  | Website | the websites (one per line), a widget sketch, *Create embed code*; voice in and out with their models, and the voice picked from the speech model's own `voices` (`speechVoices`) | `publish.origins` (each reduced to its origin); the key itself is created through the embed-keys API and shown once; `publish.voice` |
  | Check & test | every section's summary, the checklist, the proposed test conversations with *Save as test*, a link to *Try it* | a saved test goes through `POST …/tests` |

- **Grants follow the cards, on save.** Switching a card on, choosing a model
  or picking the identity check's system *stages* the grant it needs
  (`lib/agent-grant-plan.ts`, unit-tested; `AgentWorkspace.stageGrant`);
  switching it off edits the spec and stages revoking the grant unless the
  published version still uses it (`liveUses`; the card says so). The steps
  show the grants as they will be (`ws.grants`), and a staged plan counts as
  unsaved. `AgentWorkspace.save` carries the plan out — grants before the draft
  (the validator checks them; a refusal such as `grant_exceeds_manager` stops
  the save with the server's reason), revocations after — so the assistant's
  Next / Back / Done, the modal's Apply and the header's Save draft all apply
  it, and the modal's Cancel drops what the edit staged. A card for something
  the agent holds but the manager does not is shown disabled ("Granted by
  someone else").
- **Model choice.** One `ModelPicker` (`components/agents/ModelPicker.svelte`):
  the chat picker's `SearchableSelect` with the options `modelSelectOptions`
  builds (`#lib/model-option`, the GDPR / NDA badges included), led by
  *Default (<model>)* for the gateway's default model of that kind
  (`defaults.<kind>` of `GET /api/v0/agent-resources`, Models & routing →
  Default models). The list is `models.<kind>` — what the manager may use and
  so grant, built like `GET /api/v0/models` (an automatic route only when the
  manager may use its fallback, every candidate and its selector) — plus the
  models the agent holds that no list of the manager's names
  (`modelPickerOptions`). Choosing a model stages its `model` grant and sets
  `main.model`; *Default* removes the key and stages the default's grant when
  the manager may give it (`modelGrantFor`), otherwise the step says the
  default is out of their reach (`defaultOutOfReach`). What the agent ran on
  before is staged for revoking unless the spec still runs on it, defaults
  of unset keys included (`modelsInUse`). A new agent starts on *Default*.
  The advanced editor's main-model and classifier-model pickers use the same
  component over the agent's granted models only —
  [`agents.md`](agents.md#models).
- **Voice.** The *Website* step's transcription / speech pickers are the same
  `ModelPicker` over `models.transcription` / `models.speech`
  (`publish.voice.transcription_model` / `speech_model`). Switching a
  direction on starts it on *Default* and stages the default's grant. A
  direction the manager may grant no model for shows why rather than a switch.
- **Errors.** `setupErrorMessage` (`agent-setup.ts`) turns a failed call into a
  catalog message — the assistant unavailable (404/405/501/503), its model
  failing (502), a network failure, a rate refusal with its `Retry-After` — and
  keeps the server's own envelope message otherwise. A raw status line such as
  "405 Method Not Allowed" is never shown.
- **Strings.** Everything a person reads is in `agent_setup.ftl` (six
  languages), the templates' texts too (`@key` strings in
  `agent-templates.json`, which `tests/it/agent_test_chat.rs` creates in every
  language to prove each is a valid draft). What the model reads — tone and
  language lines, the hand-off task, the descriptions of `topic` and
  `request` — is English, like the agent's system message.
- **The prompt assistant** ([`agent-builder.md`](agent-builder.md#prompt-assistant)).
  *Suggest a setup* sends the scenario, the chosen template and the current
  buffer to `POST …/assist/suggest`; the proposal is kept on the workspace
  (`ws.suggestion`), and with the scenario and the parts already handled in
  the tab's `sessionStorage` per agent (`proposal-memory.ts`), so a reload or
  a step opened by its URL still offers it; every step shows its part in an `AiSuggestion`
  (`SuggestionBox`) with *Apply* / *Dismiss*: the task, the tone (chips by
  their ids and translated labels, the answer language, and only the rest as
  free text — `suggestedTone`), the topics, the tools by their card titles
  (granted when applied) and the knowledge bases (switched on as their cards
  are), with a note per subject no knowledge base covers yet, the details
  (`suggestedSlotRows`), the identity card (`suggestedMethod`), the
  hand-offs as sentences with their condition (`applySuggestedRules`: a
  hand-off that waits for the identity, on an agent without an identity
  check, sets up the check the proposal recommends first, so the gate is
  written rather than dropped) and, on the last step, test conversations. Applying edits the step's model like a
  manual edit, so it reaches the spec only through the normal save; the
  endpoint writes nothing. What the assistant left out is listed on the start
  step with its reason. The task, the tone and the answer for other topics
  each have *Improve* (`ImproveText`, `…/assist/improve`): the proposed text
  before / after with the reason, applied only on *Apply*.
- **The agent architect**: see below.

### Agent architect

A conversation that plans an agent with the person and writes the draft
([`agent-builder.md`](agent-builder.md#agent-architect)). `ArchitectModal`
(`lib/components/agents/`) is a centred `Modal` (`lg`, the body about
70 dvh high; never a side drawer) holding `ArchitectChat`, which mounts only
while it is open.

- **Entry points.** The agents list header (*Plan with the architect*), the
  new-agent dialog (*Or plan it in a conversation with the architect*, which
  swaps the dialog for the architect) and the setup overview's call to
  action, which plans that agent.
- **The conversation** is the person's chat (`POST /api/v0/agent-architect`
  opens their newest one about the agent, *New conversation* a fresh one)
  and streams through the same `createConversationController` as `/chat`,
  drawn by `StreamedChat` (shared with the test chat). User turns are
  `chat-end` bubbles, the architect's answers Markdown, and every tool call
  shows in `ToolCalls` with its input and output.
- **Undo and links** (`lib/architect.ts`, unit-tested): a completed
  `update_agent_draft` offers *Undo*, which restores the revision it kept
  (`agentsApi.restoreDraft`; the server also revokes the grants that change
  made unless something still uses them) and then shows *Undone*; on the agents list a
  call naming a setup page offers *Open setup*. After every finished turn
  (and an undo) the host reloads: the list its agents, the overview the
  workspace with `refresh(ws.dirty)`, which keeps an unsaved edit of the
  person's buffer rather than overwriting it.
- **Voice** is the composer's dictation: `DictationButton` with the first
  transcription model (`/api/v0/transcription_models`), the transcript
  appended to the text field. It shows only when a transcription model is
  available. Enter sends, Shift+Enter breaks the line.
- **Strings** are `architect-*` in `agent_setup.ftl`.

## Inbox

`/inbox` (sidebar: Workspace → Inbox) lists what
waits for them: an agent's approvals and handoffs when they are an admin, a
manager with a `read` or `write` share, or hold a `respond` share (the
agent's responders), and their own paused scheduled or webhook runs. It is the `/api/v0/agents/inbox` surface of
[`agent-hil.md`](agent-hil.md); the data layer and pure helpers are
`web/src/lib/inbox.ts` (unit-tested in `inbox.test.ts`).

- **The sidebar entry** appears only where something can arrive
  (`inboxShown`): an item waits (with its count as a badge), the person
  answers for at least one published agent, or the inbox is open. Both
  facts ride the `inbox` frame of `GET /api/v0/agents/inbox/events`
  (`{count, answers}`); `answers` is the inbox's own standing rule
  (`inbox::answers_for_published`) over the published agents, read when the
  stream attaches. An installation without agents shows no inbox until a
  person's own run pauses; a push or channel link opens `/inbox` either way.

- **An item** (`{…, question?, call?, detail?, context?}`: `detail` is an
  approval's `message`, such as `schedule_action`'s preview) is a
  [suspension card](#suspension-card) under a heading with
  the agent (or the run's title), its kind and when it was asked. A
  manager's item links to the agent, an owner's to the chat; a responder's
  links nowhere, since they may open nothing else.
- **`?item=<id>`**, the link every notification carries, scrolls to and
  highlights that item.
- **Live.** `web/src/lib/inbox.svelte.ts` keeps one `EventSource` per tab on
  `GET /api/v0/agents/inbox/events`; each `inbox {count}` frame updates the
  sidebar badge (hidden at 0) and makes the open page refetch the list. The
  stream ends after ten minutes and `EventSource` reconnects on its own,
  which is what it is for here, unlike the chat stream.
- **Workbench.** The agent workbench's Sharing panel (Settings tab) offers
  the `respond` level next to `read` and `write`: users or groups who answer
  the inbox and need no agent-management permission. The subject is picked
  with `SearchableSelect` in server-search mode (`onsearch`: the component
  shows the caller's results unfiltered) from
  `GET /api/v0/agents/{id}/share-subjects?q=` — a few matches from two
  characters on, never the roster ([`agents.md`](agents.md#shares)). A
  subject that may not hold `read` or `write` is refused by the server, and
  its message is shown. Below it, *Notification channels* (Slack or
  Discord incoming webhooks; the URL is write-only, the list shows its host,
  whether the message carries details, and its language). A `read` share sees
  both read-only.


### Suspension card

`SuspensionCard.svelte` (`web/src/lib/components/`) is the one card for
whatever a paused turn waits for, wherever it is answered: the inbox, the
agent builder's test chat and a person's own chat. Its pure half is
`web/src/lib/suspension.ts` (unit-tested in `suspension.test.ts`).

- **What it shows.** A hand-off's question, the visitor's last message, the
  slots by their labels (`slotLine`: the `label` the server copies from
  `state.<slot>.description` when the hand-off is stored, the hand-off and
  identity slots by catalog name, else the key; a value the model may not see
  as who vouched for it), the transcript when the route hands it over; an
  approval's tool and its arguments, pretty-printed, and what the call would
  do when the tool says so (the request's `message`, e.g. `schedule_action`'s
  preview); the minutes left.
- **What it offers** is exactly the request's `options` (`decisionButtons`):
  Approve once and Deny for an approval, an answer field with its submit
  button and Decline for a value. A request offering nothing (a visitor
  waiting for staff) shows no button at all. A `secure_input` value is typed
  into a password field, a staff answer to a hand-off into a text area
  (`answerField`); an empty answer is refused in place.
- **Where it differs** is passed in, never decided inside: a `lead` line
  (`waitingLead`: the chat's `chat-waiting-*`, the test chat's
  `agents-test-waiting-*`), a `note`, a heading snippet (the inbox's), the
  answer callback, and the error to show.
- **Strings** are `suspension-*` in `suspension.ftl`.
## Reactive state

Shared state lives in `.svelte.ts` modules exporting `$state` objects, built as factories rather than classes — `$state` in a module closure is the documented universal-reactivity pattern, and the returned object's methods close over it directly.

- `session.svelte.ts` — `me`, loaded once per app start; `null` while unknown *and* on 401, with a `loaded` flag so the layout can tell the two apart.
- `sidebar.svelte.ts` — the conversation list, its search box, and the mobile drawer. Pages call `refresh()` after anything that changes a conversation so the list never drifts.
- `chat.svelte.ts` — one controller per conversation view; owns the `ConversationState` plus the `EventSource` lifecycle.
- `push.svelte.ts` — Web Push opt-in. This state is **device-local**, not server state: two browsers of the same user subscribe independently.

## Theming

`web/src/app.css` registers Croit `light` and `dark` daisyUI themes with the built-in palettes switched off. They are one design in two lightnesses, taken from the agent-setup mockup:

| Token | Dark (default) | Light | Used for |
|---|---|---|---|
| `base-100` | `#1b1b1d` | `#ffffff` | the page, field fills |
| `base-200` | `#242427` | `#f5f4f8` | cards, panels, default buttons |
| `base-300` | `#3a3a40` | `#dedce4` | the one line colour: card edges, tables, dividers (`border-base-300`) |
| `base-content` | `#ecebf0` | `#1c1b22` | text; muted text is `text-base-content/60` |
| `primary` | `#8558f0` | `#7c3aed` | actions, selection, focus ring |
| `neutral` | `#2d2d31` | `#1c1b22` | raised chips, scrims |
| `success` / `warning` / `error` | `#4ade80` / `#fbbf24` / `#f87171` with dark text | `#15803d` / `#b45309` / `#c81e1e` with white text | alerts, badges, `StatusPill` |

Shapes are shared: `--radius-field` 10 px (buttons, inputs, tabs), `--radius-box` 14 px (cards, alerts, modals), `--radius-selector` 8 px (badges, checkboxes, toggles), no depth or noise. Secondary stays the brand peach. `lib/theme.test.ts` keeps both themes on the same token set and checks every text-on-fill pair for WCAG AA (4.5:1); change a colour and that test tells you whether it still reads. The dark primary is a shade under the mockup's `#8b5cf6` because white text on `#8b5cf6` is only 4.2:1.

Cards are solid `base-200` panels with a `base-300` edge (one rule in `app.css`, so no page repeats it). Fields have one look app-wide: daisyUI's `input` / `select` / `textarea` at the default size (40 px) in forms, `-sm` only inside dense rows (table cells, toolbars, filter bars); a focused field rings in `primary`, and a field with `aria-invalid="true"` (or the `-error` variant) is error-coloured, focused or not. daisyUI 4's `input-bordered` & co. are inert in v5 and rejected by `markup-drift.test.ts`. The shared layout adds subtle peach and purple ambient accents. Urbanist Latin and Latin Extended fonts are bundled under `web/static/fonts/`, with the OFL license beside them. Other scripts use the fallback font stack. The official white and black SVG wordmarks are served from `web/static/` and selected by theme. Tailwind v4 Vite scans the Svelte sources automatically, so no `@source` globs are needed.

The theme is stored in a `theme` cookie (`light` / `dark`) and applied **before first paint** by a small inline script in `app.html`: it reads the cookie and sets `document.documentElement.dataset.theme`, using `dark` when there is no cookie. Doing it there — before CSS resolves — avoids a flash of the wrong theme. The toggle in `+layout.svelte` writes the same cookie, flips the attribute, and updates the browser theme color.

**Hard rules (unchanged from the previous UI):**

- daisyUI semantic component classes (`btn`, `card`, `alert`, `badge`, `input`, `select`, `tabs`, `dropdown`, `toast`, …) plus token utilities (`bg-base-100`, `text-base-content/60`, `border-base-300`, `text-error`, …) and plain Tailwind for layout. No bespoke `.brand-mark` / `.tagline` classes — if a treatment isn't covered by daisyUI + Tailwind, drop the treatment.
- Override daisyUI focus/borders in `@layer utilities` **unlayered** (`@layer utilities { … }` with no nested sub-layer name). daisyUI emits its components inside `@layer utilities { @layer daisyui.l1.l2.l3 { … } }`, so anything in `@layer components` loses regardless of specificity; per the Cascade Layers spec, unlayered content in a layer comes after its sub-layers, which is the slot we need.
- **Mobile-first.** Target ~360 px first and use `sm:` to enhance. Touch targets ≥44 px. `dvh`/`dvw`, never `vh`/`vw`. Stack via a parent `gap`, not child `margin-top`.
- `form-control` and `label-text-alt` do **not** exist in daisyUI 5. The house pattern for a labelled control is a `flex flex-col gap-1` label with the help `<span>` after the input.

### Shared UI components

Where daisyUI has no component for a pattern the mockup uses, `web/src/lib/components/ui/` has one, built from daisyUI classes and Tailwind utilities only. Their state-to-class mapping lives in `lib/ui-variants.ts` (unit-tested), so "selected" looks the same on every one of them. Reach for these before composing the classes by hand.

| Component | Use it for | Not for |
|---|---|---|
| `Modal` | Any centred dialog: bindable `open`, title, optional description, `size` (`sm`–`xl`), body, optional `footer` snippet. Escape, backdrop and the ✕ all close it; nothing renders while closed. `EditModal` is `Modal` plus the admin rows' Cancel/Save footers. | Full-screen pickers (`CapabilityPicker`) and the feedback sheet, which are bottom sheets on mobile. |
| `ChoiceCard` | A single pick between options that each need a sentence ("on the website" / "by e-mail"). Several in a `role="radiogroup"` grid; with `multiple` a switch of a multi-pick set (`role="checkbox"` in a `role="group"`, the agent setup's abilities). | Two or three short words — use `SegmentedControl`. |
| `SegmentedControl` | A small closed enum (a handful of options) shown at once, as a daisyUI `join` of radio buttons — a value to pick, such as the agent analytics' chart metric. | Long or data-driven lists — `SearchableSelect`. Navigation between views — daisyUI `tabs` (`role="tablist"`), as the agent workbench does. |
| `ChipToggle` | Multi-pick from a short visible set (channels, tags); with `onremove` a removable token. | A grant field over a closed server-known set — `SearchableSelect multiple`. |
| `AiSuggestion` | Anything the model proposed that the user has not accepted yet; the dashed primary edge is reserved for this. `actions` holds Apply / Dismiss. | Settled configuration or help text — `alert alert-info`. |
| `StatusPill` | A state word next to a name: `ok` / `warn` / `bad` / `info` / `neutral` as a soft, round daisyUI badge ("Live v3", "Needs setup"). | Counts and kinds (`badge badge-outline`). |
| `StepIndicator` | Where the user is in a multi-step flow; pills are current (primary), done (green) or upcoming; `onselect` makes them jump. | Page sections — `SectionTabs` / daisyUI `tabs`. |

## PWA and Web Push

The app is an installable PWA. Both halves ship with the SPA in `web/static/` and are served from the root by the same handler as everything else:

| File | Role |
|---|---|
| `manifest.webmanifest` | Name, colours, `display: standalone`, icons. |
| `sw.js` | Service worker. Registered by `+layout.svelte` on mount. |
| `icons/*.png`, `favicon.svg` | Installed-app icons. |

The service worker is served **verbatim** — it is not a Vite entry point, so it gets no bundling or type-checking. Keep it hand-valid browser JS. Because the SPA's assets are content-hashed and carry `immutable` server cache headers, the worker carries **no fetch cache at all**: installability and push are its whole job, and every request passes straight through to the network.

Turn-complete **Web Push** rides on top. `sw.js` carries the `push` and `notificationclick` handlers, `lib/push.svelte.ts` drives the opt-in and subscription through `/api/v0/push/{config,subscribe,unsubscribe}`, and the server half is [`aiplane_features::server::push`](../crates/aiplane-features/src/server/push/) (self-generated VAPID keypair, RFC 8291 payload encryption) fired from the assistant worker. `push::send_to_user` is the one fan-out over a user's subscriptions — the finished turn, the agent inbox and the `notify_user` tool all use it: the message is written in each subscription's language, title and body are cut to `MAX_TITLE_CHARS` (80) / `MAX_BODY_CHARS` (300), and a subscription the push service reports gone is pruned. Whether a notification is actually *shown* is decided in the worker via `clients.matchAll` — suppressed when a focused tab already has that conversation open. See the README's *Notifications* section for the operator-facing view.

Installability requires HTTPS (localhost exempt for dev), and on iOS Web Push needs the PWA installed to the home screen (16.4+).

## Voice

Distinct from the composer's dictation button (transcript into the textarea), voice mode is a hands-free spoken conversation, orchestrated client-side in `lib/voice.svelte.ts` over the ordinary chat machinery — no extra server worker.

The turn pipeline is **half-duplex, push-to-talk**:

1. Tap to record via `lib/voice-recorder.ts` (PCM capture through the `static/pcm-recorder.js` AudioWorklet, by `web/shared/voice-recorder.ts`, which the embed widget records with too). Tap again to stop.
2. The WAV posts to `POST /api/v0/transcriptions`; the transcript is submitted as an ordinary chat turn with the `voice` flag set, so the server injects the voice directive (short spoken replies, no tool-use narration — see [`gateway-api.md`](gateway-api.md)).
3. As the reply streams in, sentences are peeled off and posted to `POST /api/v0/speech`, played in order. While the assistant speaks the mic stays inert, so there is no echo loop.

Everything persists as normal chat turns, so the conversation stays readable and continuable in text. The feature only appears when a `speech` upstream pool **and** a transcription model are both available.

Both voice endpoints check the user's limits as a chat submit does and refuse with the same `429 rate_limited` envelope and `Retry-After`; a sentence served from the speech cache is not refused by limits, though it is by access. These calls are multipart uploads and audio answers, so they cannot go through `request<T>()`; they turn a refused response into the same `ApiError` with `responseError()`. `lib/voice-refusal.ts` then words it: the gateway's limit codes (`rate_limited`, `rate_limit_exceeded`) become the catalog's `voice-limit-reached` in the voice modal, the dictation button and the feedback voice note. The status alone decides nothing, because a `429` from an upstream or a proxy says nothing about the user's limits. Any other refusal shows the envelope's own message, or `error-request-failed` with the status when the body carries none (`refusalSentence()` in `lib/api.ts`, which the feedback dialog's other calls use too). A limit refusal during read-aloud drops the rest of that reply's sentences instead of asking for each; the reply stays on screen.

## i18n

The Fluent catalogs under `crates/session-core/locales/<lang>/*.ftl` are the
single string source for both server responses and the SPA. `mise run
gen-locales` compiles them into `web/src/lib/locales/*.ts`; Svelte components
use `t()`, `dt()`, and `n()` from `i18n.svelte.ts`. Never put a user-visible
literal in a component.

`crates/session-core/build.rs` fails when the six catalog key sets or variables
drift, and the web locale test verifies the generated catalogs against English.
After changing any Fluent file, add the translation to all six languages and
run `mise run gen-locales` before checking or building the SPA. Non-English
files carry a `# STATUS: llm-generated, unreviewed` banner; that is a
content-quality caveat, not permission to omit a language.

## Development loop

One terminal:

```bash
mise run dev       # public Vite/HMR on :8080, private Rust gateway on :8081
```

Open `http://localhost:8080`. Vite proxies the complete dynamic surface to :8081 (`web/vite.config.ts`), including attachments and OAuth callback routes, so there is only one browser origin. A Svelte save re-renders in well under a second with **no Rust rebuild**.

`mise run dev-served` builds `target/frontend/build` and serves the compiled SPA directly from AIplane. Use that to check the production-shaped artifact, cache headers and history fallback.

| Task | What it does |
|---|---|
| `mise run web-install` | `npm ci` in `web/`. Re-runs only when `package.json`/lock change. |
| `mise run dev` | Complete HMR stack on :8080; Vite proxies to the private gateway. |
| `mise run dev-served` | Compiled SPA served directly by AIplane, without HMR. |
| `mise run build-web` | `vite build` → `target/frontend/build/` (what the Dockerfile COPYs). |
| `mise run check-web` | `svelte-check` — TypeScript, a11y and Svelte diagnostics. |
| `mise run test-web` | `node --test` over `web/src/lib/**/*.test.ts` (the pure, framework-free halves). |

`lint`, `verify` and `ci` all depend on the relevant ones, so a fresh checkout needs no manual step.

### Browser debugging — `mise run dev-ui`

Every authed surface is gated by OIDC, which makes ad-hoc browser debugging annoying. **Don't fabricate a hand-rolled `test.html`** — it won't boot the real bundle and will miss real bugs.

```bash
AIPLANE_STATIC_DIR=target/frontend/build mise run dev-ui
```

Boots the full rama gateway on `127.0.0.1:8080` against an in-memory SQLite, wiremock chat + transcription backends, and a pre-seeded admin session with demo data. It prints the signed cookie on startup; paste it via `document.cookie` after a `goto`, then drive any page with Playwright. Full recipe in [`dev-workflow.md`](dev-workflow.md#debugging-the-ui).

For the *real* dev gateway (your own `gateway.sqlite`), the debug-only `GET /__dev/session` signs you in as the fixture user without touching anything.

### Browser tests

`e2e/spa*.test.mjs` drive the SPA with Playwright against a running `mise run dev` (`mise run e2e`): shell boot, the signed-out redirect into OIDC, the signed-in identity render, the tokens and admin surfaces, and a full chat turn streaming in over the event protocol. See [`testing.md`](testing.md) and `e2e/README.md`.
