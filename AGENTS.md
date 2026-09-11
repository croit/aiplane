# AGENTS.md — read this first

This file is the canonical entry point for any AI agent (or new human contributor) working in this repo. Read it end-to-end before doing anything else.

## What this project is

A single Rust binary plus the supporting crates it lives on:

- **`gateway`** — authenticated, OpenAI-compatible LLM proxy. Speaks `/v1/chat/completions`, `/v1/audio/transcriptions`, `/v1/models` so any OpenAI SDK talks to it, and `/v1/messages` in the Anthropic dialect so Claude Code can be pointed at it. OIDC browser login + gateway-minted bearer tokens. Routes across **multiple upstream LLM backends** with health checks + RAII in-flight accounting. Injects **company-specific tools** gated by **RBAC**. Serves a SvelteKit single-page app (dashboard / tokens / persisted multi-conversation chat) over a JSON `/api/v0` API.

Shared crates:
- **`session-core`** — chat substrate (DB schema + worker registry + the JSON-SSE event protocol in `chat_json` + `SessionDriver` trait). The gateway plugs in an `OpenAiDriver`; the trait keeps the substrate driver-agnostic so a future second consumer can drive the same chat surface without forking.
- **`shared`** — OpenAI wire types shared across the workspace.

Built on **rama 0.3** (HTTP server + router + middleware) on the server, and **SvelteKit 2 + Svelte 5** with **daisyUI v5 + Tailwind v4** in `web/`. The browser talks to `/api/v0` over JSON and receives live turn updates as JSON frames on an SSE stream.

## Repo layout

```
/
├── AGENTS.md                    # this file
├── README.md                    # human-facing — keep current with deploy story
├── mise.toml                    # toolchain pin + build/test/lint tasks
├── Cargo.toml                   # workspace manifest (9 members)
├── Dockerfile                   # gateway runtime image
├── docs/                        # detailed design docs (index in docs/README.md)
├── web/                         # SvelteKit SPA (Tailwind v4 + daisyUI v5) — see docs/ui.md
├── gateway.example.toml         # template config — copy to gateway.toml
└── crates/
    ├── shared/                  # OpenAI wire types, shared with the CLI
    ├── session-core/            # chat-style UI substrate
    │   ├── src/                     SessionDriver trait, worker registry, db (chat_*
    │   │                            tables), Plait renderers (markdown + lumis-highlighted
    │   │                            code), SSE primitives, icons
    │   └── ui/ts/                   composer + scroll TS
    ├── gateway-core/            # base: db, config, crypto, rbac, upstreams
    ├── gateway-features/        # optional subsystems: rag, skills, comfyui, push, …
    ├── gateway-runtime/         # tool API + AppState/RamaState + chat driver
    ├── gateway-tools/           # the tool implementations
    ├── gateway-api/             # the server-rendered HTML pages
    ├── gateway/                 # the binary: router, proxy, api, main
    └── sandbox-runner/          # the sandboxed-tool execution service
```

### The gateway crate stack

The gateway is one binary assembled from three layered crates. This is **load
bearing for build speed**, not cosmetic: it used to be ~108k lines in one
compilation unit, so editing any file re-ran the whole frontend + codegen. Each
crate depends only on the ones beneath it.

```
gateway            bin + router/proxy/api/oidc      6.5k  ← thinnest, most-edited
   ├── gateway-api     server-rendered HTML pages  25.5k  ← siblings: neither
   └── gateway-tools   the tool implementations    14.5k  ←   depends on the other
          └── gateway-runtime  tool API + AppState/RamaState + chat driver  14.7k
                 ├── gateway-features  RAG, skills, ComfyUI, push, geoip, …  13.9k
                 └── gateway-core      db, config, crypto, rbac, upstreams   22.1k
```

Lines that must recompile after a one-line edit: `gateway` 6.5k, `gateway-tools`
21k, `gateway-api` 32k, `gateway-runtime` 61k, `gateway-features` 75k,
`gateway-core` 97k — against **97k for any edit** before the split. The gains are
front-loaded on purpose: the layers that churn most are the cheapest to rebuild.

`gateway-api` and `gateway-tools` are siblings: neither depends on the other, so
editing a page doesn't rebuild the tools and vice versa.

Two rules keep it that way, and both are easy to break by accident:
1. **Put new code as high in the stack as it will go.** Something belongs in
   `gateway-core` only if code below the feature layer genuinely needs it.
2. **Never reference upward.** `gateway-features` must not name `AppState` or the
   tool registry; `gateway-core` must not name a feature. One such reference
   collapses a layer.

**When adding code, put it as high in the stack as it will go.** Something only
belongs in `gateway-core` if code below the page layer actually needs it. Adding a
reference from `gateway-core` to a page — or pushing a module downward for
convenience — makes every build slow again. See [`docs/architecture.md`](docs/architecture.md#crate-boundaries).

Inside `crates/gateway-core/src/` (base layer):

```
migrations/               # (crate root) sqlx migration set, embedded by db/mod.rs
server/
    auth/oidc.rs              hand-rolled OIDC client (reqwest)
    auth/token.rs             gateway-token mint/hash helpers
    config.rs                 [upstream_pools] + [[models]] + [oidc] schema
    db/                       sqlx; users/tokens/sessions/prefs/usage — chat_* live
                              in session-core, gateway just runs the migration
    crypto.rs                 AES-256-GCM at-rest sealing
    rbac/                     role → tool/model resolution; filters against the
                              registries via the GrantableSet trait, so it can sit
                              at the bottom while they live two layers up
    upstreams/                pool registry, health probes, RAII Acquired guard
    tool_naming.rs            well-known tool ids/prefixes + slug→title humaniser
    usage/ limits/            metrics sink, rate-limit + quota enforcer
rama_server/
    session.rs                signed-cookie + sqlite session store; is_safe_return_to
    cors.rs                   the CORS layer
```

Inside `crates/gateway-features/src/server/`: the optional subsystems — `rag/`,
`skills.rs`, `comfyui/`, `push/`, `github/`, `geoip/`, `typst.rs`, `image_gen.rs`,
`chat_attachments.rs`, `embeddings.rs`, `speech.rs`, `pdf.rs`, `ocr.rs`,
`search_settings.rs`, `document_canvas.rs`. None of them may name `AppState` or the
tool registry.

Inside `crates/gateway-runtime/src/`:

```
openai_driver.rs          # SessionDriver impl: OpenAI streaming chat-completions
loop_guard.rs
server/
    tools/                    Tool trait, ToolContext, registry, catalog, runner,
                              MCP manager, sandbox client (impls: gateway-tools;
                              echo + time stay here as the test fixtures)
    state.rs                  AppState
    comfyui_tool.rs           ComfyUI Tool/ToolSource impls + ComfyuiHandle
    scheduled/ webhooks.rs compaction.rs headless.rs   state-dependent workers
rama_server/
    state.rs                  RamaState (wraps AppState + sessions/usage/limits)
    auth.rs                   require_bearer for /v1/*
```

Inside `crates/gateway-tools/src/`: one module per tool family (`fetch_url`,
`search_web`, `typst_render`, `document`, `rag`, `qr`, `netcheck`, …). Register a
new tool in the `ToolRegistry` that `gateway`'s `main.rs` builds, and grant it in
`[rbac]`. `tests/` holds the two test modules that need both the machinery and the
real tools (catalog grouping, `AppState` authorization).

Inside `crates/gateway-api/src/`:

```
build_info.rs             # git SHA / version label (build.rs stamps it)
pages/                    # the /api/v0 JSON handlers (the name predates the SPA)
    mod.rs                    shared helpers — auth gates, error envelope, raw path segments
    chat/json_api.rs          chat CRUD, the JSON submit, and the SSE event stream
    json_admin.rs             the /api/v0/admin/* surfaces
    json_workspace.rs         memory, scheduled actions, webhooks
    json_skills.rs            skills, connectors, feedback, ComfyUI
    rag_oauth.rs, integrations.rs
                              the only server-RENDERED pages left: OAuth callback
                              landings, which a provider redirects a browser to
                              before any SPA route exists
```

Inside `crates/gateway/src/`:

```
main.rs                   # boot: config → state → SessionStore → OIDC → rama serve
rama_server/              # routing glue only:
    router.rs                 the rama::http::service::web::Router builder
    proxy.rs                  /v1/{models,chat/completions,audio/transcriptions,…}
    api.rs                    session-authed /api/v0/* JSON endpoints
    oidc_handlers.rs          /auth/{login,callback,logout}
    rag_api.rs sandbox_api.rs comfyui_api.rs
    vad.rs                    silence trimming ahead of Whisper
tests/it/                 # integration suite — builds the router, serves requests in-process
```

The SPA is built by `mise run build-web` into `target/frontend/build/` and
served from disk by `rama_server::spa` when `GATEWAY_STATIC_DIR` points there;
the Dockerfile COPYs that directory into the image. Nothing is `include_bytes!`'d
any more.

## Hard rules — do not violate without asking

1. **Minimize Cargo dependencies.** Every new crate added to `Cargo.toml` requires a one-line justification in [`docs/dependencies.md`](docs/dependencies.md). Prefer stdlib + what rama already brings in.
2. **All toolchain and build/test/lint commands go through `mise`.** No `Makefile`, no `justfile`, no ad-hoc shell scripts checked in. See [`docs/dev-workflow.md`](docs/dev-workflow.md).
3. **Thorough testing, test-first (TDD).** Write the failing test before the implementation — red, green, refactor. Every public function has unit tests; every rama route has an integration test (`crates/gateway/tests/`); upstream LLMs are mocked with `wiremock` so tests run offline. The rama integration pattern is `router.serve(req).await` — no socket binding. **Style is Chicago / Classicist (state-based):** assert on observable results and real collaborators (in-memory SQLite via `:memory:`, `wiremock` upstreams, actual registries), not on interaction mocks. Reach for London-school behaviour-verification mocks only when a collaborator is genuinely un-fakeable (network you can't stand up, a clock, randomness) — and say so in a comment. Full strategy + required coverage in [`docs/testing.md`](docs/testing.md).
4. **Error messages are a product surface.** Use `thiserror` at API boundaries, `anyhow` + `.context()` internally, and write messages that say *what was happening, what went wrong, and what to do about it*. Full rules in [`docs/errors.md`](docs/errors.md).
5. **UI uses daisyUI component classes + Tailwind utilities, not hand-invented CSS.** Every visual element gets daisyUI semantic classes (`btn btn-primary`, `card card-body`, `alert alert-error`, `dropdown dropdown-end`, `badge badge-outline`, …) in the Svelte components under `web/src/`. Token utilities for bespoke layout (`bg-base-100`, `text-base-content/60`, `border-base-300`, `text-error`, …) plus standard Tailwind layout (`flex`, `mb-4`, `grid`). One-off ".tagline" / ".brand-mark" classes are not — drop the visual treatment or push daisyUI for the missing component. New server endpoints return JSON under `/api/v0`, never HTML; live turn updates ride the JSON-SSE protocol in `session_core::chat_json`. See [`docs/ui.md`](docs/ui.md).
6. **No comments explaining what code does** — names and types should already say that. Only comment *why* when it's non-obvious. Docs explain the system; code shows it.
7. **No backwards-compat shims** while the project is pre-1.0. We're starting fresh; if something needs to change, change it.
8. **Keep `README.md` deploy-current.** When you add or change a runtime knob, a config field, a host-package requirement, or a mise task on the deploy path, update `README.md` in the **same commit**. The README's "Quick start" + "Build + deploy" sections are the only thing a new operator reads before standing the stack up; if they don't reflect today's state, the next person wastes an hour. This is a strengthening of the broader "update docs in the same change as the code" rule from the working agreement at the bottom of this file — same spirit, just calling out the entry door explicitly so it doesn't drift.
9. **User-visible strings go through the translation layer; everything else is English.** The product ships in six languages (en/de/fr/es/ru/zh). The Fluent catalogs under `crates/session-core/locales/<lang>/*.ftl` are canonical for BOTH halves — the server renders some strings itself (tool prompts, OAuth error pages, proxy errors) and the SPA renders the rest, and both must name a message identically or the two disagree in front of the user. Never hardcode a user-visible string in a Svelte component or a handler: add the key to all six `.ftl` files, run `mise run gen-locales`, and call `t('key')`. `build.rs` fails the build if a language is missing a key, and `i18n_drift` fails if the generated TS catalogs are stale.

   Everything a user does *not* see — log lines, comments, identifiers, test names — is English, always. Do not mix languages within a single message. Non-English text outside the catalogs is allowed only for *domain content that is intrinsically in another language*: the German business-letter fixture under `examples/typst-templates/letter/`, or a non-ASCII character used deliberately in a test (`'ß'` for a UTF-8 boundary case). Those are data, not app strings.
10. **Code principles — DRY, SOLID, KISS, Ubiquitous Language.** Default to the simplest thing that works (**KISS**) and don't repeat a fact or a shape in two places (**DRY** — extract a helper like `ChunkMeta::envelope` rather than copy a JSON literal twice). Follow **SOLID** where it pulls its weight: the `Tool` trait + `ToolRegistry` already give you open/closed extension (add a tool, don't touch the loop) and dependency inversion (drivers depend on the `SessionDriver` trait, not a concrete bin) — keep new code on that grain. Speak the codebase's **Ubiquitous Language** consistently in names, comments, and docs: `upstream` / `pool` / `backend`, `gateway-owned` vs `client-owned` tool calls, `turn` / `round`, `byte-dumb proxy`, `Acquired` in-flight guard. Don't coin a synonym for a term that already exists. These are guidance, not gates — if applying one would bloat or obscure, prefer the simpler code and note why.

## Daily workflow

After `mise install` (one time):

```bash
# One terminal during UI work: public Vite/HMR on :8080 proxies every
# dynamic route to the private debug gateway on :8081.
mise run dev
mise run dev-served        # production-shaped compiled SPA, no HMR

# Other day-to-day tasks
mise run dev-build         # debug build only (target/debug/gateway), ~2 s incremental
mise run build-css         # one-shot CSS build
mise run fmt               # cargo fmt

# Verifying a change — climb this ladder, don't start at the top
mise run check                     # workspace type-check, seconds
mise run test-crate <crate> [filt] # tests for the crate you touched
mise run lint-crate <crate>        # clippy for that crate
mise run verify                    # ONCE before pushing: lint + all tests (~20 min)
mise run test / mise run lint      # the whole-workspace halves of `verify`

# Browser-driven UI debugging (no OIDC required)
mise run dev-ui            # real rama server on :8080 + wiremock chat &
                           # transcription backends + a pre-seeded session;
                           # prints the cookie for playwright. Use this —
                           # NOT a hand-rolled test.html — when debugging
                           # ANY authed page (chat, tokens, dashboard,
                           # theme toggle, /api/v0/*). See docs/dev-workflow.md
                           # → "Debugging the UI".

# Slow path — DON'T use for iteration
mise run build             # cargo build --release. ~12 s incremental, ~70 s clean.
                           # Only for deploys / perf measurement.
mise run ci                # verify + the release build. 20-25 min. CI runs this
                           # on every push; you almost never need it locally.

# Housekeeping
mise run setup-hooks       # once per clone: git hooks + keep Spotlight out of target/
mise run sweep-target      # reclaim target/ (cargo never GCs it; it hit 1.16M
                           # files / 327 GB here, 99% of it dead, and cargo
                           # scans that directory on every invocation)
```

**Debug, not release.** `mise run dev` and `mise run dev-build` produce debug binaries — runtime perf is identical to release for anything you'd interact with manually (the entire UI surface, smoke testing). Use `mise run build` only when you're shipping or actually benchmarking; rebuilding release on every iteration wastes 10 s per cycle for no gain.

The mise tasks DAG handles the CSS prerequisite automatically: `dev`, `dev-build`, `build`, `test`, and `lint` all depend on `build-css`, so a fresh checkout doesn't need any manual setup.

Full reference: [`docs/dev-workflow.md`](docs/dev-workflow.md).

## Where to find what

Start in [`docs/README.md`](docs/README.md) for the index. The topical docs:

| Topic | Doc |
|---|---|
| System architecture, request flow, component boundaries | [`docs/architecture.md`](docs/architecture.md) |
| Toolchain, mise tasks, dev loop | [`docs/dev-workflow.md`](docs/dev-workflow.md) |
| Dependency policy + the current allowed list | [`docs/dependencies.md`](docs/dependencies.md) |
| OIDC login + gateway-minted tokens | [`docs/auth.md`](docs/auth.md) |
| OpenAI-compat endpoints, streaming, transcription | [`docs/gateway-api.md`](docs/gateway-api.md) |
| Claude Code / Anthropic Messages compatibility (`/v1/messages`) | [`docs/claude-code.md`](docs/claude-code.md) |
| Multi-provider routing, load balancing, health checks | [`docs/upstreams.md`](docs/upstreams.md) |
| Tool registry, role→tool mapping, execution loop | [`docs/tools-rbac.md`](docs/tools-rbac.md) |
| Which tools exist, their gates and toggle keys | [`docs/tools-inventory.md`](docs/tools-inventory.md) |
| Web UI — the SvelteKit SPA, the JSON API, the SSE protocol | [`docs/ui.md`](docs/ui.md) |
| Testing strategy and required coverage | [`docs/testing.md`](docs/testing.md) |
| Error handling — types, messages, OpenAI mapping | [`docs/errors.md`](docs/errors.md) |
| Phased delivery plan + current phase | [`docs/roadmap.md`](docs/roadmap.md) |

## Working agreement for agents

- **Plan before you implement.** For anything that touches more than one file or one concept, draft an approach and confirm before writing code.
- **Update docs in the same change as the code.** If you change the auth flow, update `docs/auth.md` in the same commit. Stale docs are worse than no docs.
- **When you discover a missing piece** — an undocumented invariant, a non-obvious gotcha — add it to the relevant doc. Don't rely on conversation history.
- **Tests live next to the code.** Unit tests in `#[cfg(test)] mod tests`, integration tests in `crates/gateway/tests/`. Run `mise run verify` before declaring a task done.
- **Verify cheaply, then once for real.** Compiling dominates this workspace: a full `mise run verify` is ~20 minutes, of which ~19 are linking test binaries, not running tests. Iterate with `mise run test-crate <crate>` (seconds), then run the full gate **once**, at the end, after every fix you already know about is in. Starting it earlier means paying it twice. Don't kill a cargo process to unstick a parallel mise task — they share one `target/` lock, and killing one fails its sibling. See [`docs/dev-workflow.md`](docs/dev-workflow.md) → "The feedback ladder".
- **If a hard rule is in your way**, surface it to the user. Don't quietly bypass.
