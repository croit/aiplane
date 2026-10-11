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

AIplane is one binary assembled from a layered stack. An edit recompiles its crate and the crates that depend on it. The most frequently edited handlers and tools sit above shared runtime and persistence code.

```text
aiplane            binary, router, proxy, OIDC
   ├── aiplane-api     browser JSON handlers
   └── aiplane-tools   tool implementations
          └── aiplane-runtime  tool API, AppState, chat and agent drivers
                 ├── aiplane-features  optional subsystems
                 └── aiplane-agents    agent persistence, run sessions, rates
                        └── aiplane-core  database, crypto, RBAC, upstreams
                               ├── session-core  owner-agnostic chat substrate
                               └── shared        wire types
```

`aiplane-api` and `aiplane-tools` are siblings; neither depends on the other. The same holds for `aiplane-features` and `aiplane-agents`. Agent-table accessors belong in `aiplane-agents`, keeping an agent persistence edit from recompiling the core or optional feature layer. Current line counts and rebuild times depend on the checkout and machine; the Cargo dependency graph defines the boundary.

**Rule of thumb when adding code:** put it as high in the stack as it will go.
Something only belongs in `aiplane-core` if code below the feature layer genuinely
needs it. Pushing a module downward for convenience is what makes builds slow
again.

The direction is checked, not just written down:
`workspace_crates_depend_only_down_the_stack` (`crates/aiplane/tests/it/architecture.rs`)
encodes the levels above and fails on any upward or sideways Cargo edge, naming
it. See [`testing.md`](testing.md#architecture-tests).

### `crates/shared`
I/O-free wire types and encoding helpers: the browser API types in `api`, the sandbox runner contract in `sandbox`, and base64/hex helpers. It depends on `serde`, `serde_json` and `jiff`.

### `crates/session-core`
The chat substrate: the `chat_*` tables' accessors, the worker registry, the
`SessionDriver` trait and the JSON-SSE protocol (`chat_json`). It is
owner-agnostic: it reads and writes a person's conversation by `user_id`, and a
conversation whose `user_id` is NULL belongs to an owner it does not model. No
person's read returns one, and its sweep of expired pauses covers a person's
root conversations only. The opaque `parent_turn_id` also fences private child
history from sharing, independent deletion and the root expiry sweep.
Connection-level session and idle turn-pair writers join a caller-held
`BEGIN IMMEDIATE` transaction; their pool wrappers use the same implementation.
The columns of the other owner (`principal_id`, `agent_version`, `visitor_id`,
`lang`) and queries that resolve parent ownership — opening and reading an
agent run, the agent pause sweep, the inbox reads
— live in `aiplane-agents` (`db::run_sessions`), which reuses session-core's
suspension row mapper. The migrations' CHECK and foreign key to
`system_principals` are SQL only; no session-core Rust depends on them.

### `crates/aiplane-core`
The base layer — the things everything else stands on, and the least-edited code
in the tree. No routing, no `AppState`, no tool registry:
- `auth/oidc.rs` — hand-rolled OIDC client (discovery + JWKS-verified ID tokens, on reqwest).
- `auth/jwks.rs` — the one JWKS cache, shared by the OIDC login and an agent's `host_jwt` verifier: each fetches its set its own way, the cache decides when (fresh for a TTL, refetched for an unknown `kid` at most once per window).
- `auth/token.rs` — gateway-token mint/hash helpers.
- `config.rs` — typed `[upstream_pools]`, `[[models]]`, `[oidc]`, `[rbac]` schema.
- `db/` — sqlx; users / tokens / sessions / prefs / usage / limits / …, plus `migrations/` at the crate root, embedded by `db/mod.rs`'s `sqlx::migrate!`. Migrations run on one connection with foreign keys **off** and a `foreign_key_check` after, so a parent table can be rebuilt without `ON DELETE CASCADE` emptying its children (see `migrations/README.md`).
- `principal.rs`, `run_chain.rs` — who acts: a person or a system principal, and on an agent run the call chain (agent → sub-agent → tool) that audit rows serialize. See [`agent-runs.md`](agent-runs.md#the-call-chain).
- `crypto.rs` — AES-256-GCM at-rest sealing for DB-stored secrets.
- `rbac/` — role lookup and grant resolution. It filters grants against the tool and skill registries through the [`GrantableSet`] trait (two methods, used via generics) rather than depending on them, which is what lets RBAC sit at the bottom while the registries live two layers up.
- `upstreams/` — pool registry, backend health probes, RAII `Acquired` guard for in-flight accounting.
- `reasoning.rs`, `model_defaults.rs`, `feature_defaults.rs` — per-model capability and effort tables.
- `tool_naming.rs` — the well-known tool ids/prefixes (`comfyui_`, `typst_`, `enable_tools`, `read_skill`) and the slug→title humaniser. Down here because RBAC, the typst discovery pass, and the catalog all need it and they're on three different layers.
- `net_guard.rs` — the one IP classifier (`classify` → `Public`, `Loopback`, `Private`, `Cgnat`, `LinkLocal`, `Multicast`, …, with IPv4-mapped IPv6 classified as the IPv4 it carries). Every outbound check — `outbound_guard`, the push subscription check, the WebDAV RAG source — and the GeoIP lookup ask it and apply their own policy. Down here because callers sit on three layers.
- `outbound_guard.rs` — the one way to connect to a destination the operator does not configure himself (`fetch_url`, `load_image_url`, `tls_cert`, `dns_lookup` and `whois_lookup` for the model; A2A routes and `host_jwt` JWKS URLs for an agent's owner; Web Push endpoints a browser registered; the MCP OAuth flow's discovery, registration and token endpoints): the host is resolved, every address checked, and the client pinned to those addresses (`resolve_to_addrs`, so DNS rebinding cannot swap one in); the client uses no proxy (environment or system: a proxy would resolve the host again behind the check) and follows no redirect itself, and `get` / `get_with` follow up to five, re-checking every hop. Link-local (cloud metadata), unspecified, broadcast and multicast are always refused. `check_url(url, policy)` is the part that needs no lookup (scheme, host, no credentials, a literal or `localhost` address against the policy); configuration is checked with it on save — A2A card URLs, JWKS URLs, a manager's notification webhook — so saving and running refuse the same URLs. Each caller names its `Policy`: `Policy::agent` and `Policy::web` allow loopback, private and CGNAT only under the operator's `$AIPLANE_ALLOW_PRIVATE_NETWORKS` (`agent` wants `https` unless private networks are allowed, `web` takes plain `http`); `Policy::public_https` (push, the netcheck tools' DoH and RDAP lookups) takes public `https` hosts only, whatever the operator allows; `Policy::mcp_oauth` allows private ranges on purpose (an admin curates the MCP catalog) and plain `http` only to loopback. A host's lookup is bounded to `DNS_BOUND` (5 s); its addresses and the client pinned to them are reused for `CACHE_TTL` (30 s), and the addresses are checked against the caller's policy on every use, so a cached answer is never let through where a fresh one would be refused — caching only defers noticing a DNS change. No other module builds a guarded client: the architecture test's allow-list names this one.
- `capped_read.rs` — `read_capped`, the one bounded reader for outbound response bodies: a declared `Content-Length` over the cap is refused before anything is read, and otherwise reading stops the moment the running total passes it. `read_capped_json` / `read_capped_text` parse on top of it, `read_capped_for` / `read_capped_json_for` word its errors for a person, and `read_error_text` keeps the first 16 KiB of an error answer to quote. Every outbound response is read through it (the architecture test `response_bodies_are_read_only_through_the_capped_reader` fails on a `.bytes()` / `.text()` / `.json()` anywhere else), each with a cap that fits: `API_ANSWER_BYTES` (4 MiB) for token, discovery, search and lookup answers; `MODEL_ANSWER_BYTES` (64 MiB) for a model backend's whole answer (non-streamed completions, embeddings, base64 images, model lists); and the caller's own where neither fits (`fetch_url` 32 MiB, `load_image_url` and ComfyUI outputs 25 MiB, WebDAV listings 32 MiB, the GeoIP download 512 MiB, the sandbox runner's answer 768 MiB).
- `usage/`, `limits/` — the metrics sink and the spend-limit/quota enforcer. Usage rows carry the run's `agent_id` and the enforcer checks an agent's budget against them, because spend is metered for every caller in one place; the agent's *visitor rates* are agent-only and live in `aiplane-agents` (`rates`).
- What stays here although agents use it, and why: the one embedded migration set (the sqlx history is never split, so the agent tables' DDL is here while their accessors are not); `Principal`, `GrantSet`, `GrantKind` and `RunChain` (RBAC, the upstream registry and usage metering read them); the `gwe_`/`gwv_` token helpers in `auth/token.rs` (one module proves every bearer prefix disjoint); and `db/reseal.rs`'s list of sealed columns, which names `agent_notify_channels` and the three agent spec columns (whose credentials it finds by their `*_sealed` keys, without knowing the spec's layout) so key rotation covers every secret in one pass.
- `rama_server/session.rs` — signed-cookie + sqlite session store, plus the `is_safe_return_to` redirect guard the OIDC callback needs to bounce a signed-in user back to the SPA route they asked for; `rama_server/cors.rs` — the `/v1` CORS layer and the plumbing both CORS layers share (`CorsHeaders`: the headers an allowed origin gets and the preflight answer; `vary_origin`). The embed layer reads agent data, so it lives with the router in `aiplane`; the two differ on purpose only in which origin they allow. Neither needs `AppState`, so both stay here.

### `crates/aiplane-features`
The optional subsystems — what a deployment switches on at `/admin/settings`
and can run entirely without: `rag/`, `skills.rs`, `comfyui/` (client, store, manifest,
runner, scheduler), `push/`, `github/`, `geoip/`, `typst.rs`, `image_gen.rs`,
`chat_attachments.rs`, `embeddings.rs`, `speech.rs`, `vad.rs` (silence trimming ahead of Whisper), `pdf.rs`, `ocr.rs`,
`search_settings.rs`, and `document_canvas.rs` (the chat canvas store, shared
by the chat document endpoints and the document tools above).

Each stands on `aiplane-core` and knows nothing about `AppState`, the tool
registry, or routing. That ignorance is the whole point — it's what lets this
layer sit below the runtime. A reference from here up into `aiplane-runtime`
collapses the split.

### `crates/aiplane-agents`
The agent builder's persistence and the pieces only agents need, on
`aiplane-core` beside `aiplane-features` (neither depends on the other):
- `db/` — accessors for the agent tables: `agents` (versions, shares),
  `system_principals` (grants, system tokens), `agent_audit`, `embed_keys`,
  `visitor_sessions`, `a2a_contexts`, `agent_a2a_tasks`, `agent_state`,
  `agent_tests`, `agent_analytics`, `agent_verifiers`, `agent_channels`,
  `agent_retention`. They share `aiplane-core`'s pool, error
  type and timestamp helpers; the DDL stays in `aiplane-core`'s migrations.
- `db/run_sessions.rs` — conversations owned by a system principal: creating and
  reading them, decoding a conversation's owner, the sweep of an agent
  conversation's expired pause, and the inbox reads that tell a person's pause
  from an agent's. session-core keeps no knowledge of them (see
  `crates/session-core` above).
- `rates.rs` — every sliding-window rate of an agent through one primitive,
  `record_within`: an event is checked against its windows and recorded in
  one `WriteTx`, so parallel requests cannot all pass. The visitor gate
  (per-conversation and per-IP admissions over every inbound channel) and
  the verifiers' send and lookup limits both use it.
- `db/write_tx.rs` — `WriteTx`, a transaction that holds the write lock from
  `BEGIN IMMEDIATE`; the activity log's `append` and the rate primitive take
  one, so nothing reads what it then writes in a deferred transaction.
- `notify_channels.rs` — the Slack/Discord webhook payloads of the inbox.

Like `aiplane-features`, it names neither `AppState` nor the tool registry, and
nothing below it names an agent.

### `crates/aiplane-runtime`
Where the world gets tied together:
- `server/tools/` — the tool *machinery*: the `Tool` trait and `ToolContext`, the `ToolRegistry`, the round-loop `runner`, the `catalog` (tool id → group → toggle key), the MCP connection manager, and the sandbox client. Implementations live in `aiplane-tools`, above; `echo` and `get_current_timestamp` stay here as the canonical trivial tools that the registry/runner tests build registries out of.
- `server/state.rs` — `AppState`: the db pool, config, `Arc<UpstreamRegistry>`, `Arc<ToolRegistry>`, `Arc<Resolver>`, and the optional feature handles (RAG indexer, skills, ComfyUI, push, geoip, sandbox client, MCP manager).
- `rama_server/state.rs` — `RamaState` wraps `AppState` (via `Deref`) and adds the session store, worker registry, usage sink and rate-limit enforcer; `rama_server/auth.rs` — `require_bearer` for `/v1/*`.
- `openai_driver.rs` — the `session_core::SessionDriver` impl that streams a chat completion, plus `loop_guard.rs`.
- `openai_driver/turn_policy.rs` — the chat driver's turn policy. `TurnPolicy` is `Chat` (a person's turn) or `Agent(&AgentRun)`, taken from the turn's `Actor`, and owns everything that differs between them: the leading system message (built in one place, refreshed per round only when an agent's conversation state can change it), the round's tool offer (a person's per-conversation overlay; an agent's grants ∩ spec tools plus its synthetic and terminal tools), the model access, budget, injection scan and usage name, the conversation's "off" switches (a person's only), how the final round and a round without calls end under a finish contract, and settling the run (`run_outcome`). The round loop calls the policy and never asks whether it runs an agent, so an agent run makes none of the chat-only reads (`allowed_tools_for_session`, `chat_session_tools`, the MCP enabled-key union). An enum, not a trait object: the set is closed, as `Actor` is, and the methods are async.
- `suspend.rs` + `openai_driver/resume.rs` — durable suspend and resume: a tool returns `tool_suspend(…)`, the driver writes the run state to `chat_turn_suspensions` and frees the worker, and a resume continues the same turn from there. `server/tools/ask_first.rs` (`AskFirst`) wraps any tool so it runs only after someone approves the call. See [`tools-rbac.md`](tools-rbac.md#suspend-and-resume).
- `finish.rs` — the completion contract for non-interactive runs (`FinishContract`, `RunOutcome`): a run given one ends only through a schema-valid `finish(result)` call or a structured incomplete outcome. `headless::drive` takes one and returns the outcome. See [`tools-rbac.md`](tools-rbac.md#finish-contract).
- `server/{scheduled,webhooks,compaction,headless}` — the background workers that need state. `headless::open_session`/`drive` run as a person or as a system principal: an agent run's session is owned by the principal (no person's chat), it is offered only the principal's grants, and with a `RunChain` every tool call's decision is audited (`openai_driver/call_policy.rs`).
- `agents/` — the agent runtime ([`agents.md`](agents.md)): the spec validator and the typed `AgentSpec` it produces (`agents/spec/model.rs`, the only reader of a spec's JSON and the one place its defaults live), the spec cache that holds each published version compiled, the run profile, router, verifiers, human handoff, A2A client and server halves, output filter and evals.
- `server/model_choices.rs` — the one list of models a caller may pick, by kind (model ids, backend aliases, automatic routes), with the gateway default first: the chat picker (`GET /api/v0/models`), the agent resources, the grant cap for kind `model` and the agents' unset-model default all read it ([`agents.md`](agents.md#models)). `server/model_route.rs` — resolving a model name the way a chat turn does (an automatic route by its selector), shared by the chat driver and an agent's own model calls.
- `server/comfyui_tool.rs` — the ComfyUI `Tool`/`ToolSource` impls and the `ComfyuiHandle` that `AppState` holds. Split out of `aiplane-features`' `comfyui/` because it needs the tool API.

`aiplane-tools` and `aiplane-api` both sit on this and neither depends on the
other, so a tool edit and a page edit stay independent.

### `crates/aiplane-tools`
The tool implementations — one module per tool family (`fetch_url`,
`fetch_attachment`, `search_web`, `typst_render`, `document`, `rag`, `memory`,
`qr`, `netcheck`, …). Each holds `Tool` impls; they plug into the machinery in
`aiplane-runtime` and are registered into the `ToolRegistry` that `aiplane`'s
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
The `/api/v0` JSON handlers the SPA calls. `pages/mod.rs` carries the
shared helpers every handler uses — `require_session_json` / `require_admin_json`
(the 401/403 gates), `json_ok` (a success body of any serializable wire type) and `json_error` / `json_error_with` (the refusal, always `shared::api::ErrorEnvelope`, the last with extra fields such as a validator's `issues`; the gateway crate's handlers build theirs with the same `json_error`) — and
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

### `crates/aiplane`
The binary and its routing glue — deliberately thin:
- `router.rs` — builds the `rama::http::service::web::Router`, mounting handlers from `aiplane-api` and this crate.
- `proxy.rs` — `/v1/{models,chat/completions,audio/transcriptions,audio/speech,embeddings,rerank,images/generations,images/edits}` handlers. The chat path branches between a byte-dumb path and a gateway-owned tool loop with buffered and streaming forms; embeddings, rerank, images, and speech are byte-dumb relays to their pool kind.
- `translated.rs` — the chat pipeline behind a translated dialect: tool layer, automatic routing, content guard, alias resolution with the outage wait, limits, model defaults, reasoning effort, and the buffered or streaming tool loop. `messages.rs` (`/v1/messages`, Anthropic) and `responses/` (`/v1/responses`, OpenAI Responses) translate their request into a chat completion, run it here, and translate the result back; each supplies only its refusal wording and the `StreamSink` that encodes its stream.
- `responses/` — the Responses translation (`request.rs`, `output.rs`, `stream.rs`) and `store.rs`, the `api_responses` table behind `store`, `previous_response_id` and `GET`/`DELETE /v1/responses/{id}`.
- `api.rs` — session-authed JSON at `/api/v0/*`.
- `oidc_handlers.rs` — `/auth/{login,callback,logout}`, backed by a `pending_logins` row keyed by the OIDC `state` parameter.
- `rag_api.rs`, `sandbox_api.rs`, `comfyui_api.rs`, `setup_api.rs` — the remaining JSON surfaces. (`setup_api.rs` lives here rather than in `aiplane-api` so the first-run wizard's API survived the removal of the page stack.)
- `spa.rs` — serves the built SvelteKit SPA from `AIPLANE_STATIC_DIR`: content-type map, cache policy, traversal guard, and the `index.html` history fallback. Its `GET /` + `GET /{*name}` catch-all is registered **last**, because rama matches in registration order.
- `openapi/` — `GET /openapi.json`: the routes scanned out of `router.rs`, each described by its declaration (credential, request, responses, errors) with schemas derived from the handlers' wire types. See [`ui.md`](ui.md#the-json-api-and-its-contract).
- `first_run.rs` — the layer that redirects everything to `/setup` until setup completes, with an allowlist for the SPA's static shell.
- `body_limit.rs` — the request body cap every route sits behind. The layer reads the body itself (a declared length over the cap is refused before anything is read; otherwise reading stops as the running total passes it) and hands the handler buffered bytes, so no handler can drain an unbounded body: 1 MiB by default, 64 MiB on the large-body routes (`/v1/*`, `/api/v0/chat/*`, transcription, feedback, skill uploads), `413 payload_too_large` past it — in the Anthropic envelope (`request_too_large`) on `/v1/messages` and `/v1/messages/count_tokens`, like every other error there. `/hooks`, `/a2a` and `/api/v0/embed` read through their own tighter caps and are passed through. The cap is not matched on the path: `router.rs` registers each group of routes under its own endpoint layer, `.with_endpoint_layer(endpoint(BodyLimitLayer::DEFAULT | UPLOAD | ANTHROPIC_UPLOAD | HANDLER_CAPPED))`, so a route takes the cap of the group it is written in; a route added under `HANDLER_CAPPED` must cap its own read, and the architecture test reads that group out of the router and checks its handlers.

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

1. **Authenticate the credential.** `aiplane_runtime::rama_server::auth::require_bearer` accepts a gateway bearer or `x-api-key`, resolves a person or system principal, and applies the credential's effective restrictions.
2. **Resolve access and routing.** The requested model can name a real model, an alias or an automatic route. The proxy checks pool access, model restrictions and applicable limits before upstream work.
3. **Execute the appropriate path.** With no gateway-owned tool layer, the proxy forwards the request and relays the response. With gateway tools, it combines gateway and client tool definitions and drives buffered or streamed upstream rounds. It executes gateway-owned calls and returns client-owned calls to the client. The round budget bounds the turn and requests a final answer when exhausted; see [the gateway contract](gateway-api.md#tool-round-budget).
4. **Release capacity.** An `Acquired` guard holds the backend's in-flight slot for the lifetime of the request/response work and releases it on drop.

## Request flow: chat (JSON over SSE)

Submitting and reading a reply are two separate requests — the SPA holds one long-lived stream open per conversation and posts messages into it.

1. **`POST /api/v0/chat/sessions/{id}/messages`** (`pages::chat::json_api::message_send`) resolves the user from the session cookie, confirms they own the session, and reads the body (multipart when there are attachments).
2. Submission uses the existing worker registry and durable work queue. The server returns `placement: started`, `folded` or `queued`: a message starts work, joins running work or waits for capacity. The client renders that server decision rather than maintaining its own submission queue.
3. It persists the user turn + an `in_progress` assistant turn, auto-titles the session (a heuristic title synchronously, then a background LLM-generated one), and spawns the assistant worker. The worker drives the model — tool-call loop and reasoning included — writing every increment to SQLite and pushing a `TurnUpdate` onto its broadcast after each write. It runs to completion whether or not anyone is listening.
4. **`GET /api/v0/chat/sessions/{id}/events`** (`session_core::chat_json`) is the read side. It emits a `snapshot` rebuilt from the DB, then subscribes to the broadcast and, on each coalesced flush (≥120 ms), diffs the turn row against what this subscriber has already seen and emits `turn_delta` / `reasoning_delta` / `tool_call_started` / `tool_call_done` / `turn_finalized`. It closes on `turn_finalized`, or emits `idle` and closes when no worker is live.
5. **Reconnect is just re-attach.** There is no `Last-Event-ID` replay because the DB *is* the replayer: the snapshot on attach subsumes anything missed. That is why closing a tab mid-stream loses nothing.
6. **`POST /api/v0/chat/sessions/{id}/cancel`** flips the worker's cancel flag; the worker observes it between upstream chunks and exits cleanly into finalize. With no worker but a turn suspended for a decision, it gives up on the decision and the turn ends `cancelled`.
7. **A suspended turn** (a tool asked for an approval) has no worker. The stream says what it waits for (`suspended`) and closes; **`POST …/turns/{turn_id}/resume`** answers it and starts a worker that continues the same turn, after a restart too. Until then the conversation is held: a new message is refused with `409 decision_pending` (it would run before the paused call, in a context the decision was not asked about), and the composer says why.

The event shapes and the client-side contract (notably `full: true` meaning "replace, don't append") are documented in [`ui.md`](ui.md#chat-streaming-the-json-event-protocol).

## Configuration

No config file. Everything an operator sets is a database row, edited in the
admin UI: topology at `/admin/upstreams`, groups at `/admin/groups`, the OIDC
provider in the setup wizard, and the rest at `/admin/settings`. Secrets are
sealed at rest under the at-rest key; a backend may instead name an environment
variable to read its key from.

`Config` survives as the in-memory runtime shape — `settings::apply` writes the
stored rows over its defaults on boot, so the hundred call sites that say
`state.config().chat.ocr.dpi` never had to change. Those typed reads fall
back to the default on a stored value they cannot use, so a save checks first:
`FieldSpec::check` accepts exactly what they use (a whole number of 0 or more,
a finite number with either decimal separator, one of a choice's options), and
`POST /api/v0/admin/settings` refuses a section with any unusable field
(`422 invalid_settings`, one translated `issues` entry per field) without
storing any of it. What is left outside the
database is what has to be resolved *before* it can be opened:
`$AIPLANE_SESSION_KEY`, `$AIPLANE_DB_PATH`, `$AIPLANE_DATA_DIR`,
`$AIPLANE_PUBLIC_URL`, `$AIPLANE_BOOTSTRAP_ADMIN_GROUPS`,
`$AIPLANE_TRUSTED_PROXIES`, and `$IP` / `$PORT`. The last decides the client IP
for every consumer (`RamaState::client_ip`): the TCP peer unless it is a trusted
proxy, then the rightmost untrusted `X-Forwarded-For` hop.

See the per-subsystem docs for what each screen controls.
