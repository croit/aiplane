# Web UI

The UI is a **SvelteKit SPA** (Svelte 5 runes, `adapter-static`) that lives in `web/`, is compiled ahead of time by Vite into plain static files, and is served **from the root `/`** by the Rust binary. It talks to AIplane exclusively over the session-authenticated JSON API at `/api/v0/*`, and streams chat over a JSON-SSE event protocol. Styling is [daisyUI v5](https://daisyui.com/) on Tailwind v4.

There is **no Node at runtime**: still one container, one process, one port. The container image contains the gateway binary plus the built `build/` directory; `AIPLANE_STATIC_DIR` points the binary at it.

The whole stack:

| Layer | Tech | Lives in |
|---|---|---|
| HTTP server / router | rama 0.3 | `crates/aiplane/src/rama_server/router.rs` |
| Static SPA hosting | hand-rolled rama handler over `tokio::fs` | `crates/aiplane/src/rama_server/spa.rs` |
| UI framework | SvelteKit 2 / Svelte 5 (runes), Vite | `web/` |
| API contract | OpenAPI 3.1, generated from backend route declarations | `GET /openapi.json` |
| API client | one `fetch` helper + hand-declared shapes | `web/src/lib/api.ts` |
| Chat streaming | JSON events over SSE | `crates/session-core/src/chat_json.rs` ↔ `web/src/lib/chat-protocol.ts` |
| Styling | Tailwind v4 + daisyUI v5 | `web/src/app.css` |
| Markdown rendering | `marked` + `dompurify`, client-side | `web/src/lib/markdown.ts` |

## How it is served

`crates/aiplane/src/rama_server/spa.rs` serves the build directory named by the `AIPLANE_STATIC_DIR` environment variable. It is deliberately hand-rolled rather than a `tower-http::ServeDir`, because the dependency policy keeps the server stack rama-only (see [`dependencies.md`](dependencies.md)); the logic is small and unit-tested end to end.

What it does, and why:

- **Mounted at the root, registered last.** `GET /` and `GET /{*name}` are the final two routes in `router.rs`. rama matches in registration order, so a catch-all registered any earlier would swallow the API, proxy and auth routes. Anything added after them would be unreachable.
- **History fallback.** A client route like `/tokens` has no file on disk. Anything that is not an existing file falls back to `index.html` so the client router can resolve it. This is the standard contract an SPA needs from a static host.
- **Cache policy by kind.** SvelteKit emits content-hashed asset filenames, whose bytes never change at a given URL — those get `public, max-age=31536000, immutable`. `index.html` and `sw.js` get `no-cache`, or an update would never reach a browser; the manifest gets a short revalidating max-age.
- **Traversal guard.** The relative path is normalised *on its own* before being joined to the root, so a leading `..` with nothing to consume is refused. Normalising after the join would let the root's own components absorb the `..` — safe, but it would silently turn the guard into a no-op.
- **Case is preserved.** rama lowercases only the *matched* path for route lookup; the `Request` handed to the handler keeps the original URI, so `req.uri().path()` still carries the case of a content-hashed filename. (Same precedent as `retrieve_model` — see [the note in `router.rs`](../crates/aiplane/src/rama_server/router.rs).)
- **503, not 404, when undeployed.** No `AIPLANE_STATIC_DIR` (or a missing directory) answers `503` with a message naming the variable. That is deliberately distinct from the router's 404 so an operator can tell "the SPA build was never copied in" from "that path does not exist". The rest of AIplane is unaffected.

`crates/aiplane/tests/it/spa_routes.rs` pins the *wiring* (the catch-all really reaches this handler, proven by the 503); `spa.rs`'s own unit tests pin the serve behaviour against a temp directory.

### The first-run gate

`rama_server::first_run` redirects the whole surface to the setup wizard until setup completes. Because the wizard *is* the SPA now, `serves_before_setup` allowlists the SPA's static shell — `/_app/*`, `/assets/*`, `/icons/*`, `favicon.*`, `manifest.webmanifest`, `sw.js`, `robots.txt`, `pcm-recorder.js` — plus `/setup*` and `/auth/callback`. Gating a JavaScript module request would 303 it to an HTML page and leave the wizard blank. Those files are unauthenticated static bytes; the SPA's own API calls self-protect with 401/403.

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
│   │   ├── voice-recorder.ts  PCM capture + analyser
│   │   ├── markdown.ts        marked → DOMPurify → {@html}
│   │   └── usage-types.ts
│   └── routes/
│       ├── +layout.svelte     app shell: nav, sidebar, theme, sign-out, feedback
│       │                      (the feedback FAB + dialog mount HERE so they
│       │                       survive client-side navigation)
│       ├── +layout.ts         prerender = false, ssr = false
│       ├── +page.svelte       dashboard
│       ├── login/ chat/ chat/[id]/ tokens/ tools/ memory/ scheduled/
│       ├── webhooks/ skills/ integrations/ usage/ setup/ rag/ rag/profiles/
│       └── admin/             +layout.svelte + models, upstreams, users, groups,
│                              tokens, limits, settings, skills, connectors,
│                              comfyui
└── static/               copied verbatim into the build output
    ├── manifest.webmanifest, sw.js, robots.txt
    ├── favicon.svg, icons/*.png
    └── pcm-recorder.js   AudioWorklet processor (its own JS realm — not bundled)
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

Long or data-driven choices use the shared `SearchableSelect` combobox instead
of a native select. It searches labels, stored values, descriptions, keywords and
badges; supports arrow keys, Enter and Escape; and bounds long result lists to
their own scroll region. Model choices render the model id left-aligned and show
compact GDPR and NDA status badges right-aligned on every row. Green check badges
mean the model is cleared; red crossed-out badges mean it is restricted. The
search field alone carries the accent focus ring; triggers use neutral borders,
and keyboard-active options use a subtle surface change. Small
closed enums such as hour/day/week or on/off remain native selects because a
search field would add friction without improving discovery.

The root layout owns the document title through the route registry in
`page-titles.ts`; data-driven pages publish their resolved name through the
shared override in `page-title.ts`. Conversation metadata is refreshed
after both sidebar changes and final turn events, so an asynchronous generated
title cannot update the sidebar while leaving the header or browser history
stale.

## The JSON API and its contract

Every dynamic thing the SPA does is a `/api/v0/*` call — about 140 operations
across more than 100 paths. `GET /openapi.json` generates an OpenAPI 3.1
document from the route declarations compiled into AIplane. There is no
detached contract file to copy into the container or synchronize after a route
change. Path parameters and request methods are inferred from the declarations;
explicit backend wire types remain the authority for request and response
fields.

**How calls are actually made today.** Every request goes through one helper, `request<T>()` in `lib/api.ts`: a same-origin `fetch` that sends the session cookie, parses the error envelope, and throws an `ApiError` carrying the status and the server's message. `lib/api.ts` then exposes the `api.*` wrappers routes call, with their response shapes declared by hand against `shared::api`. `lib/admin-client.ts` is the same transport for the admin views, flattening the envelope into a plain `Error`.

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

Six routes outlived the server-rendered pages because they are not a UI:

| Route | Why it is not under `/api/v0` |
|---|---|
| `POST /hooks/{secret}`, `POST /hooks/rag/{token}` | Public triggers. The URL *is* the credential; a third party (a file host's webhook, a cron line) calls them. |
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
| `turn_finalized` | `turn_id`, `status`, `error_message?`, `model?`, `duration_ms` | Terminal: no further deltas for this turn. |
| `steer` | `turn_id`, `id`, `text`, `status` | A mid-turn interjection appeared or reached its outcome. One event for both, merged on `id`, so a client attaching late ends up in the same state as one that watched from the start. |
| `sidebar_changed` | — | Session metadata changed (title generated, pin toggled). Deliberately payload-free: the list endpoint is the source of truth, so the client refetches. |
| `info` | `message` | Transient notice, rendered as a dismissible banner (e.g. a vision fallback). |
| `tool_prompt` | a `ToolPromptEvent` | Human-in-the-loop prompt — `ask_user`, a location request, or a tool confirmation — plus its `Hide` counterpart when it is answered, times out, or the turn ends. |
| `idle` | — | No live worker for this session; nothing more will arrive. |

Invariants worth knowing before you touch either side:

- **The DB is the replayer.** There is no `Last-Event-ID` replay. A client that reconnects simply re-attaches, and the first `snapshot` — rebuilt from SQLite — subsumes anything missed. The worker runs to completion independently of any HTTP listener and writes its progress to the DB as it goes, so closing a tab mid-stream loses nothing.
- **Deltas append, unless `full: true`.** `full` marks a cursor reset: the row was rewritten and `text_delta` carries the *whole* text, so the client must replace its buffer rather than append. A client cannot detect a rewrite on its own, which is why the server says so. There are no delete events by design.
- **Flushes are coalesced** to ≥120 ms per subscriber with a trailing flush, so the final state always lands. Each flush reads one turn, not the conversation.
- **The stream ends at `turn_finalized` / `idle`.** The server closes there, so `chat.svelte.ts` closes the `EventSource` too — letting it auto-reconnect would loop snapshot/idle forever on a quiet session. After a submit (or any suspected change) `attach()` reopens, and the fresh snapshot is the replay.
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

## Reactive state

Shared state lives in `.svelte.ts` modules exporting `$state` objects, built as factories rather than classes — `$state` in a module closure is the documented universal-reactivity pattern, and the returned object's methods close over it directly.

- `session.svelte.ts` — `me`, loaded once per app start; `null` while unknown *and* on 401, with a `loaded` flag so the layout can tell the two apart.
- `sidebar.svelte.ts` — the conversation list, its search box, and the mobile drawer. Pages call `refresh()` after anything that changes a conversation so the list never drifts.
- `chat.svelte.ts` — one controller per conversation view; owns the `ConversationState` plus the `EventSource` lifecycle.
- `push.svelte.ts` — Web Push opt-in. This state is **device-local**, not server state: two browsers of the same user subscribe independently.

## Theming

`web/src/app.css` registers two daisyUI themes named `light` and `dark` (a shadcn-flavoured neutral palette: the primary action is near-black in light, near-white in dark; only info/success/warning/error carry hue) with daisyUI's built-in palettes switched off. Unlike a Rust-templated UI it needs no `@source` globs — the Tailwind v4 Vite plugin scans the Svelte sources itself.

The theme is stored in a `theme` cookie (`light` / `dark`) and applied **before first paint** by a small inline script in `app.html`: it reads the cookie and sets `document.documentElement.dataset.theme`, falling back to `prefers-color-scheme` when there is no cookie. Doing it there — before CSS resolves — is what avoids a flash of the wrong theme. The toggle in `+layout.svelte` writes the same cookie and flips the attribute.

**Hard rules (unchanged from the previous UI):**

- daisyUI semantic component classes (`btn`, `card`, `alert`, `badge`, `input`, `select`, `tabs`, `dropdown`, `toast`, …) plus token utilities (`bg-base-100`, `text-base-content/60`, `border-base-300`, `text-error`, …) and plain Tailwind for layout. No bespoke `.brand-mark` / `.tagline` classes — if a treatment isn't covered by daisyUI + Tailwind, drop the treatment.
- Override daisyUI focus/borders in `@layer utilities` **unlayered** (`@layer utilities { … }` with no nested sub-layer name). daisyUI emits its components inside `@layer utilities { @layer daisyui.l1.l2.l3 { … } }`, so anything in `@layer components` loses regardless of specificity; per the Cascade Layers spec, unlayered content in a layer comes after its sub-layers, which is the slot we need.
- **Mobile-first.** Target ~360 px first and use `sm:` to enhance. Touch targets ≥44 px. `dvh`/`dvw`, never `vh`/`vw`. Stack via a parent `gap`, not child `margin-top`.
- `form-control` and `label-text-alt` do **not** exist in daisyUI 5. The house pattern for a labelled control is a `flex flex-col gap-1` label with the help `<span>` after the input.

## PWA and Web Push

The app is an installable PWA. Both halves ship with the SPA in `web/static/` and are served from the root by the same handler as everything else:

| File | Role |
|---|---|
| `manifest.webmanifest` | Name, colours, `display: standalone`, icons. |
| `sw.js` | Service worker. Registered by `+layout.svelte` on mount. |
| `icons/*.png`, `favicon.svg` | Installed-app icons. |

The service worker is served **verbatim** — it is not a Vite entry point, so it gets no bundling or type-checking. Keep it hand-valid browser JS. Because the SPA's assets are content-hashed and carry `immutable` server cache headers, the worker carries **no fetch cache at all**: installability and push are its whole job, and every request passes straight through to the network.

Turn-complete **Web Push** rides on top. `sw.js` carries the `push` and `notificationclick` handlers, `lib/push.svelte.ts` drives the opt-in and subscription through `/api/v0/push/{config,subscribe,unsubscribe}`, and the server half is [`aiplane_features::server::push`](../crates/aiplane-features/src/server/push/) (self-generated VAPID keypair, RFC 8291 payload encryption) fired from the assistant worker. Whether a notification is actually *shown* is decided in the worker via `clients.matchAll` — suppressed when a focused tab already has that conversation open. See the README's *Notifications* section for the operator-facing view.

Installability requires HTTPS (localhost exempt for dev), and on iOS Web Push needs the PWA installed to the home screen (16.4+).

## Voice

Distinct from the composer's dictation button (transcript into the textarea), voice mode is a hands-free spoken conversation, orchestrated client-side in `lib/voice.svelte.ts` over the ordinary chat machinery — no extra server worker.

The turn pipeline is **half-duplex, push-to-talk**:

1. Tap to record via `lib/voice-recorder.ts` (PCM capture through the `static/pcm-recorder.js` AudioWorklet, plus an `AnalyserNode` tap for the visualiser). Tap again to stop.
2. The WAV posts to `POST /api/v0/transcriptions`; the transcript is submitted as an ordinary chat turn with the `voice` flag set, so the server injects the voice directive (short spoken replies, no tool-use narration — see [`gateway-api.md`](gateway-api.md)).
3. As the reply streams in, sentences are peeled off and posted to `POST /api/v0/speech`, played in order. While the assistant speaks the mic stays inert, so there is no echo loop.

Everything persists as normal chat turns, so the conversation stays readable and continuable in text. The feature only appears when a `speech` upstream pool **and** a transcription model are both available.

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
