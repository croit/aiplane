# Architecture

## One-paragraph summary

AIplane is a single Rust binary built on **rama 0.3**, which is a proxy-native HTTP framework. The same process serves the OpenAI-compatible API (`/v1/*`), the OIDC browser flow (`/auth/*`), the session-authed JSON API (`/api/v0/*`), and — from the root `/` — the static files of a **SvelteKit SPA** (Svelte 5, `adapter-static`, built from `web/`). The SPA is compiled ahead of time and served out of `AIPLANE_STATIC_DIR`, so there is still no Node at runtime: one container, one process, one port. Chat streams over a JSON-SSE event protocol (`session_core::chat_json`) rather than server-rendered diffs; styling is **daisyUI v5 + Tailwind v4** with a shadcn-flavoured neutral palette. See [`ui.md`](ui.md).

> Indexing an external file share (Nextcloud / WebDAV / …) into RAG is its
> own subsystem with its own extension point — see
> [`fileshare-rag.md`](fileshare-rag.md).

## Diagram

```
                                ┌──────────────────────────────────────────────┐
                                │            Gateway (Rust, rama 0.3)          │
                                │                                              │
   Browser ───── HTTPS ────────►│  ┌─────────────────────┐  ┌───────────────┐  │
                                │  │  /  SPA static      │  │  /auth/*      │──┼──► OIDC provider
                                │  │     shell (web/)    │  │  OIDC flow    │  │   (Keycloak/Authentik/…)
                                │  ├─────────────────────┤  └───────────────┘  │
                                │  │  /api/v0/*  JSON    │                     │
                                │  │  + chat SSE events  │                     │
                                │  └─────────────────────┘                     │
                                │                                              │
   OpenAI SDK ── HTTPS ────────►│  ┌────────────────────────────────────────┐  │
                                │  │  /v1/chat/completions, /v1/audio/...   │──┼──► Upstream pool A (chat)
                                │  │  [bearer auth][rbac][tool injection]   │──┼──► Upstream pool B (whisper)
                                │  │  [tool-call loop]    [model routing]   │──┼──► …
                                │  └────────────────────────────────────────┘  │
                                │                                              │
                                │  SQLite (sessions, gateway tokens, audit)    │
                                └──────────────────────────────────────────────┘
```

## Crate boundaries

AIplane is one binary assembled from a layered stack of crates under
`crates/`. The layering is load-bearing for dev-build speed, not just tidiness:
the `gateway` crate used to be ~108k lines in a single compilation unit, so
editing *any* file re-ran the whole frontend + codegen. Each crate below depends
only on the ones beneath it, so an edit recompiles that crate and what sits above
it — never what sits below.

```
gateway            bin + router/proxy/api/oidc      6.5k  ← thinnest, most-edited glue
   ├── aiplane-api     the /api/v0 JSON handlers   25.5k  ← siblings: neither
   └── aiplane-tools   the tool implementations    14.5k  ←   depends on the other
          └── aiplane-runtime  tool API + AppState/RamaState + chat driver   14.7k
                 ├── aiplane-features  RAG, skills, ComfyUI, push, geoip, …  13.9k
                 └── aiplane-core      db, config, crypto, rbac, upstreams   22.1k
                        ├── session-core   chat-UI substrate
                        └── shared         OpenAI wire types
```

What that buys, in lines that must recompile after a one-line edit:

| edit site | recompiled |
|---|---|
| pre-split monolith | **97,310** (one unit) |
| `gateway` | 6,510 |
| `aiplane-tools` | 21,041 |
| `aiplane-api` | 32,017 |
| `aiplane-runtime` | 61,266 |
| `aiplane-features` | 75,124 |
| `aiplane-core` | 97,189 |

The gains are front-loaded deliberately: the layers that churn most (handlers, tools,
glue — about 60% of file touches over six months) are the cheapest to rebuild, and
`aiplane-core` — the one that still costs a full rebuild — is the least-edited.

Those counts are the measurement that motivated the split, taken before the SPA
migration deleted the server-rendered page stack; `aiplane-api` is roughly a third
of the size quoted above now. The ordering — and therefore the rule below — is
unchanged, and UI work no longer recompiles Rust at all.

**Rule of thumb when adding code:** put it as high in the stack as it will go.
Something only belongs in `aiplane-core` if code below the feature layer genuinely
needs it. Pushing a module downward for convenience is what makes builds slow
again.

### `crates/shared`
Pure data types, no I/O:
- OpenAI request/response schema (`ChatCompletionRequest`, `ChatCompletionResponse`, streaming chunk type, tool-call types, audio transcription types).
- Tool descriptors (`ToolDef`, `ToolSchema`), role identifiers, RBAC rule types.
- Gateway error type (rendered identically by server and CLI).

Depends only on `serde`, `serde_json`, `thiserror`.

### `crates/aiplane-core`
The base layer — the things everything else stands on, and the least-edited code
in the tree. No routing, no `AppState`, no tool registry:
- `auth/oidc.rs` — hand-rolled OIDC client (discovery + JWKS-verified ID tokens, on reqwest).
- `auth/token.rs` — gateway-token mint/hash helpers.
- `config.rs` — typed `[upstream_pools]`, `[[models]]`, `[oidc]`, `[rbac]` schema.
- `db/` — sqlx; users / tokens / sessions / prefs / usage / …, plus `migrations/` at the crate root, embedded by `db/mod.rs`'s `sqlx::migrate!`.
- `crypto.rs` — AES-256-GCM at-rest sealing for DB-stored secrets.
- `rbac/` — role lookup and grant resolution. It filters grants against the tool and skill registries through the [`GrantableSet`] trait (two methods, used via generics) rather than depending on them, which is what lets RBAC sit at the bottom while the registries live two layers up.
- `upstreams/` — pool registry, backend health probes, RAII `Acquired` guard for in-flight accounting.
- `reasoning.rs`, `model_defaults.rs`, `feature_defaults.rs` — per-model capability and effort tables.
- `tool_naming.rs` — the well-known tool ids/prefixes (`comfyui_`, `typst_`, `enable_tools`, `read_skill`) and the slug→title humaniser. Down here because RBAC, the typst discovery pass, and the catalog all need it and they're on three different layers.
- `usage/`, `limits/` — the metrics sink and the rate-limit/quota enforcer.
- `rama_server/session.rs` — signed-cookie + sqlite session store, plus the `is_safe_return_to` redirect guard the OIDC callback needs to bounce a signed-in user back to the SPA route they asked for; `rama_server/cors.rs` — the CORS layer. Neither needs `AppState`, so both stay here.

### `crates/aiplane-features`
The optional subsystems — what a deployment switches on at `/admin/settings`
and can run entirely without: `rag/`, `skills.rs`, `comfyui/` (client, store, manifest,
runner, scheduler), `push/`, `github/`, `geoip/`, `typst.rs`, `image_gen.rs`,
`chat_attachments.rs`, `embeddings.rs`, `speech.rs`, `pdf.rs`, `ocr.rs`,
`search_settings.rs`, and `document_canvas.rs` (the chat canvas store, shared
by the chat document endpoints and the document tools above).

Each stands on `aiplane-core` and knows nothing about `AppState`, the tool
registry, or routing. That ignorance is the whole point — it's what lets this
layer sit below the runtime. A reference from here up into `aiplane-runtime`
collapses the split.

### `crates/aiplane-runtime`
Where the world gets tied together:
- `server/tools/` — the tool *machinery*: the `Tool` trait and `ToolContext`, the `ToolRegistry`, the round-loop `runner`, the `catalog` (tool id → group → toggle key), the MCP connection manager, and the sandbox client. Implementations live in `aiplane-tools`, above; `echo` and `get_current_timestamp` stay here as the canonical trivial tools that the registry/runner tests build registries out of.
- `server/state.rs` — `AppState`: the db pool, config, `Arc<UpstreamRegistry>`, `Arc<ToolRegistry>`, `Arc<Resolver>`, and the optional feature handles (RAG indexer, skills, ComfyUI, push, geoip, sandbox client, MCP manager).
- `rama_server/state.rs` — `RamaState` wraps `AppState` (via `Deref`) and adds the session store, worker registry, usage sink and rate-limit enforcer; `rama_server/auth.rs` — `require_bearer` for `/v1/*`.
- `openai_driver.rs` — the `session_core::SessionDriver` impl that streams a chat completion, plus `loop_guard.rs`.
- `server/{scheduled,webhooks,compaction,headless}` — the background workers that need state.
- `server/comfyui_tool.rs` — the ComfyUI `Tool`/`ToolSource` impls and the `ComfyuiHandle` that `AppState` holds. Split out of `aiplane-features`' `comfyui/` because it needs the tool API.

`aiplane-tools` and `aiplane-api` both sit on this and neither depends on the
other, so a tool edit and a page edit stay independent.

### `crates/aiplane-tools`
The tool implementations — one module per tool family (`fetch_url`,
`fetch_attachment`, `search_web`, `typst_render`, `document`, `rag`, `memory`,
`qr`, `netcheck`, …). Each holds `Tool` impls; they plug into the machinery in
`aiplane-runtime` and are registered into the `ToolRegistry` that `gateway`'s
`main.rs` builds.

A pure sink like `aiplane-api`, and a sibling of it. Two tests live in
`tests/` rather than beside their code because they span both layers — the
catalog-grouping and `AppState`-authorization tests need the machinery from
`aiplane-runtime` *and* the real concrete tools from here. A unit test inside
`aiplane-runtime` can't reach them: a `cfg(test)` build of a crate is a separate
crate instance, so its types don't unify with a dependent crate's. That same
constraint is why a handful of test-support helpers (`ToolContext::for_test`,
`pdf::test_support`, `comfyui::Client::with_http`) are plain `pub` rather than
`#[cfg(test)]`.

### `crates/aiplane-api`
The `/api/v0` JSON handlers the SPA calls — everything the deleted page stack used
to render server-side, now answering JSON instead. `pages/mod.rs` carries the
shared helpers every handler uses — `require_session_json` / `require_admin_json`
(the 401/403 gates) and `json_ok` / `json_error` (the response envelope) — and
re-exports the handlers the router mounts.
`chat/` is a directory module for the multi-conversation chat (`json_api.rs` for
the endpoints and the event stream, `title.rs` for auto-titling); `json_admin.rs`,
`json_skills.rs` and `json_workspace.rs` own the admin, skills/connector and
memory/scheduled/webhook surfaces; `rag*.rs`, `integrations.rs`, `tools.rs`,
`webhooks.rs` and `feedback.rs` own the rest, including the handful of non-`/api/v0`
OAuth and webhook-trigger routes that outlived the pages.

This crate is a **pure sink** — nothing in `aiplane-core` references it, and only
the router mounts it. Keep it that way: a back-edge from `aiplane-core` into a
handler would collapse the split. `build_info.rs` (and the `build.rs` that stamps
the git SHA into it) lives here too, because it keeps a new commit from
invalidating `aiplane-core`.

### `crates/gateway`
The binary and its routing glue — deliberately thin:
- `router.rs` — builds the `rama::http::service::web::Router`, mounting handlers from `aiplane-api` and this crate.
- `proxy.rs` — `/v1/{models,chat/completions,audio/transcriptions,audio/speech,embeddings,images/generations,images/edits}` handlers. The chat path branches between a streaming fast-path (no tool grants) and the buffered tool-call loop; embeddings, images, and speech are byte-dumb relays to their pool kind.
- `api.rs` — session-authed JSON at `/api/v0/*`.
- `oidc_handlers.rs` — `/auth/{login,callback,logout}`, backed by a `pending_logins` row keyed by the OIDC `state` parameter.
- `rag_api.rs`, `sandbox_api.rs`, `comfyui_api.rs`, `setup_api.rs` — the remaining JSON surfaces. (`setup_api.rs` lives here rather than in `aiplane-api` so the first-run wizard's API survived the removal of the page stack.)
- `spa.rs` — serves the built SvelteKit SPA from `AIPLANE_STATIC_DIR`: content-type map, cache policy, traversal guard, and the `index.html` history fallback. Its `GET /` + `GET /{*name}` catch-all is registered **last**, because rama matches in registration order.
- `first_run.rs` — the layer that redirects everything to `/setup` until setup completes, with an allowlist for the SPA's static shell.
- `vad.rs` — neural voice-activity detection, trimming silence off uploaded voice notes before Whisper sees them.

`main.rs` wires it all: config → db → upstreams → tools → rbac → SessionStore →
OIDC → `rama_server::router::serve`. The lib target exists so the integration
tests in `tests/` can build the router and drive it with `router.serve(req)`
without binding a socket.

The UI's assets are **not** baked into the binary. `mise run build-web` compiles
`web/` into `target/frontend/build/`, the container image COPYs that directory in,
and `rama_server::spa` serves it from `AIPLANE_STATIC_DIR` — content-hashed bundles
`immutable`, `index.html` and `sw.js` `no-cache`. With the variable unset the UI
answers 503 and nothing else changes, which is what makes a headless deployment
(API + proxy only) a supported configuration rather than an accident.

## Request flow: `POST /v1/chat/completions`

1. **`rama_server::auth::require_bearer`** validates `Authorization: Bearer gwk_…` against the `tokens` table, resolves the user. 401 on miss.
2. **RBAC** (`state.rbac`) maps the user's OIDC roles → role IDs → set of allowed tool IDs.
3. **Branch on the request body:**
   - *Fast path* — no allowed tools. Nothing to inject, so resolve `model` → pool → backend via `state.upstreams.acquire_for`, then `forward_streaming` wraps the upstream's `bytes_stream()` in a `rama::http::Body::from_stream`. The `Acquired` guard rides inside the stream's scan closure so the in-flight slot stays held for the lifetime of the response. (A client-supplied `tools` array does *not* divert here — when the user has grants we take the tool path and union ours in.)
   - *Tool path* — taken whenever the user has tool grants, including when the client brought its own `tools` (unioned in, de-duped by name). The runner in `server::tools::runner` injects tool defs, forces `stream: false`, and loops: acquire pool → forward → if the turn's `tool_calls` are gateway-owned *only*, execute them concurrently and feed the results back as `role: "tool"` messages → re-POST. A turn that calls any client-owned tool is returned to the client unchanged (it drives its own tools). Bounded at 10 rounds. Final response carries an `x-gateway-tool-rounds` header.
4. **`Acquired::drop`** releases the in-flight slot. The pool's atomic counter decrements on the next pick.

## Request flow: chat (JSON over SSE)

Submitting and reading a reply are two separate requests — the SPA holds one long-lived stream open per conversation and posts messages into it.

1. **`POST /api/v0/chat/sessions/{id}/messages`** (`pages::chat::json_api::message_send`) resolves the user from the session cookie, confirms they own the session, and reads the body (multipart when there are attachments).
2. It registers a per-user **worker** slot (a broadcast channel keyed by the user id). A second concurrent submit for the same user is refused rather than racing a parallel stream.
3. It persists the user turn + an `in_progress` assistant turn, auto-titles the session (a heuristic title synchronously, then a background LLM-generated one), and spawns the assistant worker. The worker drives the model — tool-call loop and reasoning included — writing every increment to SQLite and pushing a `TurnUpdate` onto its broadcast after each write. It runs to completion whether or not anyone is listening.
4. **`GET /api/v0/chat/sessions/{id}/events`** (`session_core::chat_json`) is the read side. It emits a `snapshot` rebuilt from the DB, then subscribes to the broadcast and, on each coalesced flush (≥120 ms), diffs the turn row against what this subscriber has already seen and emits `turn_delta` / `reasoning_delta` / `tool_call_started` / `tool_call_done` / `turn_finalized`. It closes on `turn_finalized`, or emits `idle` and closes when no worker is live.
5. **Reconnect is just re-attach.** There is no `Last-Event-ID` replay because the DB *is* the replayer: the snapshot on attach subsumes anything missed. That is why closing a tab mid-stream loses nothing.
6. **`POST /api/v0/chat/sessions/{id}/cancel`** flips the worker's cancel flag; the worker observes it between upstream chunks and exits cleanly into finalize.

The event shapes and the client-side contract (notably `full: true` meaning "replace, don't append") are documented in [`ui.md`](ui.md#chat-streaming-the-json-event-protocol).

## Configuration

No config file. Everything an operator sets is a database row, edited in the
admin UI: topology at `/admin/upstreams`, groups at `/admin/groups`, the OIDC
provider in the setup wizard, and the rest at `/admin/settings`. Secrets are
sealed at rest under the at-rest key; a backend may instead name an environment
variable to read its key from.

`Config` survives as the in-memory runtime shape — `settings::apply` writes the
stored rows over its defaults on boot, so the hundred call sites that say
`state.config().chat.ocr.dpi` never had to change. What is left outside the
database is what has to be resolved *before* it can be opened:
`$AIPLANE_SESSION_KEY`, `$AIPLANE_DB_PATH`, `$AIPLANE_DATA_DIR`,
`$AIPLANE_PUBLIC_URL`, `$AIPLANE_BOOTSTRAP_ADMIN_GROUPS`, and `$IP` / `$PORT`.

See the per-subsystem docs for what each screen controls.
