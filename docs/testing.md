# Testing strategy

"Thorough testing" is a project-level rule (see [`AGENTS.md`](../AGENTS.md)). Concretely, that means each layer below is non-empty and runs in CI.

## Layers

| Layer | Lives in | What it covers |
|---|---|---|
| **Unit** | `#[cfg(test)] mod tests` next to the code | Pure functions, parsers, picker strategies, config validation |
| **Integration (in-process)** | `crates/gateway/tests/` | Build a `RamaState` against an in-memory SQLite + wiremock upstreams, then call `router(state).serve(req)` directly — no socket binding, since `rama`'s service is a plain async function. Shared setup lives in `tests/common/mod.rs`. |
| **Integration (mocked upstreams)** | `crates/gateway/tests/` | `wiremock` instances stand in for LLM backends; verify routing, the tool-call loop, streaming, the full OIDC login flow (`oidc_integration.rs`), and the JSON-SSE chat event wire (`chat_json_api.rs`). |
| **Contract generation** | `crates/gateway/tests/it/` | `openapi_drift.rs` proves the backend serves generated OpenAPI for representative routes and that no detached spec exists; `readme_routes.rs` checks the README's HTTP-endpoints table against `router.rs`; `spa_routes.rs` checks that the SPA catch-all is registered and reaches the SPA handler. |
| **SPA unit** | `web/src/lib/*.test.ts` | `mise run test-web` — Node's own `node --test` with type stripping, no jsdom. Covers the framework-free halves of the SPA (the chat event fold in `chat-protocol.ts`, markdown rendering), which is what pins client-side wire behaviour. |
| **E2E (browser ↔ gateway)** | `e2e/*.test.mjs` | Playwright + Node's `node:test` against a running `mise run dev`. The SPA suites are `e2e/spa*.test.mjs` (shell boot, signed-out OIDC redirect, signed-in identity, tokens, admin, a full chat turn streaming in); the rest cover the anonymous sign-in funnel and plain-`fetch` checks of the public HTTP surface. See `e2e/README.md`. |

## Style: test-first, Chicago / Classicist

Write the test before the code — red, green, refactor (**TDD**). Tests are **state-based**: assert on observable results, exercising real collaborators (in-memory SQLite, `wiremock` upstreams, the actual `ToolRegistry` / `UpstreamRegistry`) rather than interaction mocks. Behaviour-verification (London-school) mocks are the exception, reserved for collaborators you genuinely can't stand up in-process — and the test says why in a comment. The mocking philosophy below is the practical edge of this: we fake only the things that reach outside the process.

## Mocking philosophy

- **Upstream LLMs are always mocked in tests.** Real upstream calls in tests are forbidden. `wiremock` runs in-process.
- **OIDC is mocked end-to-end.** `crates/gateway/tests/oidc_integration.rs` builds the IdP out of wiremock: a discovery document, a JWKS carrying the public half of a freshly minted RSA dev keypair, and a token endpoint that returns an RS256-signed ID token whose `nonce` matches whatever the gateway just generated.
- **DB is real-but-ephemeral.** Integration tests open SQLite via `db::open(":memory:")`. The schema migrations run exactly as in prod; the in-memory backing just means we don't leak files. One pool per test.

## What every PR must include

- New public function → unit test for the happy path and at least one failure mode.
- New rama route → integration test asserting:
    - Returns 401 without a bearer / session.
    - Returns 403 when the route is RBAC-gated and the caller isn't authorized (e.g. a non-admin hitting an admin route via `require_admin_or_403`).
    - Returns the documented success shape.
- New `/api/v0` route → appears in `GET /openapi.json` automatically; add an explicit backend wire type when its body introduces a new reusable shape.
- New tool → test that invokes it via the registry (with a mocked upstream that fakes a `tool_calls` response).
- Schema change → round-trip serde test (`from_json(to_json(v)) == v` for a representative fixture).
- New chat event or a change to one → a case in `web/src/lib/chat-protocol.test.ts`. The fold is deliberately framework-free so this needs no browser; wire behaviour that can only be checked through a browser is wire behaviour nobody checks.
- New **server-rendered** string (error envelopes, the chat-render helpers) → a Fluent key in `locales/en/<module>.ftl` **and** its translation in all 5 other locales (`de`/`fr`/`es`/`ru`/`zh`) — not a checklist item you can skip: `session-core/build.rs` won't let the crate compile otherwise. The SPA renders the same six-language Fluent corpus as the server (generated into `web/src/lib/locales/` by `mise run gen-locales`, guarded by `i18n_drift`). See [`docs/ui.md`](ui.md#i18n--what-still-applies).

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
- Run with `mise run e2e` against a live `mise run dev` in another terminal — which is also what deploys the SPA (`GATEWAY_STATIC_DIR`), so the `spa*` suites have a shell to boot. An undeployed SPA answers 503 and those tests say so rather than failing obscurely. The task points `PLAYWRIGHT_DIR` at the mise-installed `npm:@playwright/cli` automatically. See `e2e/README.md` for first-time setup (shared libs + a one-time Chromium download).
- `e2e/spa-chat.test.mjs` needs a gateway with a chat upstream, so it targets `dev-ui` (`GATEWAY_STATIC_DIR=target/frontend/build mise run dev-ui`) and skips with a pointer at that command when no pool is configured.
- `GATEWAY_URL` (default `http://localhost:8080`) targets a specific gateway; `CHROMIUM_HEADED=1` shows the browser instead of running headless.
- **Not part of the CI default** — the browser suite needs a running gateway and Chromium, so it stays a local/opt-in loop.
- Authenticated flows (`e2e/authed.test.mjs`, the `spa*` signed-in tests) don't need OIDC: they sign in through the debug-only `/__dev/*` seeding endpoints (`rama_server::dev_seed`), compiled in under `cfg(debug_assertions)` and never present in a release build. `/__dev/seed-session` resets the canonical fixture (user `alice@example.com` + her three tokens) and is reserved for the one file that asserts those counts; everything else uses the delete-free `/__dev/session`. Completing setup is also how the suite makes `/readyz` deterministic on a fresh dev database.

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
