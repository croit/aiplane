// Shared bits for the e2e tests. Imports playwright from the bundled
// @playwright/cli mise tool (so we don't need a project-local node_modules).

import assert from "node:assert";

const PLAYWRIGHT_DIR = process.env.PLAYWRIGHT_DIR
  ?? "/var/host-cache/mise/installs/npm-playwright-cli/0.1.18/lib/node_modules/@playwright/cli/node_modules/playwright";

const { chromium } = await import(`${PLAYWRIGHT_DIR}/index.mjs`);

export const BASE = process.env.GATEWAY_URL ?? "http://localhost:8080";

export { chromium };

/// Wraps `chromium.launch` so every test file picks up the same options.
export async function launchBrowser() {
    return chromium.launch({
        // Honour CHROMIUM_HEADED=1 when iterating locally; defaults to headless.
        headless: process.env.CHROMIUM_HEADED !== "1",
        // The SPA picks its language from the `lang` cookie, then from
        // `navigator.languages` (see `web/src/lib/i18n.svelte.ts`). Without
        // this the suite would render in whatever language the machine
        // running it prefers, so a developer with a German desktop would see
        // different text than CI does. Pin it: tests that care about a
        // language set the cookie themselves.
        args: ["--lang=en-US"],
    });
}

/// Returns true when GET ${BASE}/healthz responds 200. Polled by tests so we
/// can fail fast with a clear "gateway isn't running" message instead of a
/// generic Playwright timeout.
export async function gatewayIsUp() {
    try {
        const r = await fetch(`${BASE}/healthz`, { signal: AbortSignal.timeout(2000) });
        return r.status === 200;
    } catch {
        return false;
    }
}

/// Assert the dev-only seeding endpoints exist, which also completes setup on
/// a fresh dev database. Uses `/__dev/session` — the delete-free variant — so
/// any test file can call it in `before()`, even while other files (which run
/// in parallel under `node --test`) are mid-assertion. Only `authed.test.mjs`
/// may call `/__dev/seed-session`, the resetting variant.
export async function ensureDevFixture() {
    const r = await fetch(`${BASE}/__dev/session`, { redirect: "manual" });
    assert.ok(
        r.status === 303,
        `\`/__dev/session\` did not answer 303 (got ${r.status}). Server must be a debug \
build (e.g. \`mise run dev\`) — the seeding endpoints do not exist in release binaries, \
where the path falls through to the router's 404.`,
    );
}

/// Mint a session for the fixture user (`alice@example.com`) through the
/// delete-free `/__dev/session` endpoint and return the `id` cookie value, for
/// tests that drive a Playwright context directly with `addCookies`.
export async function devSessionCookie() {
    const r = await fetch(`${BASE}/__dev/session`, { redirect: "manual" });
    assert.ok(
        r.status === 303,
        `\`/__dev/session\` did not answer 303 (got ${r.status}) — run \`mise run dev\` first.`,
    );
    const setCookie = r.headers.getSetCookie().find((c) => c.startsWith("id="));
    assert.ok(setCookie, "the seeding endpoint set no session cookie");
    const value = setCookie.split(";")[0].slice("id=".length);
    return value;
}
