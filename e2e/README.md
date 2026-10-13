# e2e/

End-to-end browser tests for AIplane, driven by Playwright through Node's built-in test runner. Zero project-level `node_modules` — the tests import `playwright` directly out of the mise-installed `npm:@playwright/cli` tool.

## Run

```bash
# In one terminal:
mise run dev

# In another:
mise run e2e
```

That runs every `e2e/*.test.mjs`:
- `api.test.mjs` — plain `fetch` against the public HTTP surface.
- `anonymous.test.mjs` — Playwright-driven browser flows for the sign-in funnel an anonymous visitor sees.
- `authed.test.mjs` — Playwright flows on authenticated pages, each on a fresh seeded fixture session.
- `spa.test.mjs` — the SvelteKit SPA at the root: shell boot, signed-out redirect into OIDC, signed-in identity from `GET /api/v0/me`.
- `spa-chat.test.mjs` — the SPA chat round trip (issue #22 P2): create a conversation, submit a turn, watch the reply stream in over the JSON-SSE event protocol. Needs an AIplane with a chat upstream — `AIPLANE_STATIC_DIR=target/frontend/build mise run dev-ui` — and skips with a pointer at that command when AIplane has no pools.

The Dot suites use the seeded `dev-ui` gateway with actual tool catalogues and
scripted model responses:

- `spa-dots.test.mjs`: personal lifecycle, access, private chat, memory,
  activity and mobile layout.
- `spa-dots-background.test.mjs`: existing automation forms, Dot binding,
  exact-run durable Inbox approval and isolated memory writes.
- `spa-dots-permissions.test.mjs`: independent group permission, live grant/revocation,
  personal lifecycle without agent management; uses the seeded Engineering member
  session supplied through `AIPLANE_DOT_MEMBER_SESSION_COOKIE`.
- `spa-dots-organizations.test.mjs`: actual organization creation and group
  selection, revision conflict with retained draft, scoped agent configuration
  and publication, unavailable execution/channel controls, independent member
  management rights, four-active-Dot capacity, deactivation, membership
  revocation and mobile layout. The member case requires the seeded Engineering
  session through `AIPLANE_DOT_MEMBER_SESSION_COOKIE`; it restores its temporary
  group permissions and removes its created Dots.
- `spa-dots-organization-identities.test.mjs`: current organization writer
  without personal Dot management permission, real local OAuth round trip to
  the existing agent editor, own-principal-only selection, unavailable catalogue
  with retained local erasure, and mobile layout. Uses the seeded Engineering
  session through `AIPLANE_DOT_MEMBER_SESSION_COOKIE`; restores temporary group
  permissions and catalogue availability and deletes its created Dot.
- `spa-dots-organization-draft.test.mjs`: private saved-draft and published
  Test chat through the actual streamed renderer. Covers explicit source,
  published-version pinning after another publication, delayed-submit source
  locks, reset, private history admission, keyboard submission and mobile
  layout. Uses the seeded Engineering member session and restores its fixture
  permissions and resources. `DOCS_ORG_DOT_PUBLISHED_CAPTURE` captures the actual
  first published answer and writes separate capture metadata for the coordinator.
- `spa-dots-organization-memory.test.mjs`: the existing editor shows actual
  Dot/shared-organization scopes, administrator shared edits, member own-note
  edits, UTF-8 byte validation with retained editable text, read-only revocation,
  membership withdrawal clearing displayed content and mobile layout. Uses the
  actual Engineering session and restores group permissions. Optional
  `DOCS_ORG_DOT_MEMORY_CAPTURE` saves the real memory panel and records capture
  metadata separately for the documentation manifest.
- `spa-dots-actions.test.mjs`: action inheritance and overrides, reload,
  correction of unavailable rules, and revocation between pause and approval.
  Its browser probe validates permission refusal; the fixture does not execute
  sandbox containers. Batch dispatch is covered by real browser-request hub
  tests in `browser_control`.
- `spa-dots-communication.test.mjs`: same-owner directed consent, one-row
  saves, unchanged generations, retained conflict drafts, inactive revocation
  and mobile/keyboard layout. This suite checks consent configuration, not
  personal delegation execution. Its German-browser/English-interface case
  verifies that API errors follow the selected interface language.
- `spa-dots-delegation.test.mjs`: actual two-Dot execution, root Inbox
  approval, receiver-only memory writes, private read-only child history,
  root navigation and mobile layout.
- `spa-dots-identities.test.mjs`: local OAuth provider redirect and shared
  callback, explicit identity selection, actual selected MCP inventory and
  disconnect of only the Dot account. The `dev-ui` fixture supplies the
  `dot-workspace` connector and sealed user credentials; it uses no live
  provider account. Missing saved credentials remain unavailable without
  falling back to the user's connection. A second case disables the actual
  connector, reloads and erases only the Dot's local credentials for both a
  selected and a never-selected connection; the user's account is retained.
  A third case changes the actual catalogue server, verifies reconnect status
  and unavailable selection, reconnects and explicitly selects the Dot account,
  then checks that restoring the earlier URL does not revive old credentials.
  This leaves the synthetic user credential stale; start a fresh isolated
  fixture before repeating the complete suite.

Run these suites sequentially when checking the four-active-Dot limit:

```bash
CHROMIUM_CHANNEL=chrome mise exec -- node --test --test-concurrency=1 \
  e2e/spa-dots.test.mjs e2e/spa-dots-background.test.mjs e2e/spa-dots-actions.test.mjs \
  e2e/spa-dots-permissions.test.mjs e2e/spa-dots-communication.test.mjs \
  e2e/spa-dots-delegation.test.mjs e2e/spa-dots-identities.test.mjs
```

To refresh the delegated-history documentation screenshot, use a fresh isolated
`dev-ui` fixture: the capture creates actual Dots named Harald and Clara, runs
the same tested delegation, and records the verified route in the existing
capture manifest. It does not use a production database.

```bash
DOCS_DOT_DELEGATION_CAPTURE=docs/img/guide/dot-delegated-history.png \
  CHROMIUM_CHANNEL=chrome mise exec -- node --test e2e/spa-dots-delegation.test.mjs
```

The `e2e` mise task finds Playwright in the mise-installed `npm:@playwright/cli` package; export `PLAYWRIGHT_DIR` only to override it.

Set `CHROME_EXE` to an installed Chrome executable to use that browser instead of Playwright's bundled Chromium, or set `CHROMIUM_CHANNEL=chrome` to select the installed Chrome channel. `CHROME_EXE` selects the executable when both are set. For the personal priority suite, `TOKEN_PRIORITY_COOKIE` can select the dev-ui engineering session to exercise a non-admin ceiling. The personal token priority suite checks creation, editing, inheritance, the owner ceiling, cancellation, failed saves and mobile layout. The admin suite checks token priority save failures, backend error focus and visibility, and priority validation in the selected UI language with a different browser language.

Set `CHROMIUM_HEADED=1` to watch the browser locally:

```bash
CHROMIUM_HEADED=1 mise run e2e
# Exercise the installed Chrome browser (headless by default):
CHROMIUM_CHANNEL=chrome mise run e2e
```

Set `AIPLANE_URL=https://gw.dev` to point at a remote gateway (note: the authed/SPA-signin tests need the seeding endpoints below, so a remote target must be a debug build — in practice that means a local `mise run dev`).

## Session seeding (no OIDC needed)

The authed tests never do an OIDC dance. Two debug-only endpoints
(`rama_server::dev_seed`, compiled under `cfg(debug_assertions)`, absent from
release builds) sign a browser in directly:

- `GET /__dev/session` — signs in as the fixture user `alice@example.com`
  (creating her if absent) and completes setup on a fresh dev database.
  Idempotent and **delete-free**, so any test file can call it in `before()`
  even though `node --test` runs files in parallel.
- `GET /__dev/seed-session` — additionally resets the canonical fixture:
  exactly three tokens for alice (`Local laptop` active, `CI pipeline`
  revoked, `Production API` active, newest first). Because it *deletes*
  tokens, only `authed.test.mjs` may call it (before each test) — a
  concurrent reset would race the other files' assertions.

Both answer `303` with an ordinary `Set-Cookie: id=…` session.

## What's covered

- `/healthz`, `/readyz`, 404 routes; `/api/v0/me`, `/api/v0/tokens`: 401
  error envelope when anonymous; `/v1/chat/completions`: 401 without a
  valid bearer.
- The SPA at the root: client bundle boots and hydrates, the signed-out
  redirect into `/auth/login`, the signed-in identity rendering, and (with
  `dev-ui` as the server) a full chat turn streaming into the SPA.
- The anonymous funnel: `/` and protected pages bounce to `/login` with
  `return_to`; the login page's OIDC form.
- Authenticated `/settings` and `/settings/tokens`: identity + roles on the account
  tab, the canonical three token rows with active/revoked badges, token create
  (one-time `gwk_` plaintext banner), refused empty-name create, revoke
  flipping the row's badge via the SSE patch, sign-out.
- The authenticated chat scaffold: composer textarea, model picker, send
  button.
- The SPA at the root: client bundle boots and hydrates, the signed-out
  redirect into `/auth/login`, and the signed-in identity rendering.

## What's not covered yet

- **Tool-call loop end-to-end** — the runner is fully unit-tested in isolation, but driving the full proxy + wiremock-upstream + injection path from the browser belongs here too.

The organization identity scenario also captures the actual connected row and updates the screenshot manifest when `DOCS_ORG_DOT_IDENTITY_CAPTURE=docs/img/guide/organization-dot-identity.png` is set. It uses a synthetic seeded organization member, the real local OAuth callback and explicit saved selection.

## First-time setup notes

Chromium needs a few shared libs on Debian trixie:

```bash
sudo apt-get install -y libnss3 libnspr4 libatk1.0-0t64 libatk-bridge2.0-0t64 \
    libcups2t64 libdbus-1-3 libdrm2 libxkbcommon0 libxcomposite1 libxdamage1 \
    libxfixes3 libxrandr2 libgbm1 libpango-1.0-0 libcairo2 libasound2t0
```

And a one-time Chromium download (uses the mise-installed `npm:@playwright/cli`, located via `mise where` so it works wherever mise put it):

```bash
node "$(mise where 'npm:@playwright/cli')/lib/node_modules/@playwright/cli/node_modules/playwright/cli.js" install chromium
```

This downloads into Playwright's default browser cache (`~/.cache/ms-playwright` on Linux, `~/Library/Caches/ms-playwright` on macOS). On the shared CI/dev host, set `PLAYWRIGHT_BROWSERS_PATH=/var/host-cache/playwright/browsers` on both the download command and `mise run e2e` so the browser survives across project builds (the path lines up with `MISE_CACHE_DIR`).
