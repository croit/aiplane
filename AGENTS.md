# AGENTS.md — read this first

Read this file before working in the repository. Start at the [documentation index](docs/README.md) for details about AIplane.

## What this project is

AIplane is a self-hosted AI infrastructure layer. The `aiplane` Rust binary serves the authenticated gateway and its JSON API using rama. The SvelteKit SPA lives in `web/` and receives live chat updates over JSON-SSE.

## The gateway crate stack

The top of the stack is `crates/aiplane` (startup, router, proxy, OIDC), then `aiplane-api` (JSON handlers) and `aiplane-tools` (tool implementations), then `aiplane-runtime` (tool API, `AppState`, chat and agent drivers), then `aiplane-features` and `aiplane-agents`, then `aiplane-core` (DB, config, crypto, RBAC, upstreams). `session-core` provides the owner- and driver-agnostic chat substrate, and `shared` holds wire types. `sandbox-runner` serves sandboxed execution separately.

Place code as high in the stack as possible. Normal, build and dev dependencies go downward only. `aiplane-api` / `aiplane-tools` and `aiplane-features` / `aiplane-agents` are independent sibling pairs. Features and agents cannot reference `AppState` or the tool registry. Core cannot reference feature code or agent DB accessors. Keep agent, visitor and principal details out of `session-core`. Agent-table DDL lives in the core migration set and its accessors live in `aiplane-agents`. The [crate boundaries](docs/architecture.md#crate-boundaries) explain the layout and its architecture test.

Register tools in the `ToolRegistry` built by `crates/aiplane/src/main.rs` and grant them via `/admin/groups`. See [tools and RBAC](docs/tools-rbac.md).

## Hard rules — do not violate without asking

1. **Dependencies.** Prefer stdlib and existing dependencies. Justify each new Cargo crate in [docs/dependencies.md](docs/dependencies.md).
2. **Toolchain.** Run toolchain, build, test and lint commands through `mise`. Keep task definitions there. Avoid Makefiles, justfiles and ad-hoc checked-in scripts. The standalone exceptions are `scripts/derive-version.sh` (also called by `mise run version`) and `scripts/memory-guard.sh`.
3. **Tests.** Write a failing test first, then implement and refactor. Every public function needs unit coverage in `#[cfg(test)] mod tests`. Every rama route needs integration coverage in `crates/aiplane/tests/` through `router.serve(req).await` without binding a socket. Assert observable outcomes using in-memory SQLite, real registries and offline Wiremock upstreams. Document any interaction mock required for a collaborator that cannot be faked. Keep all test inputs and loops finite. See [testing](docs/testing.md).
4. **Errors.** Use `thiserror` at API boundaries and `anyhow` with `.context()` internally. Say what operation failed, why, and what the reader can do. See [errors](docs/errors.md).
5. **UI and HTTP.** Use the component classes in `web/src/app.css` and Tailwind utilities on the croit-identity tokens. Add reusable styles there and cover new classes in `web/src/lib/markup-drift.test.ts`. Avoid one-off CSS and daisyUI. New server endpoints return JSON under `/api/v0`. Live turns use `session_core::chat_json`. See [UI](docs/ui.md).
6. **Comments.** Use names and types to explain what the code does. Comment the reason only when it is non-obvious.
7. **Compatibility.** Pre-1.0 changes must avoid backwards-compatibility shims.
8. **Deployment README.** Update `README.md` in the same commit when a runtime knob, config field, host package or deployment task changes.
9. **Localization.** Put user-visible strings in the canonical Fluent catalogs under `crates/session-core/locales/<lang>/` for all six languages (en/de/fr/es/ru/zh). Run `mise run gen-locales` and use the translation layer. The server and SPA share keys. Write logs, code, comments and test names in English. Domain content intrinsically in another language is allowed as data. Keep individual messages in one language.
10. **Code principles.** Prefer simple code, shared representations and established interfaces such as `Tool` and `SessionDriver`. Use the project's vocabulary, including upstream, pool, backend, gateway-owned and client-owned tool calls, turn, round and `Acquired`. Favor clarity when a design principle would add complexity and record why.
11. **Reuse.** Search for an existing mechanism before adding anything. Reuse or extend what exists and state the search and result in the brief and commit. A second mechanism for the same purpose requires explicit maintainer approval.
12. **Facts and real callers.** Show users and models data from the resource itself. Show missing data as missing and fix it at its source. Every addition needs a real caller in the same change. Exclude speculative hooks, invented defaults and unrequested options. Keep references to internal croit software out of public project content.
13. **Migrations.** Use one migration per unreleased change set. Amend it, its schema fixture and checksum in place. Fold corrections for unreleased data into that migration.
14. **Product docs.** Before implementation, find affected pages via the documentation index. In the same commit, update affected user, admin, operator, API, config and deployment docs, plus screenshots, navigation, coverage and generated Markdown/LLM exports as applicable. Verify claims against the implementation and UI. For internal changes, record in the review when documented behavior and contracts are unaffected.
15. **Git.** Work on a branch, merge locally into `main`, and push to `origin` through the pre-push hook. Do not open, review or merge via GitHub PRs or GitLab MRs. Create release tags only when explicitly requested. See [releases](docs/releases.md).

## Working agreement for agents

### Before building

- Check requirements against the implementation. Raise conflicts and duplicates. Clarify requirements marked coarse, open or left to refine.
- For work touching multiple files or concepts, get maintainer confirmation on a short design covering reused mechanisms, data model, UI data source and user flow before coding. Build and show one vertical slice before adding the next layer.
- For changes to rights, privacy, data exposure, billing, metering or visible behavior, ask the maintainer first and present options with a recommendation.
- Get an approved mockup or walkthrough with real data for complex or nonstandard UI. Provide a concise ASCII mockup for complex, nonstandard or materially changed UI. Established simple dialogs and forms need no mockup. The coordinator clicks through UI changes before completion.
- Design docs for cross-cutting work include a **Shared mechanisms** section listing existing mechanisms and additions. Reuse `net_guard` and its pinned client for outbound URLs, `read_capped` / `read_body_capped` / `BodyLimitLayer` for body limits, typed `AgentSpec`, `GrantedToolSource`, the chat model list, the capability catalog and `chat_turn_suspensions` where applicable. Land new shared blocks with tests, including architecture coverage, before their consumers.

### While building

- Keep work packages small. At most three branches in flight, each merged within a day. Parallel work uses disjoint files. Serialise edits to schemas, catalogs, generated files and shared docs. Regenerate generated files on merge.
- **One commit is complete.** Include code, tests, docs, translations, generated artifacts and formatting in the same commit. Amend or squash corrective follow-ups. The pre-commit hook checks staged Rust formatting.
- Describe current behavior and its rationale in docs. Keep issue numbers and implementation history in Git and the tracker. Document newly discovered invariants and gotchas in the relevant page and split long docs by topic.
- Review the complete branch once before merge after known changes are in, then fix findings. Re-review only if a correction changes the design. Use `/code-review` at medium effort by default, low for docs, catalogs and Compose/config, and high for roughly 2,000+ changed lines or changes to rights, money or data exposure. Also run `/security-review` once for public surfaces, rights, grants, tokens, sharing, URLs chosen by someone other than the operator, or personal data.
- Inspect logs and reports before diagnosing a failure. For memory crashes, start with the latest JetsamEvent. Leave other Cargo processes running when waiting for a build lock.
- Use the [delegation brief](docs/dev-workflow.md#delegating-work-to-an-agent). Delegated agents commit each green state and end with a report. The coordinator reviews the result and verifies UI changes in a browser.
- Respect the maintainer's environment. Use a separate browser tab and emulate viewport sizes. Announce dev-server restarts and restart only to load merged state. Avoid disrupting running sessions. Back up the maintainer's DB before modifying it and report the change.
- Surface any hard rule that blocks progress.

## Daily workflow

See [dev workflow](docs/dev-workflow.md) for setup, commands and troubleshooting.

- Wrap every local build, lint, test and browser job in `mise run guard -- <command>`, or use `scripts/memory-guard.sh <GiB> <command>` when the command needs its own quoting. Default limit is 32 GiB across shared work. Set `CARGO_BUILD_JOBS=2` and `NEXTEST_TEST_THREADS=2`. Override the limit with `MEMORY_LIMIT_GB` when needed.
- Use `mise run dev` for Vite with the real debug gateway and OIDC, `dev-served` for the compiled SPA, and `dev-build` for the debug binary. Use `dev-ui` for deterministic browser automation with a seeded session and mocked upstreams.
- Verify with `mise run check`, `test-crate <crate> [filter]` and `lint-crate <crate>`. Run the full `mise run verify` gate once before pushing, after all known fixes. Complete review and verification before calling the change done.
- Reserve release builds for deploys or benchmarks. `mise run ci` adds the release build to verification. Use `mise run setup-hooks` once per clone and `mise run sweep-target` for build-cache cleanup.
