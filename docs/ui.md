# Web UI

The UI is a **SvelteKit SPA** (Svelte 5 runes, `adapter-static`) that lives in `web/`, is compiled ahead of time by Vite into plain static files, and is served **from the root `/`** by the Rust binary. It talks to the gateway exclusively over the session-authenticated JSON API at `/api/v0/*`, and streams chat over a JSON-SSE event protocol. Styling is [daisyUI v5](https://daisyui.com/) on Tailwind v4.

There is **no Node at runtime**: still one container, one process, one port. The container image contains the gateway binary plus the built `build/` directory; `GATEWAY_STATIC_DIR` points the binary at it.

The whole stack:

| Layer | Tech | Lives in |
|---|---|---|
| HTTP server / router | rama 0.3 | `crates/gateway/src/rama_server/router.rs` |
| Static SPA hosting | hand-rolled rama handler over `tokio::fs` | `crates/gateway/src/rama_server/spa.rs` |
| UI framework | SvelteKit 2 / Svelte 5 (runes), Vite | `web/` |
| API contract | OpenAPI 3 → generated TS types | `docs/openapi.json` → `web/src/lib/schema.d.ts` |
| API client | one `fetch` helper + typed wrappers | `web/src/lib/api.ts` (and `client.ts`, see below) |
| Chat streaming | JSON events over SSE | `crates/session-core/src/chat_json.rs` ↔ `web/src/lib/chat-protocol.ts` |
| Styling | Tailwind v4 + daisyUI v5 | `web/src/app.css` |
| Markdown rendering | `marked` + `dompurify`, client-side | `web/src/lib/markdown.ts` |

## How it is served

`crates/gateway/src/rama_server/spa.rs` serves the build directory named by the `GATEWAY_STATIC_DIR` environment variable. It is deliberately hand-rolled rather than a `tower-http::ServeDir`, because the dependency policy keeps the server stack rama-only (see [`dependencies.md`](dependencies.md)); the logic is small and unit-tested end to end.

What it does, and why:

- **Mounted at the root, registered last.** `GET /` and `GET /{*name}` are the final two routes in `router.rs`. rama matches in registration order, so a catch-all registered any earlier would swallow the API, proxy and auth routes. Anything added after them would be unreachable.
- **History fallback.** A client route like `/tokens` has no file on disk. Anything that is not an existing file falls back to `index.html` so the client router can resolve it. This is the standard contract an SPA needs from a static host.
- **Cache policy by kind.** SvelteKit emits content-hashed asset filenames, whose bytes never change at a given URL — those get `public, max-age=31536000, immutable`. `index.html` and `sw.js` get `no-cache`, or an update would never reach a browser; the manifest gets a short revalidating max-age.
- **Traversal guard.** The relative path is normalised *on its own* before being joined to the root, so a leading `..` with nothing to consume is refused. Normalising after the join would let the root's own components absorb the `..` — safe, but it would silently turn the guard into a no-op.
- **Case is preserved.** rama lowercases only the *matched* path for route lookup; the `Request` handed to the handler keeps the original URI, so `req.uri().path()` still carries the case of a content-hashed filename. (Same precedent as `retrieve_model` — see [the note in `router.rs`](../crates/gateway/src/rama_server/router.rs).)
- **503, not 404, when undeployed.** No `GATEWAY_STATIC_DIR` (or a missing directory) answers `503` with a message naming the variable. That is deliberately distinct from the router's 404 so an operator can tell "the SPA build was never copied in" from "that path does not exist". The rest of the gateway is unaffected.

`crates/gateway/tests/it/spa_routes.rs` pins the *wiring* (the catch-all really reaches this handler, proven by the 503); `spa.rs`'s own unit tests pin the serve behaviour against a temp directory.

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
│   │   ├── schema.d.ts        GENERATED from docs/openapi.json — do not hand-edit
│   │   ├── client.ts          openapi-fetch client over schema.d.ts (not wired up yet)
│   │   ├── api.ts             the fetch helper + typed wrappers + ApiError
│   │   ├── admin-client.ts    same transport for the admin surfaces
│   │   ├── chat-protocol.ts   the SSE event fold — framework-free, unit-tested
│   │   ├── chat.svelte.ts     reactive conversation controller (EventSource lifecycle)
│   │   ├── session.svelte.ts  identity from GET /api/v0/me
│   │   ├── sidebar.svelte.ts  conversation list + search + mobile drawer
│   │   ├── feedback.svelte.ts feedback widget state
│   │   ├── push.svelte.ts     Web Push opt-in (device-local state)
│   │   ├── voice.svelte.ts    voice-conversation orchestration
│   │   ├── voice-recorder.ts  PCM capture + analyser
│   │   ├── markdown.ts        marked → DOMPurify → {@html}
│   │   └── usage-types.ts
│   └── routes/
│       ├── +layout.svelte     app shell: nav, sidebar, theme, sign-out, feedback
│       ├── +layout.ts         prerender = false, ssr = false
│       ├── +page.svelte       dashboard
│       ├── login/ chat/ chat/[id]/ tokens/ tools/ memory/ scheduled/
│       ├── webhooks/ skills/ integrations/ usage/ setup/
│       └── admin/             +layout.svelte + models, upstreams, users, groups,
│                              tokens, limits, settings, skills, connectors,
│                              comfyui, rag
└── static/               copied verbatim into the build output
    ├── manifest.webmanifest, sw.js, robots.txt
    ├── favicon.svg, icons/*.png
    └── pcm-recorder.js   AudioWorklet processor (its own JS realm — not bundled)
```

`+layout.ts` sets `prerender = false` and `ssr = false`. Prerendering would bake the anonymous shell into every route, and this is a private surface: the identity render would flash "signed out" on first paint anyway, and no per-route HTML should be emitted for an authed page.

## The JSON API and the generated client

Every dynamic thing the SPA does is a `/api/v0/*` call — about 140 operations across ~115 paths. The contract is **`docs/openapi.json`**, hand-maintained (the wire types in `shared::api` use `jiff::Timestamp` and have no `schemars` derive, so code-first annotations would be re-annotated constantly) but **enforced**: `crates/gateway/tests/it/openapi_drift.rs` fails CI when the spec and the routes registered in `router.rs` drift in either direction — an undocumented new endpoint, or a spec entry no route serves.

The SPA's types come from that spec:

```bash
mise run gen-api-client     # docs/openapi.json → web/src/lib/schema.d.ts
```

Run it after changing any `/api/v0` route. `schema.d.ts` is generated output — edit the spec, not the file.

**How calls are actually made today.** Every request goes through one helper, `request<T>()` in `lib/api.ts`: a same-origin `fetch` that sends the session cookie, parses the error envelope, and throws an `ApiError` carrying the status and the server's message. `lib/api.ts` then exposes the `api.*` wrappers routes call, with their response shapes declared by hand against `shared::api`. `lib/admin-client.ts` is the same transport for the admin views, flattening the envelope into a plain `Error`.

`lib/client.ts` builds an `openapi-fetch` client over the generated `schema.d.ts` types (`baseUrl: '/api/v0'`, `credentials: 'same-origin'`), which is the intended end state — the compiler would then check every call against the spec. **Nothing imports it yet**, so `schema.d.ts` is currently generated and not consumed: the spec is enforced against the *router* by `openapi_drift`, not against the client. Migrating a hand-declared shape in `api.ts` onto the generated one is a safe, incremental improvement; adding a new hand-declared shape moves in the wrong direction.

A 401 means "signed out": `+layout.svelte` turns "`me` is null after load" into a redirect to `/auth/login` carrying the route the user actually wanted, and never bounces `/setup` (which runs before any account exists).

### Routes that are not `/api/v0`

Six routes outlived the server-rendered pages because they are not a UI:

| Route | Why it is not under `/api/v0` |
|---|---|
| `POST /hooks/{secret}`, `POST /hooks/rag/{token}` | Public triggers. The URL *is* the credential; a third party (a file host's webhook, a cron line) calls them. |
| `GET /rag/{id}/connect`, `GET /rag/oauth/callback` | RAG source OAuth round trip. The redirect URI is registered with an external provider, so the path is not ours to change. |
| `POST /integrations/{key}/connect`, `POST /integrations/{key}/retry`, `GET /integrations/callback` | Per-user MCP connector OAuth, same shape. |

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

The rest of the conversation surface is ordinary JSON: `POST …/messages` submits (multipart when there are attachments), `POST …/cancel` flips the worker's cancel flag, `…/fork`, `…/share`, `…/effort`, `…/documents/*` (canvas), `…/export.md` and `…/export.pdf`, `…/turns/{turn_id}/{retry,edit}`.

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

Turn-complete **Web Push** rides on top. `sw.js` carries the `push` and `notificationclick` handlers, `lib/push.svelte.ts` drives the opt-in and subscription through `/api/v0/push/{config,subscribe,unsubscribe}`, and the server half is [`gateway_features::server::push`](../crates/gateway-features/src/server/push/) (self-generated VAPID keypair, RFC 8291 payload encryption) fired from the assistant worker. Whether a notification is actually *shown* is decided in the worker via `clients.matchAll` — suppressed when a focused tab already has that conversation open. See the README's *Notifications* section for the operator-facing view.

Installability requires HTTPS (localhost exempt for dev), and on iOS Web Push needs the PWA installed to the home screen (16.4+).

## Voice

Distinct from the composer's dictation button (transcript into the textarea), voice mode is a hands-free spoken conversation, orchestrated client-side in `lib/voice.svelte.ts` over the ordinary chat machinery — no extra server worker.

The turn pipeline is **half-duplex, push-to-talk**:

1. Tap to record via `lib/voice-recorder.ts` (PCM capture through the `static/pcm-recorder.js` AudioWorklet, plus an `AnalyserNode` tap for the visualiser). Tap again to stop.
2. The WAV posts to `POST /api/v0/transcriptions`; the transcript is submitted as an ordinary chat turn with the `voice` flag set, so the server injects the voice directive (short spoken replies, no tool-use narration — see [`gateway-api.md`](gateway-api.md)).
3. As the reply streams in, sentences are peeled off and posted to `POST /api/v0/speech`, played in order. While the assistant speaks the mic stays inert, so there is no echo loop.

Everything persists as normal chat turns, so the conversation stays readable and continuable in text. The feature only appears when a `speech` upstream pool **and** a transcription model are both available.

## i18n — what still applies

The SPA's own strings are currently plain English in the Svelte components; there is no client-side translation layer yet.

The Fluent gate still applies to the **server-side strings** in `crates/session-core/locales/`, which back what the Rust side still writes in a human language: proxy and API error envelopes, MCP connector status text, the OAuth round-trip responses, and the feedback surface. `crates/session-core/build.rs` fails the build if any key present in `locales/en/*.ftl` is missing from `de`/`fr`/`es`/`ru`/`zh`, or vice versa — `cargo build`/`check`/`test` all refuse to compile the crate graph until every language has every key. That is deliberate: a partially translated UI reads as a broken product to a non-English user, so a missing translation is a build error rather than a review comment.

Adding a server-side string is therefore: add the key to `locales/en/<module>.ftl` (naming convention `<module>-<slug>`), add the same key translated to the other five, then call `t(lang, "key")` / `t_args(...)`. Non-English files are LLM-generated and carry a `# STATUS: llm-generated, unreviewed` banner — a content-quality caveat, not a licence to skip a language.

Note that the `.ftl` files still carry a large set of keys that belonged to the deleted pages (`tokens`, `settings`, `upstreams`, …). They are dead weight until either the SPA grows an i18n layer that reuses them or someone prunes them; the build gate only checks that the six locales agree, not that a key is reachable.

## Development loop

Two terminals:

```bash
mise run dev       # the Rust gateway (API + proxy) on :8080
mise run dev-web   # Vite dev server on :5173, HMR on every save
```

Open `http://localhost:5173`. Vite proxies `/api`, `/v1` and `/auth` to :8080 (`web/vite.config.ts`), so the session cookie and every backend call behave exactly as in production — while a Svelte save re-renders in well under a second with **no Rust rebuild**. That is the point of the SPA: UI iteration no longer pays the Rust compile.

`mise run dev` also sets `GATEWAY_STATIC_DIR=target/frontend/build` (built by its `build-web` dependency), so `http://localhost:8080` serves the *compiled* SPA. Use that to check the built artefact, cache headers and the history fallback.

| Task | What it does |
|---|---|
| `mise run web-install` | `npm ci` in `web/`. Re-runs only when `package.json`/lock change. |
| `mise run dev-web` | Vite dev server on :5173 with the API proxy. |
| `mise run build-web` | `vite build` → `target/frontend/build/` (what the Dockerfile COPYs). |
| `mise run check-web` | `svelte-check` — TypeScript, a11y and Svelte diagnostics. |
| `mise run test-web` | `node --test` over `web/src/lib/**/*.test.ts` (the pure, framework-free halves). |
| `mise run gen-api-client` | Regenerate `schema.d.ts` from `docs/openapi.json`. |

`lint`, `verify` and `ci` all depend on the relevant ones, so a fresh checkout needs no manual step.

### Browser debugging — `mise run dev-ui`

Every authed surface is gated by OIDC, which makes ad-hoc browser debugging annoying. **Don't fabricate a hand-rolled `test.html`** — it won't boot the real bundle and will miss real bugs.

```bash
GATEWAY_STATIC_DIR=target/frontend/build mise run dev-ui
```

Boots the full rama gateway on `127.0.0.1:8080` against an in-memory SQLite, wiremock chat + transcription backends, and a pre-seeded admin session with demo data. It prints the signed cookie on startup; paste it via `document.cookie` after a `goto`, then drive any page with Playwright. Full recipe in [`dev-workflow.md`](dev-workflow.md#debugging-the-ui).

For the *real* dev gateway (your own `gateway.sqlite`), the debug-only `GET /__dev/session` signs you in as the fixture user without touching anything.

### Browser tests

`e2e/spa*.test.mjs` drive the SPA with Playwright against a running `mise run dev` (`mise run e2e`): shell boot, the signed-out redirect into OIDC, the signed-in identity render, the tokens and admin surfaces, and a full chat turn streaming in over the event protocol. See [`testing.md`](testing.md) and `e2e/README.md`.
