# e2e/

End-to-end browser tests for the gateway, driven by Playwright through Node's built-in test runner. Zero project-level `node_modules` — the tests import `playwright` directly out of the mise-installed `npm:@playwright/cli` tool.

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
- `spa.test.mjs` — the SvelteKit SPA at `/app`: shell boot, signed-out redirect into OIDC, signed-in identity from `GET /api/v0/me`.
- `spa-chat.test.mjs` — the SPA chat round trip (issue #22 P2): create a conversation, submit a turn, watch the reply stream in over the JSON-SSE event protocol. Needs a gateway with a chat upstream — `GATEWAY_STATIC_DIR=target/frontend/build mise run dev-ui` — and skips with a pointer at that command when the gateway has no pools.

The `e2e` mise task points `PLAYWRIGHT_DIR` at the mise-installed `npm:@playwright/cli` tool automatically; export it yourself only to override.

Set `CHROMIUM_HEADED=1` to watch the browser locally:

```bash
CHROMIUM_HEADED=1 mise run e2e
```

Set `GATEWAY_URL=https://gw.dev` to point at a remote gateway (note: the authed/SPA-signin tests need the seeding endpoints below, so a remote target must be a debug build — in practice that means a local `mise run dev`).

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
- The SPA at `/app`: client bundle boots and hydrates, the signed-out
  redirect into `/auth/login`, the signed-in identity rendering, and (with
  `dev-ui` as the server) a full chat turn streaming into the SPA.
- The anonymous funnel: `/` and protected pages bounce to `/login` with
  `return_to`; the login page's OIDC form.
- Authenticated `/tokens`: identity + roles in the app shell and account
  card, the canonical three rows with active/revoked badges, token create
  (one-time `gwk_` plaintext banner), refused empty-name create, revoke
  flipping the row's badge via the SSE patch, sign-out.
- The authenticated chat scaffold: composer textarea, model picker, send
  button.
- The SPA at `/app`: client bundle boots and hydrates, the signed-out
  redirect into `/auth/login`, and the signed-in identity rendering.

## What's not covered yet

- **Tool-call loop end-to-end** — the runner is fully unit-tested in isolation, but driving the full proxy + wiremock-upstream + injection path from the browser belongs here too.

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
