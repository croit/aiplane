# Testing strategy

"Thorough testing" is a project-level rule (see [`AGENTS.md`](../AGENTS.md)). Concretely, that means each layer below is non-empty and runs in CI.

## Layers

| Layer | Lives in | What it covers |
|---|---|---|
| **Unit** | `#[cfg(test)] mod tests` next to the code | Pure functions, parsers, picker strategies, config validation |
| **Integration (in-process)** | `crates/aiplane/tests/` | Build a `RamaState` against an in-memory SQLite + wiremock upstreams, then call `router(state).serve(req)` directly — no socket binding, since `rama`'s service is a plain async function. Shared setup lives in `tests/common/mod.rs`. |
| **Integration (mocked upstreams)** | `crates/aiplane/tests/` | `wiremock` instances stand in for LLM backends; verify routing, the tool-call loop, streaming, the full OIDC login flow (`oidc_integration.rs`), and the JSON-SSE chat event wire (`chat_json_api.rs`). |
| **Contract generation** | `crates/aiplane/tests/it/` | `openapi_drift.rs` proves the backend serves the generated OpenAPI document with every operation declared (credential, success and error responses), checks representative schemas against the handlers' wire types (the groups save body, the chat submit, the event stream, the public and embed credentials), and that no detached spec exists; the unit tests in `rama_server/openapi` fail on a route without a declaration, a declaration without a route, a dangling schema reference, or an operation marked schema-unsupported that `docs/ui.md` does not list; `readme_routes.rs` checks the README's HTTP-endpoints table against `router.rs`; `spa_routes.rs` checks that the SPA catch-all is registered and reaches the SPA handler. |
| **SPA unit** | `web/src/lib/*.test.ts` | `mise run test-web` — Node's own `node --test` with type stripping, no jsdom. Covers the framework-free halves of the SPA (the chat event fold in `chat-protocol.ts`, markdown rendering), which is what pins client-side wire behaviour. |
| **E2E (browser ↔ gateway)** | `e2e/*.test.mjs` | Playwright + Node's `node:test` against a running gateway. The SPA suites are `e2e/spa*.test.mjs` (shell boot, signed-out OIDC redirect, signed-in identity, tokens, admin, personal Dot lifecycle/access rules/chat/memory/activity, and a full chat turn streaming in); the rest cover the anonymous sign-in funnel and plain-`fetch` checks of the public HTTP surface. See `e2e/README.md`. |

## Architecture tests

Feature tests check what a feature does; they cannot see that a new file
skipped a shared mechanism. The cross-cutting invariants are therefore checked
by tests that read the workspace's own source, in
`crates/aiplane/tests/it/architecture.rs`:

| Test | Invariant | Shared mechanism |
|---|---|---|
| `outbound_http_clients_are_built_only_at_the_vetted_sites` | a reqwest client (`Client::new`/`builder`, `ClientBuilder::new`, `reqwest::get`, under any `use` alias) is built only in the listed files | the net_guard-checked, pinned client in `aiplane-core/src/server/outbound_guard.rs`, with a `Policy` per caller, for every destination the operator does not configure (a user's, a model's, an agent owner's, a push endpoint, the MCP OAuth endpoints) — it is the only guarded site on the list; `AppState::http` for operator-configured backends |
| `request_bodies_are_read_only_through_the_capped_readers` | nothing outside `session_core::chrome` names `BodyExt` or drains a body with `into_data_stream` (the sandbox runner, outside the stack, has its own capped `/run` reader); `read_body_to_bytes`/`read_json` appear only behind `BodyLimitLayer` (`aiplane/src/rama_server/`, `aiplane-api/src/`) and never in the modules defining a handler `router.rs` registers under `BodyLimitLayer::HANDLER_CAPPED` (read from the router). Every file that reads a request body is listed once, with how, in `REQUEST_BODY_READS`; this rule and the next take their exceptions from it | `read_body_capped`, `read_body_prefix`, `read_json_capped`, `BodyLimitLayer` |
| `response_bodies_are_read_only_through_the_capped_reader` | no `.bytes()`, `.text()`, `.json()` or `.json::<T>()` is awaited outside the `Multipart` entries of `REQUEST_BODY_READS` — the inbound multipart readers (`aiplane-api` chat and skill uploads, `aiplane/src/rama_server/multipart.rs`), whose bodies `BodyLimitLayer` already bounded | `capped_read::read_capped` / `read_capped_json` / `read_capped_text` / `read_error_text`, with a cap per use (`API_ANSWER_BYTES`, `MODEL_ANSWER_BYTES`, or the caller's own) |
| `agent_spec_json_is_read_only_by_the_validator` | no JSON accessor (`.get`, `.get_mut`, `.remove`, `.pointer`, `[..]`) names a spec key (`publish`, `routes`, `main`, …) outside the validator | the typed `AgentSpec` from `CompiledSpec::agent()` |
| `workspace_crates_depend_only_down_the_stack` | every dependency between workspace members (normal, build, dev; from `cargo metadata --no-deps --offline`) points to a lower level of the AGENTS.md stack; siblings (`-features`/`-agents`, `-tools`/`-api`) never depend on each other; `sandbox-runner` uses only `shared` and nothing uses it; every member has a level | the stack in AGENTS.md → "The gateway crate stack" |
| `activity_events_are_written_only_through_the_activity_log` | no SQL `INSERT`/`REPLACE`/`UPDATE`/`DELETE` of `agent_audit` outside its module (`aiplane-agents/src/db/agent_audit.rs`), and no call of its writers (`agent_audit::append`, `append_now`) outside the named agent DB modules that record a change on its own transaction (each listed) and the runtime's door | the activity log's one writer, `agent_audit::append`, reached through `aiplane_runtime::agents::audit` (bounded, fails a run closed) or on a management change's own transaction |
| `model_tool_calls_dispatch_only_through_the_grant` | only the chat/agent driver, its resume path and the `/v1` loops hand calls to the runner, and each still builds its gate (`GrantedToolSource`, `DiscoverableToolSource`); `Tool::run` is called directly only by the runner and the listed wrappers/verifiers | `GrantedToolSource` (+ `RunToolSource` over it), `DiscoverableToolSource` |
| `personal_dot_calls_revalidate_access_before_execution` | the personal Dot source wraps an initially granted tool, and dispatch keeps the live owner, active Dot, selected tools and approval checks | `LivePersonalDotTool`, `GrantedToolSource`, `Permissions`, `AskFirst` |

How they work: each production `.rs` file under `crates/` (not `tests/`,
`tests.rs`, `examples/`, or a `#[cfg(test)]` item) is masked — comments
blanked, string literal contents blanked except for the spec-key rule, which
matches the literal on purpose — and searched for the call shape that breaks
the rule. A hit outside the rule's allow-list fails with the file and line. An
allow-list entry that matches nothing fails too, so an exception
cannot outlive its reason. A scan is a tripwire, not a proof: it catches the
silent omission in a new file, which is how every one of these invariants was
actually broken.

The allow-lists are deliberately short, and every entry carries its reason.
An entry marked **KNOWN GAP** would be a real violation that predates its
test, listed so the test can land green and stop the next one; fixing it
removes the entry. There are none at the moment.

**Adding one.** Write it in `architecture.rs` next to the others: the needles,
the view (`code` with literals blanked, or `text` with them kept), an
`Allowed { path, why }` list, and `assert_within` with a message that names the
shared mechanism to use instead. Then prove it: add the violation to a
production file (an undeclared `.rs` file under some `src/` is enough — the
scan reads files, not the module tree), watch the test fail naming it, remove
it. Prefer a `clippy.toml` entry as well where clippy can resolve the path (see
below), so the editor points at the line.

### Docs describe the current system

`crates/aiplane/tests/it/docs_current.rs` reads every `docs/*.md` the same
way and fails, naming file, line and rule, on a heading that names an issue
(`### What #84 built`), an issue reference (`#120`) in prose, and the
phrase `no longer`, which narrates a change rather than states a fact
(`instead of` is allowed: it usually states a design choice). Fenced code blocks and inline code spans are skipped, so a
colour (`#242427`) or quoted code never trips it. A doc whose subject is
history goes on its `HISTORY_DOCS` allow-list with the reason; the list is
empty, and an entry that matches nothing fails like the architecture lists.

### The same rules in `clippy.toml`

Where a rule is a call clippy can resolve by path, the workspace `clippy.toml`
lists it under `disallowed-methods`, so `mise run lint` (and the editor) points
at the line: `reqwest::Client::new`, `reqwest::Client::builder`,
`reqwest::ClientBuilder::new`, `reqwest::get` and
`http_body_util::BodyExt::collect` (which rama's `body::util::BodyExt`
re-exports). Each production site on the scan's allow-list carries
`#[allow(clippy::disallowed_methods)]` with a comment saying why; test modules
and the integration crates allow it once at the module or crate root, because
tests build plain clients and drain bodies to talk to their in-process mocks.
The spec-key and dispatch rules have no clippy form — what they forbid is a
`serde_json::Value` accessor or a `Tool::run` call that is fine elsewhere — so
only the scan checks them.

## Runaway tests

`.config/nextest.toml` gives every test `slow-timeout = { period = "60s",
terminate-after = 2 }`: nextest reports a test SLOW after 60 s and kills it at
120 s. Measured on the full workspace (3205 tests, `NEXTEST_TEST_THREADS=4`),
the slowest legitimate test takes ~5.3 s
(`agents::run::tests::verifiers::a_forward_in_the_round_of_the_lookup_that_opens_its_route_is_dispatched`),
so the limit only ever catches a runaway — a lost wake-up, a stream that never
ends, a body read without a cap. The live binaries (`sandbox_e2e_live`,
`nextcloud_e2e_live`) get ten periods instead, since they talk to real
infrastructure. If a new test legitimately needs more than a minute, give it a
per-test `[[profile.default.overrides]]` entry with the reason rather than
raising the default.

The timeout is a backstop, not the protection: a test that buffers without
bound can allocate tens of GB long before 120 s are up. Size-probe tests use
finite inputs (`docs/dev-workflow.md` → "Size-probe tests: finite inputs
only").

## Style: test-first, Chicago / Classicist

Write the test before the code — red, green, refactor (**TDD**). Tests are **state-based**: assert on observable results, exercising real collaborators (in-memory SQLite, `wiremock` upstreams, the actual `ToolRegistry` / `UpstreamRegistry`) rather than interaction mocks. Behaviour-verification (London-school) mocks are the exception, reserved for collaborators you genuinely can't stand up in-process — and the test says why in a comment. The mocking philosophy below is the practical edge of this: we fake only the things that reach outside the process.

## Mocking philosophy

- **Upstream LLMs are always mocked in tests.** Real upstream calls in tests are forbidden. `wiremock` runs in-process.
- **OIDC is mocked end-to-end.** `crates/aiplane/tests/it/oidc_integration.rs` builds the IdP out of wiremock: a discovery document, a JWKS carrying the public half of a freshly minted RSA dev keypair, and a token endpoint that returns an RS256-signed ID token whose `nonce` matches whatever AIplane just generated.
- **DB is real-but-ephemeral.** Integration tests open SQLite via `db::open(":memory:")`. The schema migrations run exactly as in prod; the in-memory backing just means we don't leak files. One pool per test.

## What every PR must include

- New public function → unit test for the happy path and at least one failure mode.
- New rama route → integration test asserting:
    - Returns 401 without a bearer / session.
    - Returns 403 when the route is RBAC-gated and the caller isn't authorized (e.g. a non-admin hitting an admin route via `require_admin_or_403`).
    - Returns the documented success shape.
- New `/api/v0` route → a declaration in `crates/aiplane/src/rama_server/openapi/` (credential, request, responses, errors), with `#[derive(schemars::JsonSchema)]` on the request and response types the handler really uses; `every_registered_route_is_declared_exactly_once` fails without it.
- New tool → test that invokes it via the registry (with a mocked upstream that fakes a `tool_calls` response).
- Schema change → round-trip serde test (`from_json(to_json(v)) == v` for a representative fixture).
- New chat event or a change to one → a case in `web/src/lib/chat-protocol.test.ts`. The fold is deliberately framework-free so this needs no browser; wire behaviour that can only be checked through a browser is wire behaviour nobody checks.
- New **server-rendered** string (error envelopes, the chat-render helpers) → a Fluent key in `locales/en/<module>.ftl` **and** its translation in all 5 other locales (`de`/`fr`/`es`/`ru`/`zh`) — not a checklist item you can skip: `session-core/build.rs` won't let the crate compile otherwise. The SPA renders the same six-language Fluent corpus as the server (generated into `web/src/lib/locales/` by `mise run gen-locales`, guarded by `i18n_drift`). See [`docs/ui.md`](ui.md#i18n).

If a change has no tests, the PR description must explain why and which existing test covers it.

## CI shape

CI (`.github/workflows/ci.yml`) runs a single command:

```text
mise run ci
```

`mise run ci` fans out through mise's task DAG to **lint + tests + release build + SPA build**:

- `mise run lint` → `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `check-web` (svelte-check).
- `mise run test` → `cargo nextest run --workspace`.
- `mise run test-web` → `node --test` over the SPA's unit tests.
- `mise run build` → the release gateway binary.
- `mise run build-web` → the SPA into `target/frontend/build/`.

The Rust and SPA halves build independently — `cargo` needs no Node and the SPA build needs no `cargo` — so a fresh checkout works without manual steps either way. The same job then builds the `sandbox-runner` binary and uploads the binaries and the SPA as artifacts; downstream jobs build the container images.

The version-controlled pre-push git hook (`.githooks/pre-push`, enabled with `mise run setup-hooks`) mirrors CI's lint + test locally so breakage is caught before a push triggers CI. It skips the release build — a compile error surfaces in the test step anyway. Bypass a WIP push with `git push --no-verify`.

## E2E browser tests (`e2e/`)

- Driver: Node's built-in `node:test` + Playwright. No project-level `node_modules` — the tests import `playwright` directly out of the mise-installed `npm:@playwright/cli` tool, with the path overridable via `$PLAYWRIGHT_DIR`.
- Run with `mise run e2e` against a live `mise run dev` in another terminal. The public `:8080` origin is Vite/HMR and proxies the complete gateway surface to private `:8081`, so the browser suites exercise the everyday development topology. Use `mise run dev-served` when the compiled SPA itself is under test. The task points `PLAYWRIGHT_DIR` at the mise-installed `npm:@playwright/cli` automatically. See `e2e/README.md` for first-time setup (shared libs + a one-time Chromium download).
- `e2e/spa-chat.test.mjs` needs a running AIplane with a chat upstream, so it targets `dev-ui` (`AIPLANE_STATIC_DIR=target/frontend/build mise run dev-ui`) and skips with a pointer at that command when no pool is configured.
- `AIPLANE_URL` (default `http://localhost:8080`) targets a specific gateway; `CHROMIUM_HEADED=1` shows the browser rather than running headless.
- **Not part of the CI default** — the browser suite needs a running gateway and Chromium, so it stays a local/opt-in loop.
- Authenticated flows (`e2e/authed.test.mjs`, the `spa*` signed-in tests) don't need OIDC: they sign in through the debug-only `/__dev/*` seeding endpoints (`rama_server::dev_seed`), compiled in under `cfg(debug_assertions)` and never present in a release build. `/__dev/seed-session` resets the canonical fixture (user `alice@example.com` + her three tokens) and is reserved for the one file that asserts those counts; everything else uses the delete-free `/__dev/session`. Completing setup is also how the suite makes `/readyz` deterministic on a fresh dev database.

## Personal Dot background coverage

The background slice uses real registries, in-memory SQLite and mocked model
upstreams. `crates/aiplane/tests/it/dots_background.rs` covers owner-bound
schedule creation, same-Dot private linked conversations, ordinary versus
Dot-filtered chat listings and retained inactive bindings. It also covers exact
turn correlation, pending-run deletion conflicts and synchronous webhook
fire/replay returning HTTP 202 when approval is required. Disabling an
automation still permits its pending turn to settle.

Runtime tests beside `server/background.rs` cover restoration of scheduled or
webhook usage source, recorded history limits, original/current tools ceiling,
owner and binding rejection, waiting/completed/cancelled reconciliation and
protection of newer summaries from older continuations. Headless and shared
worker/session-core tests cover atomic idle user/assistant insertion, refusal
to append to running or suspended conversations, and background registration
using the chat worker lifecycle. Atomic edit/retry tests verify that concurrent
background admission cannot rewrite history or leave a partial prompt. Approval
continuation uses actual suspension rows and a mocked upstream, asserting the
resulting Dot memory and exact run state. A busy resume leaves the original
approval unclaimed for retry; abandoned reservations clear their own worker.

`web/src/lib/dot-background.test.ts` and `background-api.test.ts` cover real
choice availability and query semantics. `e2e/spa-dots-background.test.mjs`
exercises the existing schedule/webhook forms, preselected Dot links, scoped
linked conversations, resulting Dot sessions, retained inactive edits and
mobile layout against the seeded gateway. Its pending webhook case inspects the
original arguments in Inbox, approves once and verifies exact-run completion,
Dot-only memory writes and settlement after the automation is disabled. Run it through the existing E2E
mise task and a real debug gateway; it is an opt-in browser suite.

Before calling the slice complete, verify an approval-required Dot webhook
reaches waiting, returns HTTP 202 for synchronous fire/rerun, appears in Inbox,
resumes under unchanged recorded tools/history ceilings, updates its exact run
and prevents deletion until the turn settles. Disabling the automation must
still permit settlement; deactivating the Dot must pause continuation. The
workspace proposal is not included in current runtime coverage.

## Performance / load tests

Not part of the per-PR loop, and no benchmark suite exists yet. If one is added, the natural target is the per-request middleware overhead and the upstream picker (e.g. a no-op `/v1/chat/completions` against a mocked instant upstream), measured with `criterion`.

## Coverage

We don't enforce a line-coverage number — it incentivizes the wrong tests. Instead the "what every PR must include" checklist is the gate.

## Crates dedicated to testing

Pre-approved dev-dependencies are listed in [`docs/dependencies.md`](dependencies.md). Adding anything else requires the same justification step as a runtime dep.

## Live, on-demand tests

Two test binaries are gated behind an env var and excluded from the normal
run — they need infrastructure a hermetic suite cannot have. Both compile in
`cargo test` and skip instantly without their gate.

| Binary | Gate | Runner | Needs |
| --- | --- | --- | --- |
| `sandbox_e2e_live.rs` | `RUN_SANDBOX_E2E` | manual | a sandbox-runner + real S3 |
| `nextcloud_e2e_live.rs` | `RUN_NEXTCLOUD_E2E` | `mise run test-nextcloud` | docker or podman |

`mise run test-nextcloud` owns the whole lifecycle: it starts a throwaway
Nextcloud, waits for it to finish installing (first run also pulls ~600 MB),
runs the tests single-threaded, and tears the container down on any exit
including Ctrl-C. `NEXTCLOUD_E2E_KEEP=1` leaves it running to poke at.

What belongs there rather than in the mocked suite: assertions about
*someone else's* behaviour, which a mock cannot check because the mock is
built from the same assumption. See
[`fileshare-rag.md`](fileshare-rag.md#testing) for the current list.

The separate Node suite `e2e/live/system-one.test.mjs` runs only through
`mise run test-system-one-live`. It calls a running local gateway backed by a
real System One provider and incurs inference charges. It checks model
discovery, pinned and rolling IDs, all three decision primitives, and (with
an owner session cookie) persisted token accounting. Configuration and
credential handling are documented in the root [README](../README.md).
It is deliberately outside the ordinary browser-test glob and offline CI.

## Security scanning triage

Run CodeQL against the changed source, not only the last committed revision.
Use the default code-scanning suites for Rust and JavaScript/TypeScript, as
configured in GitHub. Local results do not update GitHub alerts: those remain
attached to the last uploaded analysis until a new scan or explicit triage.

Do not weaken regression coverage or hide whole rules to achieve an empty
report. These four existing alerts have specific false-positive rationales:

| GitHub alert | Code | Rationale |
| --- | --- | --- |
| 52 | `crypto::sha256_hex`, called by `auth::token` | Gateway bearer tokens are generated from 32 OS-random bytes, not user-chosen passwords. SHA-256 is used for lookup of these high-entropy credentials; password hashing rules do not apply. Revisit if human-chosen credentials ever reach this path. |
| 81 | `known_answers::a_ciphertext_from_an_earlier_release_still_opens` | The fixed nonce belongs to a historical decryption-only test vector. Production encryption generates a fresh nonce. |
| 93 | `known_answers::key_derivation_is_byte_for_byte_stable` | A fixed session-secret input pins the exact HMAC derivation output across releases. It is not a deployed credential. |
| 94 | `known_answers::a_ciphertext_from_an_earlier_release_still_opens` | The fixed AES key must match the historical ciphertext. Randomizing it would remove the compatibility check. |

Preserve these known-answer vectors unchanged. Other round-trip fixtures use
ephemeral keys; production entropy failure must never substitute a fixed key.
Any dismissal should cite the individual rationale above, not suppress the
rule or the source file.
