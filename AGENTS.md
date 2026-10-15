# AGENTS.md — read this first

This file is the canonical entry point for any AI agent (or new human contributor) working in this repo. Read it end-to-end before doing anything else.

## What this project is

**croit AIplane** — a self-hosted AI infrastructure layer: one plane connecting applications and users to models, agents, tools and enterprise data. "The gateway" below means the OpenAI-compatible proxy layer, which is one subsystem.

A single Rust binary (`aiplane`, built from the `gateway` crate) plus the supporting crates it lives on:

- **`gateway`** — authenticated, OpenAI-compatible LLM proxy. Speaks `/v1/chat/completions`, `/v1/audio/transcriptions`, `/v1/models` so any OpenAI SDK talks to it, and `/v1/messages` in the Anthropic dialect so Claude Code can be pointed at it. OIDC browser login + gateway-minted bearer tokens. Routes across **multiple upstream LLM backends** with health checks + RAII in-flight accounting. Injects **company-specific tools** gated by **RBAC**. Serves a SvelteKit single-page app (dashboard / tokens / persisted multi-conversation chat) over a JSON `/api/v0` API.

Shared crates:
- **`session-core`** — chat substrate (DB schema + worker registry + the JSON-SSE event protocol in `chat_json` + `SessionDriver` trait). AIplane plugs in an `OpenAiDriver`; the trait keeps the substrate driver-agnostic so a future second consumer can drive the same chat surface without forking. It is owner-agnostic too: it knows a person's conversation by `user_id` and treats any other owner as opaque — agent runs, visitors and principals are `aiplane-agents`' business.
- **`shared`** — OpenAI wire types shared across the workspace.

Built on **rama 0.3** (HTTP server + router + middleware) on the server, and **SvelteKit 3 + Svelte 5** with **daisyUI v5 + Tailwind v4** in `web/`. The browser talks to `/api/v0` over JSON and receives live turn updates as JSON frames on an SSE stream.

## Repo layout

```
/
├── AGENTS.md                    # this file
├── README.md                    # human-facing — keep current with deploy story
├── mise.toml                    # toolchain pin + build/test/lint tasks
├── Cargo.toml                   # workspace manifest (10 members)
├── Dockerfile                   # gateway runtime image
├── docs/                        # detailed design docs (index in docs/README.md)
├── web/                         # SvelteKit SPA (Tailwind v4 + daisyUI v5) — see docs/ui.md
├── clients/                     # files the gateway serves to clients (the OpenCode plugin)
└── crates/
    ├── shared/                  # OpenAI wire types, shared with the CLI
    ├── session-core/            # chat-style UI substrate
    │   ├── src/                     SessionDriver trait, worker registry, db (chat_*
    │   │                            tables), Plait renderers (markdown + lumis-highlighted
    │   │                            code), SSE primitives, icons
    │   └── ui/ts/                   composer + scroll TS
    ├── aiplane-core/            # base: db, config, crypto, rbac, upstreams
    ├── aiplane-features/        # optional subsystems: rag, skills, comfyui, push, …
    ├── aiplane-agents/          # agent DB accessors, run sessions, visitor rates
    ├── aiplane-runtime/         # tool API + AppState/RamaState + chat driver
    ├── aiplane-tools/           # the tool implementations
    ├── aiplane-api/             # the server-rendered HTML pages
    ├── gateway/                 # the binary: router, proxy, api, main
    └── sandbox-runner/          # the sandboxed-tool execution service
```

### The gateway crate stack

AIplane is one binary assembled from three layered crates. This is **load
bearing for build speed**, not cosmetic: it used to be ~108k lines in one
compilation unit, so editing any file re-ran the whole frontend + codegen. Each
crate depends only on the ones beneath it.

```
gateway            bin + router/proxy/api/oidc     14.0k  ← thinnest, most-edited
   ├── aiplane-api     the /api/v0 JSON handlers   17.5k  ← siblings: neither
   └── aiplane-tools   the tool implementations    24.1k  ←   depends on the other
          └── aiplane-runtime  tool API + AppState/RamaState + chat driver  58.1k
                 ├── aiplane-features  RAG, skills, ComfyUI, push, geoip, …  25.5k  ← siblings
                 └── aiplane-agents    agent tables, run sessions, rates      7.4k  ←
                        └── aiplane-core      db, config, crypto, rbac, upstreams  45.6k
                               └── session-core  chat substrate, owner-agnostic   10.5k
```

Lines that must recompile after a one-line edit (as of #109): `gateway` 14k,
`aiplane-api` 32k, `aiplane-tools` 38k, `aiplane-runtime` 114k, `aiplane-agents`
121k, `aiplane-features` 139k, `aiplane-core` 192k, `session-core` 203k. The
gains are front-loaded on purpose: the layers that churn most are the cheapest
to rebuild. Before #109 the agent DB code sat in `aiplane-core`, so editing it
cost the full 191k.

`aiplane-api` and `aiplane-tools` are siblings: neither depends on the other, so
editing a page doesn't rebuild the tools and vice versa. `aiplane-features` and
`aiplane-agents` are siblings the same way.

Two rules keep it that way, and both are easy to break by accident:
1. **Put new code as high in the stack as it will go.** Something belongs in
   `aiplane-core` only if code below the feature layer genuinely needs it.
2. **Never reference upward.** `aiplane-features` and `aiplane-agents` must not
   name `AppState` or the tool registry; `aiplane-core` must not name a feature
   or an agent table's accessors; `session-core` must not name an agent, a
   visitor or a principal. One such reference collapses a layer.

The dependency half of rule 2 is checked: `workspace_crates_depend_only_down_the_stack`
(`crates/aiplane/tests/it/architecture.rs`) reads `cargo metadata` and fails on
any edge — normal, build or dev — from a crate to one above it or beside it,
naming the edge. A new crate fails it until it has a level there. Naming a type
across layers without a Cargo edge is impossible, so the edge check covers
rule 2 for types; it cannot see code that merely *belongs* higher (rule 1).

**When adding code, put it as high in the stack as it will go.** Something only
belongs in `aiplane-core` if code below the page layer actually needs it. Adding a
reference from `aiplane-core` to a page — or pushing a module downward for
convenience — makes every build slow again. See [`docs/architecture.md`](docs/architecture.md#crate-boundaries).

Inside `crates/aiplane-core/src/` (base layer):

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
    principal.rs run_chain.rs who acts (person / system principal) + the call chain
    usage/ limits/            metrics sink, spend-limit + quota enforcer
rama_server/
    session.rs                signed-cookie + sqlite session store; is_safe_return_to
    cors.rs                   the /v1 CORS layer
```

Inside `crates/aiplane-agents/src/` (beside `aiplane-features`): `db/` — the
accessors of the agent tables (`agents`, `system_principals`, `agent_audit`,
`embed_keys`, `visitor_sessions`, `a2a_contexts`, `agent_state`, `agent_tests`,
…; their DDL stays in `aiplane-core`'s one migration set), `db/run_sessions.rs`
(principal-owned conversations, the agent pause sweep, the inbox reads),
`db/inbound.rs` + `rates.rs` (the visitor rate gate over every inbound channel),
`notify_channels.rs` (the inbox's chat webhooks).

Inside `crates/aiplane-features/src/server/`: the optional subsystems — `rag/`,
`skills.rs`, `comfyui/`, `push/`, `github/`, `geoip/`, `typst.rs`, `image_gen.rs`,
`chat_attachments.rs`, `embeddings.rs`, `speech.rs`, `vad.rs` (silence trimming ahead of Whisper), `pdf.rs`, `ocr.rs`,
`search_settings.rs`, `document_canvas.rs`. None of them may name `AppState` or the
tool registry.

Inside `crates/aiplane-runtime/src/`:

```
openai_driver.rs          # SessionDriver impl: OpenAI streaming chat-completions
loop_guard.rs
server/
    tools/                    Tool trait, ToolContext, registry, catalog, runner,
                              MCP manager, sandbox client (impls: aiplane-tools;
                              echo + time stay here as the test fixtures)
    state.rs                  AppState
    comfyui_tool.rs           ComfyUI Tool/ToolSource impls + ComfyuiHandle
    side_call.rs              one-off model calls beside a conversation (title,
                              compaction, feedback, guard, classifier, judge,
                              assistant): metered, spend-limited, one exchange record
    scheduled/ webhooks.rs compaction.rs headless.rs   state-dependent workers
rama_server/
    state.rs                  RamaState (wraps AppState + sessions/usage/limits)
    auth.rs                   require_bearer for /v1/*
```

Inside `crates/aiplane-tools/src/`: one module per tool family (`fetch_url`,
`search_web`, `typst_render`, `document`, `rag`, `qr`, `netcheck`, …). Register a
new tool in the `ToolRegistry` that `gateway`'s `main.rs` builds, and grant it in
`[rbac]`. `tests/` holds the two test modules that need both the machinery and the
real tools (catalog grouping, `AppState` authorization).

Inside `crates/aiplane-api/src/`:

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

Inside `crates/aiplane/src/`:

```
main.rs                   # boot: config → state → SessionStore → OIDC → rama serve
rama_server/              # routing glue only:
    router.rs                 the rama::http::service::web::Router builder
    proxy.rs                  /v1/{models,chat/completions,audio/transcriptions,…}
    api.rs                    session-authed /api/v0/* JSON endpoints
    oidc_handlers.rs          /auth/{login,callback,logout}
    embed_cors.rs             CORS for /api/v0/embed/*, scoped to embed-key origins
    rag_api.rs sandbox_api.rs comfyui_api.rs
tests/it/                 # integration suite — builds the router, serves requests in-process
```

The SPA is built by `mise run build-web` into `target/frontend/build/` and
served from disk by `rama_server::spa` when `AIPLANE_STATIC_DIR` points there;
the Dockerfile COPYs that directory into the image. Nothing is `include_bytes!`'d
any more.

## Hard rules — do not violate without asking

1. **Minimize Cargo dependencies.** Every new crate added to `Cargo.toml` requires a one-line justification in [`docs/dependencies.md`](docs/dependencies.md). Prefer stdlib + what rama already brings in.
2. **All toolchain and build/test/lint commands go through `mise`.** No `Makefile`, no `justfile`, no ad-hoc shell scripts checked in. See [`docs/dev-workflow.md`](docs/dev-workflow.md). One deliberate exception: [`scripts/derive-version.sh`](scripts/derive-version.sh), because CI has to resolve the build's version in jobs that install no toolchain at all — `mise run version` is still the way a human calls it, and the script stays the only implementation.
3. **Thorough testing, test-first (TDD).** Write the failing test before the implementation — red, green, refactor. Every public function has unit tests; every rama route has an integration test (`crates/aiplane/tests/`); upstream LLMs are mocked with `wiremock` so tests run offline. The rama integration pattern is `router.serve(req).await` — no socket binding. **Style is Chicago / Classicist (state-based):** assert on observable results and real collaborators (in-memory SQLite via `:memory:`, `wiremock` upstreams, actual registries), not on interaction mocks. Reach for London-school behaviour-verification mocks only when a collaborator is genuinely un-fakeable (network you can't stand up, a clock, randomness) — and say so in a comment. Full strategy + required coverage in [`docs/testing.md`](docs/testing.md).
4. **Error messages are a product surface.** Use `thiserror` at API boundaries, `anyhow` + `.context()` internally, and write messages that say *what was happening, what went wrong, and what to do about it*. Full rules in [`docs/errors.md`](docs/errors.md).
5. **UI uses daisyUI component classes + Tailwind utilities, not hand-invented CSS.** Every visual element gets daisyUI semantic classes (`btn btn-primary`, `card card-body`, `alert alert-error`, `dropdown dropdown-end`, `badge badge-outline`, …) in the Svelte components under `web/src/`. Token utilities for bespoke layout (`bg-base-100`, `text-base-content/60`, `border-base-300`, `text-error`, …) plus standard Tailwind layout (`flex`, `mb-4`, `grid`). One-off ".tagline" / ".brand-mark" classes are not — drop the visual treatment or push daisyUI for the missing component. New server endpoints return JSON under `/api/v0`, never HTML; live turn updates ride the JSON-SSE protocol in `session_core::chat_json`. See [`docs/ui.md`](docs/ui.md).
6. **No comments explaining what code does** — names and types should already say that. Only comment *why* when it's non-obvious. Docs explain the system; code shows it.
7. **No backwards-compat shims** while the project is pre-1.0. We're starting fresh; if something needs to change, change it.
8. **Keep `README.md` deploy-current.** When you add or change a runtime knob, a config field, a host-package requirement, or a mise task on the deploy path, update `README.md` in the **same commit**. The README's "Quick start" + "Build + deploy" sections are the only thing a new operator reads before standing the stack up; if they don't reflect today's state, the next person wastes an hour. Rule 14 below makes documentation updates mandatory for every product change; this rule calls out the deployment entry points that drift most easily.
9. **User-visible strings go through the translation layer; everything else is English.** The product ships in six languages (en/de/fr/es/ru/zh). The Fluent catalogs under `crates/session-core/locales/<lang>/*.ftl` are canonical for BOTH halves — the server renders some strings itself (tool prompts, OAuth error pages, proxy errors) and the SPA renders the rest, and both must name a message identically or the two disagree in front of the user. Never hardcode a user-visible string in a Svelte component or a handler: add the key to all six `.ftl` files, run `mise run gen-locales`, and call `t('key')`. `build.rs` fails the build if a language is missing a key, and `i18n_drift` fails if the generated TS catalogs are stale.

   Everything a user does *not* see — log lines, comments, identifiers, test names — is English, always. Do not mix languages within a single message. Non-English text outside the catalogs is allowed only for *domain content that is intrinsically in another language*: the German business-letter fixture under `examples/typst-templates/letter/`, or a non-ASCII character used deliberately in a test (`'ß'` for a UTF-8 boundary case). Those are data, not app strings.
10. **Code principles — DRY, SOLID, KISS, Ubiquitous Language.** Default to the simplest thing that works (**KISS**) and don't repeat a fact or a shape in two places (**DRY** — extract a helper like `ChunkMeta::envelope` rather than copy a JSON literal twice). Follow **SOLID** where it pulls its weight: the `Tool` trait + `ToolRegistry` already give you open/closed extension (add a tool, don't touch the loop) and dependency inversion (drivers depend on the `SessionDriver` trait, not a concrete bin) — keep new code on that grain. Speak the codebase's **Ubiquitous Language** consistently in names, comments, and docs: `upstream` / `pool` / `backend`, `gateway-owned` vs `client-owned` tool calls, `turn` / `round`, `byte-dumb proxy`, `Acquired` in-flight guard. Don't coin a synonym for a term that already exists. These are guidance, not gates — if applying one would bloat or obscure, prefer the simpler code and note why.

11. **Reuse before you build — everywhere.** Before adding any selection, setting, table, endpoint, UI component, helper, client, cache, flow or text, find the existing mechanism that serves the same purpose and use or extend it. The brief and the commit name what was searched and found (or that nothing exists). A second mechanism for the same purpose needs the maintainer's explicit approval; review rejects it otherwise. *(Example: agents got their own pool/tier model selection next to the chat's models, aliases and automatic routes, plus their own capability list, approval flow and test chat — all later torn out.)*
12. **Facts only, nothing speculative.** What the product shows users or models about an existing resource comes from that resource's own data. Missing information is shown as missing and fixed at its source — never replaced by an invented title, description, greeting or default. No code without a real caller in the same change: no extension hooks "for later", no options nobody asked for, no shims (rule 7). *(Example: humanised tool ids and placeholder descriptions for knowledge bases; classifier interfaces with no implementation.)*
    **Public project content must not reference internal croit software.** Keep documentation and code comments relevant to AIplane's public behavior. Request priorities apply to chat and API calls from any application; describe them without internal-product examples.
13. **Design the data model once.** Unreleased work has one migration per change set, amended in place (with its schema fixture and checksum); never a migration per commit, and never a migration that only cleans up data that was never released.
14. **Keep product documentation current in the same change.** Every new feature and every change to existing product behavior MUST update the affected user, administrator, operator, API, configuration and deployment documentation in the same change and commit. Before implementation, find the relevant pages from the documentation index and identify which behavior they describe; after implementation, verify every edited claim against the current code and UI. Update screenshots, navigation, endpoint/settings coverage and generated Markdown/LLM exports when the change affects them. Do not copy an existing description without checking it against the implementation. If a change is entirely internal and leaves no documented behavior or contract different, record that conclusion in the change review rather than silently skipping the documentation check. A feature is not complete until its documentation is accurate and included with its implementation.

## Daily workflow

After `mise install` (one time):

```bash
# One terminal during UI work: public Vite/HMR on :8080 proxies every
# dynamic route to the private debug gateway on :8081.
mise run dev
mise run dev-served        # production-shaped compiled SPA, no HMR

# Other day-to-day tasks
mise run dev-build         # debug build only (target/debug/aiplane), ~2 s incremental
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
| Codex CLI / OpenAI Responses compatibility (`/v1/responses`) | [`docs/codex.md`](docs/codex.md) |
| Multi-provider routing, load balancing, health checks | [`docs/upstreams.md`](docs/upstreams.md) |
| Tool registry, role→tool mapping, execution loop | [`docs/tools-rbac.md`](docs/tools-rbac.md) |
| Which tools exist, their gates and toggle keys | [`docs/tools-inventory.md`](docs/tools-inventory.md) |
| Browser control — the Chrome extension, its trust boundary, the store | [`docs/browser-control.md`](docs/browser-control.md) |
| Web UI — the SvelteKit SPA, the JSON API, the SSE protocol | [`docs/ui.md`](docs/ui.md) |
| Testing strategy and required coverage | [`docs/testing.md`](docs/testing.md) |
| Versioning + how a release is cut | [`docs/releases.md`](docs/releases.md) |
| Running on Kubernetes (the Helm chart) | [`docs/kubernetes.md`](docs/kubernetes.md) |
| Error handling — types, messages, OpenAI mapping | [`docs/errors.md`](docs/errors.md) |

## Working agreement for agents

**Before building**
- **Design first, confirmed, then build — never fan out on an unsettled base.** Anything touching more than one file or concept gets a short design the maintainer confirms before code: the existing mechanisms it reuses (rule 11), its data model (rule 13), where its UI takes its data from, and its user flow. A requirement marked "coarse", "open" or "refine later" leads to a question, not to code. Build one vertical slice and show it before adding the next layer.
- **Check requirements against what exists — your own included.** If a requirement introduces a concept next to an existing one, or contradicts how the product already works, raise it before implementing it. Issue text is not proof that a design is right.
- **Complex or nonstandard UI needs an approved mockup or walkthrough with real data** before implementation, and the coordinator clicks through the result in a browser before reporting it done. For a simple dialog or form that follows an established app pattern, implement it directly without requesting a mockup approval. Present a concise ASCII mockup for complex, nonstandard UI or a UI design that materially changes behavior; do not substitute a prose-only description.
- **Decisions that are the maintainer's are asked first, not reported afterwards:** changes to rights, privacy, data exposure, billing/metering, or any visible change of behaviour. Present the options with a recommendation.
- **Design docs name their shared mechanisms.** A change that needs a cross-cutting building block lists, in a **"Shared mechanisms"** section, the existing ones it must use (`net_guard` + the pinned client for outbound URLs, `read_capped` / `read_body_capped` / `BodyLimitLayer` for bodies, the typed `AgentSpec`, `GrantedToolSource`, the chat's model list, the capability catalog, `chat_turn_suspensions` for approvals, …) and any new one it introduces. A new cross-cutting block lands, with its tests, **before** the features that use it. The architecture tests (`docs/testing.md` → "Architecture tests") enforce the existing ones; a new one gets a test there too.

**While building**
- **Small, short-lived work packages.** At most three branches in flight, each merged within a day. Parallel agents only on disjoint files (schema, catalogs, generated files and shared docs are shared files — work on them runs one after another). Generated files are regenerated on merge, never hand-merged.
- **One commit is complete.** Code, tests, docs, i18n keys, generated artifacts (locale TS, checksums, inventory fixtures) and formatting go into the same commit. No "rustfmt", "fix previous commit" or "Docs:" follow-ups — amend or squash on the branch. The pre-commit hook rejects unformatted Rust (`mise run fmt-staged`).
- **Docs describe the current system, not its history.** What is and why; no "What #N built", no issue numbers, no "no longer / instead of / now". History lives in git and the issues. Keep a doc readable — split by topic rather than append.
- **When you discover a missing piece** — an undocumented invariant, a non-obvious gotcha — add it to the relevant doc. Don't rely on conversation history.
- **Tests live next to the code.** Unit tests in `#[cfg(test)] mod tests`, integration tests in `crates/aiplane/tests/`. Every test input and loop is finite.
- **Verify cheaply, then once for real.** Compiling dominates this workspace: a full `mise run verify` is ~20 minutes, of which ~19 are linking test binaries. Iterate with `mise run test-crate <crate>` (seconds), then run the full gate **once**, at the end, after every fix you already know about is in. Don't kill a cargo process to unstick a parallel mise task — they share one `target/` lock. See [`docs/dev-workflow.md`](docs/dev-workflow.md) → "The feedback ladder".
- **Review once per branch, before the merge; fix findings before the merge.** Reviews cost real tokens and time, so they are spent where they find things.
  - **When:** once, on the whole branch diff, after every change you already know about is in. Follow-up work lands on the branch first and is reviewed with it, not in a review of its own.
  - **Fixes:** fixing what a review found does not trigger another review — tests and `mise run verify` cover the fix. Re-review only when the fix changes the design.
  - **Effort by risk:** `/code-review` at medium by default; high only for a large diff (roughly >2k lines) or one touching rights, money or data exposure; low is enough for docs, catalogs and compose/config.
  - **Security:** `/security-review` as well, once per branch, only when the change touches anything reachable without login (`/hooks`, `/a2a`, `/api/v0/embed`, OAuth callbacks, `/setup`, the public probes), rights, grants, tokens or sharing, a URL someone other than the operator chose, or personal data.
  - Merged is not done; reviewed and verified is.
- **Evidence before hypotheses.** On a failure or crash, read the logs and reports first (for a memory crash: the newest JetsamEvent, see docs/dev-workflow.md), then name a cause.

**Working with agents and the maintainer's machine**
- **Delegating work?** Use the brief template in [`docs/dev-workflow.md`](docs/dev-workflow.md#delegating-work-to-an-agent). An agent commits at every green state and always ends with a report; the coordinator checks the result (code review, and in a browser for UI) before calling it done.
- **The maintainer's environment is theirs.** Use their browser only in a tab of your own and never resize the window (emulate viewports instead). Restart their dev server only to load a merged state, and say so. Don't run anything that disturbs a running session (e.g. commands that write into a directory the dev server watches). Never change their database without a backup first, and state what changed.
- **Never cut a release unprompted.** A release is a git tag and nothing else (see [`docs/releases.md`](docs/releases.md)), and pushing one moves `:production`, which every auto-updating installation picks up that night. "Ship it" / "finish X" is not a release request; wait until someone asks for one in so many words.
- **If a hard rule is in your way**, surface it to the user. Don't quietly bypass.
