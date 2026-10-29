# Dev workflow

## Toolchain

Everything is pinned in `mise.toml`. Run `mise install` once after cloning. It installs:

- The **Rust** toolchain pinned to `1.95` (with the `rustfmt`, `clippy`, and `cargo` components).
- **`cargo-binstall`** — used behind mise's `cargo:` backend to install Rust binaries quickly (prebuilt when available, source build as fallback).
- **`sccache`** — an rustc wrapper for compilation caching. It's installed but **OFF by default** (no `RUSTC_WRAPPER` is set). Opt in locally with `RUSTC_WRAPPER=sccache` if you want it.
- **Node 24** — builds the SvelteKit SPA in `web/` and runs its `node:test` suites (`test-web`, `e2e`). Build/test only; nothing Node-shaped ships in the container image.
- **`typst` 0.15.0** — the CLI backing the `typst_<template>` tools. The `fetch-typst-cli` task copies mise's installed binary into `target/release/typst` so the release build and runtime image pick it up through the same artifact pipeline as the gateway binary.

We **do not** check in a `rust-toolchain.toml`; mise is the single source of truth.

## Daily commands

The Rust binary and the UI build separately: `cargo build` needs no Node, and the SPA build needs no `cargo`. `mise run dev` starts both development processes behind one public origin: Vite/HMR on `:8080`, with the Rust gateway private on `:8081`. UI work does not rebuild Rust at all; see [the SPA loop](#the-sveltekit-spa-web) below.

| Goal | Command |
|---|---|
| Run the complete HMR development stack on :8080, with regular OIDC login | `mise run dev` |
| Run the production-shaped compiled SPA on :8080 | `mise run dev-served` |
| Run only the Rust gateway | `mise run dev-gateway` |
| Run a stub gateway for UI debugging (seeded session, mock LLM) | `mise run dev-ui` |
| Build the AIplane debug binary (no run) | `mise run dev-build` |
| Install `web/node_modules` (only when the lockfile changed) | `mise run web-install` |
| Build the SvelteKit SPA into `target/frontend/build/` | `mise run build-web` |
| svelte-check (TS + a11y diagnostics) on the SPA | `mise run check-web` |
| Unit-test the SPA's pure TypeScript (`node --test`) | `mise run test-web` |
| Fast type-check across the workspace | `mise run check` |
| **Release** build (slow, for deploys) | `mise run build` |
| Tests — one crate (the iteration loop) | `mise run test-crate <crate> [filter]` |
| Tests — whole workspace | `mise run test` |
| Tests with stdout visible | `mise run test-nocapture` |
| Lint — one crate | `mise run lint-crate <crate>` |
| Lint (clippy `-D warnings` + `fmt --check` + svelte-check) | `mise run lint` |
| Apply Rust formatting | `mise run fmt` |
| The pre-push gate (lint + Rust tests + SPA checks and build) | `mise run verify` |
| Reclaim `target/` (stale artifacts) | `mise run sweep-target [days]` |
| Everything CI runs (lint + tests + release build + SPA build) | `mise run ci` |
| Scan the whole git history for committed secrets | `mise run secrets` |
| Scan only the staged diff for secrets | `mise run secrets-staged` |
| rustfmt --check the staged Rust files | `mise run fmt-staged` |
| Enable the version-controlled git hooks | `mise run setup-hooks` |

**Debug vs release.** `mise run build` (release) takes ~12 s cold-incremental and ~70 s from clean — only use it when you actually want optimised output (deploys, perf measurement). For day-to-day iteration (running locally, screenshotting pages, smoke-testing changes) use `mise run dev` or `mise run dev-build`; those produce a debug binary in ~2 s incremental (vs ~11 s for a release build). Runtime perf is identical for any UX you'd interact with; only synthetic benchmarks notice the difference.

`mise run setup-hooks` points `core.hooksPath` at `.githooks/`. Run it once per clone — it installs three hooks:

- **pre-commit** — gitleaks over the staged diff (~100 ms), so a credential can't reach local history in the first place, and `rustfmt --check` over the staged Rust files, so formatting lands in the commit it belongs to rather than a follow-up.
- **pre-push** — the secret scan again over the *full* history, then lint + tests. Push is the last moment before something becomes public.
- **commit-msg** — rejects `Co-authored-by:` / `Claude-*:` attribution trailers.

**On secret scanning.** An internal bearer token was once committed to this public repo and pushed. GitHub's own secret scanning cannot catch that class: its free tier matches only ~200 *provider* formats, and the generic "HTTP Bearer Token" pattern that would have matched sits behind the paid Secret Protection tier. Worse, gitleaks' *default* rules miss it too — `generic-api-key` captures the value after `=` with `[\w.=-]+`, which stops at the space in `Authorization = "Bearer <token>"` and only sees the 6-char literal `Bearer`. `.gitleaks.toml` therefore adds explicit `http-bearer-token` and `http-basic-auth` rules. If you touch that config, re-check both directions: real tokens in `Authorization` headers must be caught, and placeholders (`Bearer {token}`, `Bearer $VAR`) must not be.

CI runs the same scan in a dedicated `secret scan` job with `fetch-depth: 0`, which is the backstop for pushes made with `--no-verify` or from a clone where `setup-hooks` was never run.

Credentials belong in `mise.local.toml` or the DB (sealed with `AIPLANE_ENCRYPTION_KEY` when set, otherwise a key derived from `AIPLANE_SESSION_KEY`) — all gitignored or outside the tree. Tool configs that carry tokens (`.codex/`, editor/agent configs) should live in `$HOME`, not in the repo.

Anything not covered: add a task to `mise.toml` rather than typing the raw command into a script. Discoverability matters.

## The feedback ladder — don't run the full gate to check one change

Measured on an M-series laptop, editing `aiplane-core` (the root of the crate
graph — everything above it rebuilds), before and after the `target/` cleanup
and profile trim described below:

| Operation | Before | After |
|---|---|---|
| `ls -f target/debug/deps` | **66 s** | **0.065 s** |
| touch `aiplane-core` → `gateway` test binaries linked | **414 s** | **14–18 s** |
| touch `aiplane-core` → `mise run test` (all 2365 tests) | **~20 min** | **40 s** |
| `target/` on disk | 327 GB, 1.16M files in `deps` | ~30 GB, ~12 k files |

The shape to internalise: **compiling and linking dominates; running tests is
noise** (2365 tests execute in 13 s). Clippy (`dev`), nextest (`test`) and the
release build are three *different* profiles, so `mise run ci` compiles the
workspace three times over — that's why it's still ~20 minutes even now, and
why it belongs at the end of a change set exactly once, not in the loop.

So climb the ladder, and only step up when the rung below is green:

```bash
mise run check                      # ~seconds — does it type-check at all?
mise run test-crate aiplane-api     # the crate you're editing (+ optional filter)
mise run lint-crate aiplane-api     # clippy for that crate
mise run verify                     # ONCE, before pushing: lint + all tests
```

`mise run ci` adds the release build on top of `verify`; you only need it if
you're actually shipping a binary — CI builds it on every push anyway. The
pre-push hook runs `verify`, so a normal `git push` is already the gate.

Two habits that cost more than any tooling change:

- **Running the full gate more than once per change set.** Fix everything you
  know about first — a `verify` started before a fix lands is 20 minutes you
  pay twice.
- **Killing a cargo process to "unstick" a parallel task.** They share one
  `target/` and one lock; `Blocking waiting for file lock` is normal, and
  killing one of them fails its sibling task and buys another full run.

When worktrees share `CARGO_TARGET_DIR`, a cached `session-core` artifact can
omit locale files added in the other worktree. A response containing a Fluent
key rather than its text indicates a missing compiled message. After checking
that the key exists in all six catalogs, touch
`crates/session-core/src/i18n.rs` and rerun the affected mise test task to rebuild
the embedded catalogs. Do not edit the translation to compensate for a stale
artifact.

## Delegating work to an agent

Parallel agents are how the agent builder grew three capped body
readers, five IP classifiers and ~6k lines of agent code in `aiplane-core`:
each brief said *what* to build and nothing about what already existed or where
it belonged. A brief for delegated work carries the following, filled in — not
just "see AGENTS.md":

```markdown
## Task
<issue / goal, the acceptance criteria, and what is out of scope>

## Confirmed design
<link to the confirmed design: reused mechanisms, data model, UI data
sources and user flow. No confirmed design → don't delegate yet.>

## Before writing code
- Search for the existing mechanism first and name what you found (or that
  you found nothing) — backend helpers, but equally product concepts: the
  model list, the capability catalog, approvals (`chat_turn_suspensions`),
  sharing, settings, UI components, catalog strings.
  `rg -n '<verb>|<noun>' crates/ web/src/`, the "Shared mechanisms" of the
  design doc, docs/architecture.md → "Crate boundaries". Reuse or extend; a
  second mechanism is reported back for approval, never improvised
  (AGENTS.md rule 11).
- Facts only (AGENTS.md rule 12): show a resource's own data; missing data is
  shown as missing, never invented. No code without a caller in this change.
- Crate placement: say which crate the new code goes in and why it cannot go
  higher (AGENTS.md → "The gateway crate stack").
- A requirement that contradicts the existing product, or a design gap you
  hit, is reported back — not resolved by guessing.

## Invariants to keep
- Outbound URLs the operator does not configure (a user's, a model's, an
  agent owner's, a browser's push endpoint, the MCP OAuth endpoints): the
  net_guard-pinned client (`outbound_guard::get` / `get_with` / `pin`, with
  the caller's `Policy`); operator backends: `AppState::http`.
- Bodies: `read_body_capped` / `read_json_capped` / `read_capped`; whole-body
  drains only behind `BodyLimitLayer`.
- Agent specs: the typed `AgentSpec`, never the JSON.
- Tool calls: through `GrantedToolSource` / `DiscoverableToolSource`.
- <anything specific to this task>
The architecture tests check the first four (docs/testing.md).

## Tests
- Test-first. Size-probe inputs are finite (cap + 1, or a bounded "endless"
  stream such as `common::endless_body()`), never an unbounded stream.

## Build discipline
- One Rust-building agent at a time on this machine; run every cargo/mise
  command as `CARGO_BUILD_JOBS=4 mise run …`.
- Climb the feedback ladder (check → test-crate → lint-crate) and run
  `mise run verify` once, at the end.
- Commits per item, each complete (code, tests, docs, i18n, generated
  files, formatting — AGENTS.md "One commit is complete"); commit at every
  green state so nothing is lost if you stop; never `--no-verify`; never push
  or merge unless asked.
- Leave the maintainer's environment alone: their dev server, browser window
  and database (AGENTS.md "The maintainer's environment is theirs").

## Report
- Always end with a report, also when stopping early: what changed and where,
  the mechanisms reused (and any second one you'd need, for approval),
  behaviour changes the maintainer should decide on, anything left undone,
  and the verify result.
```

Why the build discipline: two agents building this workspace at once each run a
full parallel `rustc` fan-out plus a linker per test binary, and the machine
runs out of memory long before either finishes — see "A memory crash: read the
newest JetsamEvent report first" below. `CARGO_BUILD_JOBS=4` keeps one build's
peak to a level the rest of the machine survives.

### Why the build profiles look like that

The root `Cargo.toml` sets `debug = "line-tables-only"` for the workspace and
`debug = false` for every dependency: the 17 test binaries each dragged the
full DWARF of every dependency behind them.

Honest accounting, since both changes landed together: the debuginfo trim is
worth maybe 15% of the incremental rebuild on its own (414 s → 353 s measured),
and it halves what a build writes to disk. The 20× came from the `target/`
cleanup below. The profile still earns its place — CI was already dropping
debuginfo via `CARGO_PROFILE_*`, so only local builds were paying full DWARF,
and less written per build is less to clean up later.

What you keep: function names **and** `file:line` in every panic and backtrace,
for gateway code. What you give up: stepping through code in a debugger with
locals. If you need that for one crate, build it with
`RUSTFLAGS="-C debuginfo=2"`. CI goes one step further and sets
`CARGO_PROFILE_DEV_DEBUG=0` / `CARGO_PROFILE_TEST_DEBUG=0` (env beats the
manifest) because the runner has a disk quota, not a debugger.

### `target/` hygiene — the slowest thing in this repo was the build directory

Cargo has **no garbage collector**. Every fingerprint change — a profile flag, a
feature, a dependency bump, a branch switch, a toolchain update — writes a
complete new artifact set and keeps the old one *forever*. On macOS the dev
default is `split-debuginfo=unpacked`, so each unit's object files stay in
`deps/` too (that's where the debug info lives), and a dev build emits up to
`codegen-units` objects per crate.

Measured here before the cleanup:

| | |
|---|---|
| `target/` | **327 GB** |
| files in `target/debug/deps` | **1,165,431** — of which **1,154,133 (99%) superseded garbage** |
| `ls -f target/debug/deps` | **66 seconds** (a bare listing, no stat, no sort) |
| stale copies of the 260 MB `gateway` test binary | **19** |

Cleaning it out took the incremental loop from 414 s to 14 s and the full test
run from ~20 min to 40 s. Nothing about the code changed.

That last row is the point: cargo scans that directory on **every** invocation,
so a million dead files taxed every `cargo check`, every test run, every build.
Two OS daemons piled on — Spotlight (`mds` + `mds_stores` + `mdworker` at ~60%
CPU combined, mid-build, with 8 k `.rmeta` files already indexed) and
`syspolicyd` at ~30% (Gatekeeper validating every freshly linked binary).

So:

```bash
mise run sweep-target        # delete artifacts nothing rebuilt in 14 days
mise run sweep-target 3      # more aggressive
```

It touches only cargo's own artifact directories (`{debug,release}/{deps,build,
incremental,.fingerprint}`) — never the top of `target/release`, which also
holds the `typst` and `libpdfium.so` binaries the fetch tasks put there and no
rebuild would restore. The pre-push hook kicks the same sweep off **detached, at
most weekly**, so this can't silently rot again; `target/.last-sweep` is the
marker it checks.

`mise run setup-hooks` also drops a `.metadata_never_index` file into `target/`,
which is what Spotlight looks for to skip a tree. Run it once per clone.

Caveat on the sweep: it goes by mtime, and a *valid* artifact nothing has
rebuilt in 14 days looks identical to a dead one, so a sweep can cost you a
recompile of long-untouched units. That's the trade — a million-file directory
costs more, every day.

For one crate specifically, `cargo clean -p <crate>` is precise. A full
`cargo clean` costs a cold rebuild (**186 minutes** on this workspace — 621
units, including ~116 tree-sitter grammars and two C++ builds), so it is a last
resort, not routine hygiene.

### Optional, per-developer: sccache + incremental

Not in the repo, because it's a machine-level choice: a user-global
`~/.cargo/config.toml` can add `[build] rustc-wrapper = "<path to sccache>"`
(sccache is already in the mise toolset) and `incremental = true` on the `dev`
and `test` profiles. They do **not** conflict — cargo compiles only *workspace*
crates incrementally and never registry dependencies, so incremental owns the
edit loop while sccache owns the dependency graph across cold builds, branch
switches and profile changes. Measured here, one-line edit in `aiplane-api`
then `cargo nextest run --workspace`: **102 s → 59 s**. Give sccache a cache
big enough for this dependency graph (100 GiB in its own config file); at the
10 GiB default it evicted as fast as it wrote and measured a 0% hit rate.

CI deliberately doesn't use sccache — `Swatinem/rust-cache` caches the registry
and `target/` there instead.

## Layout while developing

`mise run dev` starts Vite on public `127.0.0.1:8080` and `cargo run --package aiplane` on private `127.0.0.1:8081`. Vite owns the browser origin and proxies every gateway-owned route, including `/chat/attachment/*`, OAuth callbacks, liveness endpoints, `/api`, `/v1`, and `/auth`. On startup the binary:

- binds the private address supplied by the task (`127.0.0.1:8081`);
- opens the SQLite database at `$AIPLANE_DB_PATH` (default `gateway.sqlite`) and runs migrations;
- applies the stored operator settings over the built-in defaults;
- builds the upstream registry from the database and spawns the health probes;
- builds the OIDC client if a provider is configured, otherwise starts without login.

There is no config file to copy. A fresh database boots into the setup wizard:

```bash
mise run dev
# then open http://localhost:8080 — it redirects to /setup
```

If you want a working UI without an identity provider, `mise run dev-ui` boots
a stub gateway with mock backends and a pre-seeded session instead.

`mise run dev-served` is the production-shaped alternative: it builds `target/frontend/build` and serves that directory directly from the debug gateway on `http://localhost:8080`, without HMR. `mise run dev-gateway` exposes the Rust process alone and honors the normal `IP` / `PORT` environment variables.

**Login works in every worktree.** `mise run dev` is the regular path: it serves your real
`gateway.sqlite`, whose OIDC provider is configured there, so the browser login is the
production login. A linked worktree has no database of its own, so on its first run the
gateway task copies the main worktree's database with SQLite's online backup (safe while
the main server is running) and reuses its `.aiplane-dev-session-key`, which keeps the
copied database's sealed secrets readable. The copy is a snapshot; each worktree then
migrates and writes its own database. An existing worktree database is never overwritten —
delete `gateway.sqlite*` in the worktree to reseed, or set `AIPLANE_DEV_SEED=0` to keep a
fresh `/setup` database. If the main database is ahead of the checkout's own migrations
(the branch predates one of them), the seed is skipped with that reason rather than leaving
a database the binary would refuse to open; merge the main branch or start from `/setup`.

## Environment

Env config is layered through mise, not a `.env` file:

- **`mise.toml` `[env]`** holds the non-secret defaults committed to the repo (`RUST_BACKTRACE=1`, `RUST_LOG=info,aiplane=debug,aiplane_core=debug,aiplane_features=debug,aiplane_agents=debug,aiplane_runtime=debug,aiplane_tools=debug,aiplane_api=debug`).
- **`mise.local.toml` `[env]`** holds secrets and machine-local overrides — it is **gitignored**. This is where local dev keys go: `AIPLANE_SESSION_KEY`, `AIPLANE_ENCRYPTION_KEY`, provider keys (`OPENAI_API_KEY`, `ZAI_API_KEY`, …), etc.

Web-search settings are **not** environment variables any more. Provider, SearXNG URL, Brave API key, and Tavily API key live in the database and are set under **Web search** on `/admin/settings?tab=web-search` (keys sealed at rest like every other gateway secret). `SEARCH_PROVIDER`, `SEARXNG_URL`, and `BRAVE_SEARCH_API_KEY` are still read at boot to fill settings that are still empty; there is no Tavily environment-variable import. Once a setting is present in the database, its environment variable is ignored.

Secrets live in the database, sealed at rest, and are entered in the admin UI. A backend may instead name an environment variable to read its key from (`api_key_env = "GPU01_KEY"`), which is why provider keys still belong in `mise.local.toml`. `$AIPLANE_SESSION_KEY` is read directly and is mandatory.

Which env vars each subsystem needs is documented in `docs/auth.md` (OIDC) and `docs/upstreams.md` (provider keys).

### `RUST_LOG` and the crate split

A tracing target is the *crate* a span or event was emitted from, so AIplane
now emits under the following crate targets:

| target | covers |
|---|---|
| `aiplane` | router, `/v1` proxy, `/api/v0`, OIDC handlers, `main` |
| `aiplane_core` | config, DB, crypto, RBAC, upstreams, auth, sessions |
| `aiplane_features` | RAG, skills, ComfyUI, push, geoip, typst discovery, attachments, PDF/OCR/speech |
| `aiplane_agents` | agent tables, visitor rates and inbox channel notifications |
| `aiplane_runtime` | the tool registry/catalog/runner, `AppState`, the chat driver, scheduler, webhooks |
| `aiplane_tools` | the tool implementations (`fetch_url`, `search_web`, typst, document, …) |
| `aiplane_api` | the `/api/v0` JSON handlers, including the chat event stream |

A bare `RUST_LOG=info,aiplane=debug` therefore only raises the level for the
routing glue — page and tool logs stay at `info`. The committed defaults in
`mise.toml`, `Dockerfile`, `deploy/compose.example.yml`, and
`deploy/quadlet/aiplane.container` all name these targets explicitly.

**If you run AIplane from your own env or unit file, update `RUST_LOG` when
you deploy this change** — an unchanged filter silently drops page and tool logs
to whatever the global default is. Note the underscores: crate names are
normalised, so it's `aiplane_core`, not `aiplane-core`.

`AIPLANE_SESSION_KEY` — 64 hex chars (32 bytes) for the session-cookie HMAC. **AIplane refuses to boot without it.** It used to fall back to an ephemeral per-process key, which quietly logged every user out on each restart *and* left every sealed secret in the DB unreadable; that failure was invisible until it had already cost data, so it is now a hard startup error carrying the `openssl rand -hex 32` line to fix it.

`mise run dev` handles this for you: it generates `.aiplane-dev-session-key` (gitignored, 0600) on first run and reuses it forever after. That is also a fix for local development — with the old ephemeral key, backend API keys and connector secrets stored in your local `gateway.sqlite` were silently unreadable after every restart.

`AIPLANE_DATA_DIR` — root for everything AIplane *writes*: the SQLite database (`<data_dir>/gateway.sqlite`) and the RAG store (`<data_dir>/data/rag`). Unset it stays empty, so a `cargo run` in a checkout writes `./gateway.sqlite` and `./data/rag` exactly as before; the container image sets it to the mounted volume, which is what lets a deployment persist state with no config file. Read-only paths (typst templates, skills bundles) deliberately do *not* hang off it — they ship in the image's read-only layers.

## Debugging the UI

Every authed surface — the SPA's screens and the `/api/v0/*` JSON routes behind them — is gated by OIDC, which makes ad-hoc browser debugging (browser automation, devtools, screenshotting bugs) annoying: you'd otherwise need a full OIDC provider wired up just to *see* a page. The `dev-ui` mise task short-circuits that:

```bash
mise run dev-ui
```

The mise task builds the SPA and sets `AIPLANE_STATIC_DIR`.

This runs the `dev_ui` example (`crates/aiplane/examples/dev_ui.rs`), which boots the real rama gateway on `127.0.0.1:8080` against:

- an **in-memory SQLite**;
- the existing **scheduled-action worker**, so authored cron jobs fire against
  the same mock model and durable run-history path used by the UI;
- an in-process **`wiremock` chat pool** that serves `GET /models` (advertising `demo-model` + `demo-model-pro`) and `POST /chat/completions` (a streaming variant emitting two SSE deltas + `[DONE]`, plus non-streaming and feedback-extraction variants);
- a published agent (`website-helper`, on the `chat` pool) with a fixed embed key for `http://localhost:8000`, so the embed widget example works: `python3 -m http.server 8000 -d examples/embed`, then open `http://localhost:8000/` (see [`embed.md`](embed.md));
- a **scripted agent model** on the chat mock, so every agent flow runs end to end: it answers only requests that offer an agent's synthetic tools (and the test-case judge, always "passed") and acts on the visitor's last message — an email address sets `email` and requests a verification code, "invoice"/"billing" sets `issue` and calls `forward_request`, "human"/"person" calls `request_human`, and a routed sub-agent calls `finish`. `website-helper` is wired for all of it: an `otp` verifier on a mock **Demo ERP** MCP connector (`erp`, agent scope) that accepts the code `123456`, a `billing` route behind that verifier to the published sub-agent `billing-helper`, and a `staff` handoff route that `eng@example.com` answers through a `respond` share (log in with the printed non-admin cookie to answer it from `/inbox`);
- an in-process **`wiremock` transcription pool** that serves `GET /models` (advertising `demo-whisper` + `demo-whisper-large`) and `POST /audio/transcriptions` (a stubbed JSON response);
- a pre-seeded **`dev@example.com`** user with an `admin` role (every model / tool / skill granted), the `examples/demo-skills` bundle loaded, and representative demo data (a finished chat conversation, scheduled actions, RAG collections, and an MCP connector catalog) so the screens render populated.

The example maps the debug `/__dev/session` user's `admin` group to the admin
role, so that URL shows the same setup hints as the printed admin seed cookie.
It uses the same built-in tool registry as the real binary and loads
the example Typst templates. Its sandbox client uses the same registry and
`browse_page` action catalogue as production; the local mock refuses `/run`
with an explicit fixture error and does not execute containers. This supports
access-rule UI checks, not sandbox execution verification. The exact prompt
`Agent action approval probe` produces a `browse_page` navigation call for
checking action revocation through the real Inbox flow. Tools whose operator
backend is missing (GeoIP) appear as disabled rows with setup links. A retained
SQLite connection keeps the fixture database alive while the server is idle.
The mock has no file storage or RAG indexer, so dependent tools, including the
tools for its mock image pool, are also shown disabled. It initializes the
same Web Push sender as production so device notification settings can be
tested. Unavailable tools are visible for layout testing, while unregistered
placeholder tools are absent
from the model's tool list.

The example imports that same feature configuration into its in-memory settings
rows before serving. Consequently, saving an unrelated `/admin/settings`
section and rebuilding the runtime surface cannot make the demo skills or
ComfyUI catalog disappear halfway through a browser run.

It's a local-only convenience — not a test target, and not run by CI.

It prints the signed session cookie on startup, e.g.:

```
dev gateway listening on http://127.0.0.1:8080
seed cookie (paste into playwright / curl):
    id=03aab419…
```

### From curl

Paste the cookie to reach any authed endpoint:

```bash
COOKIE='id=…'
curl -b "$COOKIE" http://127.0.0.1:8080/api/v0/me
curl -b "$COOKIE" http://127.0.0.1:8080/api/v0/chat/sessions
```

A chat turn is two calls: `POST /api/v0/chat/sessions/{id}/messages` to submit (create a session first with `POST /api/v0/chat/sessions`), and `GET /api/v0/chat/sessions/{id}/events` to watch the reply arrive as JSON-SSE events. The wiremock backend resolves every prompt in ~no time, so the whole submit → stream → finalize cycle is observable without flake.

### Signing in to the real `mise run dev` gateway

`dev-ui` is its own server with mock backends. To poke at an authed page of the *real* dev gateway (your actual `./gateway.sqlite`), the same trick exists as debug-only endpoints (`rama_server::dev_seed`): `GET /__dev/session` signs you in as the fixture user `alice@example.com` without touching anything, and `GET /__dev/seed-session` additionally resets her tokens to the canonical three — the same pair the e2e suite drives (`mise run e2e`):

```bash
curl -si http://127.0.0.1:8080/__dev/session | grep -i set-cookie   # id=…
```

### From a browser / automation

Open any origin page (e.g. `http://127.0.0.1:8080/login`), then inject the cookie via devtools (`document.cookie = 'id=…; Path=/'`) or your automation tool's cookie API, and navigate to the route you want. From there the SPA runs against the mock backend with real streaming.

The repo's README/docs screenshots are produced this way — see the `take-screenshots` helper under `.claude/skills/take-screenshots/`, which drives Playwright with the seeded cookie.

### Why a seeded session rather than patching out auth?

Every code path under test (cookie parsing, session lookup, RBAC, the session gate, the SSE stream, …) is the same one production runs. The only things faked are the upstream LLM and the OIDC handoff.

## The SvelteKit SPA (`web/`)

The UI has **two** development modes.

**Hot-reload mode — the everyday loop.** One terminal:

```bash
mise run dev       # public Vite/HMR on :8080, private Rust gateway on :8081
```

Open `http://localhost:8080`. Vite proxies the complete dynamic surface to AIplane on :8081 (`web/vite.config.ts`), so sessions, OIDC callbacks, attachments, downloads, SSE, and API calls share the production-shaped browser origin. A Svelte-file save re-renders in well under a second with **no Rust rebuild**.

Do not run `mise run check-web` (or any `svelte-kit sync`) while this server is up: it can replace the generated dev modules underneath Vite and leave the next page load blank with `504 Outdated Optimize Dep`. Restart `mise run dev` afterwards; if the cache stays stale, `rm -rf web/node_modules/.vite` first.

**Served mode — what production looks like.** `mise run dev-served` builds the SPA and lets AIplane serve it directly at `http://localhost:8080`. Use this to check the built artefact, cache headers and history fallback. No Node runs in production: the container image `COPY`s the built `target/frontend/build/` directory in and the Rust binary serves it (`crates/aiplane/src/rama_server/spa.rs`). `dev-served` and `dev-ui` also run `mise run stage-extension`, which packages the Chrome extension into `target/frontend/build/downloads/` — the download `/tools/browser` links to — because `build-web` recreates that directory and would otherwise leave the link dead.

AIplane serves its OpenAPI 3.1 contract at `GET /openapi.json`. The routes are read from `router.rs`; each one's credential, request, responses and errors come from its declaration in `crates/aiplane/src/rama_server/openapi/`, with schemas derived (`schemars`) from the handler's own wire types. A new `/api/v0` route fails the build's tests until it is declared there. The production container carries no detached spec, and the client in `web/src/lib/api.ts` remains hand-written against the backend wire types. See [`ui.md`](ui.md#the-json-api-and-its-contract).

Chat streams over the JSON-SSE event protocol (`session_core::chat_json` ↔ `web/src/lib/chat-protocol.ts`): the composer posts `POST /api/v0/chat/sessions/{id}/messages` and the reply arrives on `GET …/events` as `snapshot` / `turn_delta` / `tool_call_done` / `turn_finalized` … events, with the DB snapshot on every attach acting as the reconnect replay. `mise run test-web` unit-tests the client's event fold (`web/src/lib/chat-protocol.test.ts`); `e2e/spa-chat.test.mjs` drives the full round trip against `dev-ui`.

Everything else about the UI — the layout of `web/`, the event table, theming, the PWA and Web Push, voice mode — is in [`ui.md`](ui.md).

## CI

GitHub Actions is wired up in `.github/workflows/ci.yml`. It triggers on pushes to `main`, on tags, and on pull requests. The toolchain comes from `mise.toml` via `jdx/mise-action`; `Swatinem/rust-cache` caches the cargo registry + `target/` across runs (CI does **not** use sccache). There are four jobs:

1. **ci** — runs `mise run ci`, which fans out via mise's DAG to lint + test + release-build + SPA-build. It then builds the `sandbox-runner` binary and uploads two artifacts: `gateway-binaries` (`target/release/{gateway, sandbox-runner, typst, libpdfium.so}`) and `gateway-spa` (`target/frontend/build/`) (7-day retention). Debuginfo is dropped from the dev/test profiles (`CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0`) so the multi-profile compile doesn't run the runner out of disk.
2. **container** (needs `ci`) — downloads the artifacts and builds the production image from `/Dockerfile` with `docker/build-push-action`. On pull requests it builds with `push: false` (validation only). On the default branch and on tags it pushes to GHCR (`ghcr.io/croit/aiplane`) with tags from `docker/metadata-action` (branch, tag, `sha-<short>`, and `latest` on the default branch).
3. **sandbox-image** (needs `ci`, `push` events only) — builds and pushes the code-execution sandbox gold image (`ghcr.io/croit/aiplane-sandbox`) from `sandbox-image/Containerfile`.
4. **sandbox-runner-image** (needs `ci`, `push` events only) — builds and pushes the sandbox runner image (`ghcr.io/croit/aiplane-sandbox-runner`) from `deploy/sandbox-runner/Containerfile`.

The two sandbox images are large and slow to build, so they only run on `push` (main/tags), never on PRs. See `docs/sandbox.md`.

The production `Dockerfile` is **runtime-only** — it compiles nothing. Starting from `debian:trixie-slim`, it:

- `apt-get install`s `git` + `ca-certificates` (the RAG indexer shells out to `git clone`, which validates TLS via the OS trust store, not the Rust binary's baked-in `webpki-roots`);
- `COPY`s the prebuilt `gateway` binary, plus `typst` (→ `/usr/local/bin/typst`), `libpdfium.so` (→ `/usr/local/lib/`), and the sample `examples/typst-templates` (→ `/opt/typst-templates`);
- `COPY`s the built SPA (`target/frontend/build` → `/usr/share/gateway/ui`) and sets `AIPLANE_STATIC_DIR` to it — a read-only layer the SPA handler only reads;
- runs as a non-root `gateway` user and exposes `8080`.

The UI is the one thing the image carries outside the binary, and it is plain static files: **no Node runtime**. No `cargo`, `npm` or `vite` runs in the image build — those all happen in the `ci` job, and the outputs arrive as artifacts.

CI never invokes `cargo`, `npm` or `vite` directly; everything routes through mise tasks. If you need a new CI step, add a `[tasks.…]` entry to `mise.toml` and call it from the workflow.

## Traps that have actually cost us time

Each of these has bitten at least once, each presents as something other than
what it is, and each now has a test that fails if it comes back. They are
written down because the symptom never points at the cause.

### Changing a build setting invalidates the whole graph, not just your crate

Editing a profile flag (`debug`, `incremental`, `opt-level`), `RUSTC_WRAPPER`
or `CARGO_INCREMENTAL` changes the fingerprint of **every** unit and every
sccache key, so the next build is a cold one — for this workspace, tens of
minutes, because the dependency graph includes ~116 tree-sitter grammars and
two C++ builds (usearch, pdfium). It has cost an hour twice now: once
A/B-testing sccache settings, once tightening the debuginfo profile.

Two rules follow:

1. **A/B build settings in a separate `CARGO_TARGET_DIR`**, never by flipping
   the setting back and forth in the shared one.
2. **Check what's already set before you change anything.** Build settings
   arrive from three places, and the repo only owns one of them: the root
   `Cargo.toml` (committed, everyone), `~/.cargo/config.toml` (that
   developer's machine only — `cargo config get` prints the merged result) and
   the environment (`CARGO_PROFILE_*`, which beats both — that's how CI drops
   debuginfo entirely). A setting that looks missing from the manifest may
   already be in effect locally.

### A test fixture that shells out to `git` can rewrite *your* repository

**Symptom.** Any of: your commit identity silently becomes
`t <t@example.invalid>`; `core.bare = true` appears in `.git/config` and every
worktree command starts failing with `fatal: this operation must be run in a
work tree` while `git log` still works; or `git status` shows your entire tree
staged as deleted, with a single `README.md` left in the index whose blob is
`hello world\n`. Nothing in any reflog explains it.

**Cause.** `git` exports `GIT_DIR`, `GIT_INDEX_FILE`, `GIT_WORK_TREE` and
friends to hooks and to every process a hook starts. `.githooks/pre-push` runs
the whole test suite, so the suite inherits a pointer to the real repository —
and a fixture that spawns `git` without clearing those variables operates on
*that* repo rather than its tempdir. `git config` overwrites your identity,
`git init` sets `core.bare`, `git add` replaces your index.

The damage is confined to the index and config: **working-tree files are never
touched**, so a wiped index is repaired with plain `git reset` (never
`--hard`), which rebuilds it from `HEAD`.

**Prevention.** `INHERITED_GIT_VARS` in
`crates/aiplane-features/src/server/rag/git.rs` lists the variables; every
`git` spawn clears them. `every_git_spawn_scrubs_the_inherited_context` (same
file) scans `crates/` and fails if a file naming `Command::new("git")` does
not also name `INHERITED_GIT_VARS` or `env_remove`. It cannot prove the scrub
reaches the right command, but the failure mode that actually bit was a silent
omission in a new fixture, and that is now impossible to add unnoticed.

Build scripts need the same treatment and cannot import from the workspace —
`crates/aiplane-api/build.rs` repeats the list. A build run from inside a hook
would otherwise resolve `HEAD` in the calling repository and stamp a foreign
SHA into `AIPLANE_GIT_SHA`, defeating the AGPL §13 source link it exists for.

### Size-probe tests: finite inputs only

**Symptom.** The machine freezes or reboots during a test run; afterwards the
only trace is a test process that grew to ~75 GB. It happened three times in a
row.

**Cause.** A TDD red test for a body cap fed an *endless* stream to a handler
that did not have the cap yet — which is the point of a red test. Without the
cap the handler buffers for as long as the stream lasts, and an endless stream
lasts until memory runs out. The test's own timeout does not help: by the time
it fires, gigabytes are allocated, and the OS kills or freezes something else
first.

**Prevention.** A test that probes a size limit sends a *finite* input that is
merely far over the limit: `cap + 1` bytes with a declared length, or for the
"no `Content-Length`" case a bounded stream such as `common::endless_body()`
(8 MiB, `crates/aiplane/tests/it/common/mod.rs`) or the 8 MiB chunked peer in
`capped_read.rs`'s tests. Against a missing cap such a test fails cleanly — the
handler reads it all and answers something other than 413 — rather than taking
the machine down. Put a `tokio::time::timeout` around the call as well, so a
handler that waits for more input fails rather than hangs.

### A memory crash: read the newest JetsamEvent report first

**Symptom.** Builds, tests or the whole desktop die without a Rust error; a
terminal shows `Killed: 9`, or the machine was simply restarted.

**Cause.** On macOS that is usually *jetsam*, the kernel's out-of-memory
killer. Guessing at the cause (a test? the linker? two parallel builds?) wastes
the next hour, because each guess costs a rebuild.

**Prevention.** Read the evidence first. Every jetsam kill writes
`/Library/Logs/DiagnosticReports/JetsamEvent-<date>.ips`:

```bash
ls -t /Library/Logs/DiagnosticReports/JetsamEvent-*.ips | head -1
```

The report lists every process with its resident pages at the moment of the
kill (`rpages` × the page size, 16 KiB on Apple silicon) and marks the one
killed: the largest entries name the culprit — a test binary at tens of GB is a
size-probe test (above), many `rustc`/`ld` entries are concurrent builds (one
Rust-building agent at a time, `CARGO_BUILD_JOBS=4`).

### Never `git push .` at a branch checked out in a sibling worktree

**Symptom.** The push succeeds, the branch moves, and the other checkout now
shows the entire changeset as unstaged deletions — as though someone reverted
the work. Nothing is lost, but it reads as catastrophic.

**Cause.** `git push .` updates a ref. It does not touch the working tree or
index of the worktree that has that branch checked out, which is then stale
against its own `HEAD`. The `receive.denyCurrentBranch` guard that exists to
prevent exactly this does **not** fire, because for a push originating in a
linked worktree the target branch is not receive-pack's "current" branch —
and for the same reason `receive.denyCurrentBranch = updateInstead` does not
help either. Both were tried.

**Prevention.** To move a branch that is checked out somewhere else, run the
merge *in that checkout*:

```
cd <the checkout that has the branch>
git status --short          # confirm it is clean
git merge --ff-only <source-branch>
```

`--ff-only` updates ref and working tree together and refuses outright if the
tree is dirty, so it cannot overwrite anything. If a `git push .` has already
left a checkout stale, `git reset --hard <branch>` in that checkout repairs it
— safe only once `git status` there shows nothing but the expected diff, since
a stale index makes genuine local edits indistinguishable from the inverse of
the incoming commits.

### daisyUI 5 deleted classes that daisyUI 4 relied on

**Symptom.** A form looks like its spacing is broken: label and input sit side
by side (`Name [input]`), help text is wedged between a label and its own box,
and the vertical gaps are enormous. Adjusting `gap-*` utilities changes
nothing, because the gaps are the line height of a wrapping inline paragraph.

**Cause.** `form-control` was daisyUI 4's label-plus-control wrapper and does
not exist in 5 — the label component's stylesheet under
`web/node_modules/daisyui/components` declares two selectors and that is not
one of them. A label carrying it gets no layout at all, so its children fall
back to `inline`. `label-text-alt` is gone the same way, and it carried the
shrink and dim that make a hint read as an aside rather than another paragraph.

**Prevention.** The house pattern for a labelled control is
`<label class="flex flex-col gap-1">` with the help `<span class="text-xs">`
**after** the input. `label-text` is inert too but harmless: daisyUI 4 gave it
`text-sm`, which bare uses inherit anyway.

Pinned by `web/src/lib/markup-drift.test.ts` (`mise run test-web`), which greps
every `.svelte` file for the dropped classes — the replacement for the two Rust
drift tests that died with the page stack.

### Tailwind scans source text, including comments

**Symptom.** The CSS bundle grows by a kilobyte or two, carrying a component
nothing on screen uses.

**Cause.** The Tailwind scanner reads source files looking for class-name
candidates and cannot tell a comment from markup. Naming a daisyUI class in
prose inside a scanned file is enough to emit its CSS. Under the SPA the
scanned set is `web/src` (the Tailwind v4 Vite plugin picks it up
automatically — no `@source` globs needed), so this now bites in Svelte and TS
comments rather than Rust doc comments.

**Prevention.** No test for this one — describe a class rather than spelling
it, and when the bundle size moves, check *which* selectors moved rather than
just that it changed.

### A new migration is invisible until the crate rebuilds

**Symptom.** Tests fail with "no such column" or "no such table" for a schema
the newest migration plainly creates, and keep failing however often they run.

**Cause.** `sqlx::migrate!("./migrations")` embeds the migration files at
compile time but never tells cargo it read them. Adding a file changes no
source cargo watches, so `aiplane-core` and every test binary that embeds the
set stay on the schema they were built with.

**Prevention.** `crates/aiplane-core/build.rs` prints
`cargo:rerun-if-changed=migrations`, so a file added to, changed in or removed
from the directory rebuilds the crate. `migrations_are_frozen.rs` →
`the_embedded_set_is_the_directory` compares the compiled-in versions with the
directory at run time and fails if they differ.
