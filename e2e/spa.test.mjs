// The SvelteKit SPA (issue #22) served by the gateway at /app.
//
// The static shell itself is covered by the Rust tests (spa.rs unit +
// spa_routes integration) and `build-web` in CI. What a *browser* can catch
// that those can't is the client side: that the shell actually boots and
// that the signed-out redirect into the OIDC login fires. So these tests
// need a running gateway with GATEWAY_STATIC_DIR pointing at the built SPA
// (`mise run dev` sets that for you) — they run via `mise run e2e`.
//
// The signed-in test logs in through the debug-only, delete-free
// /__dev/session endpoint (same one the other files' before() hooks use),
// so this file never races the resetting /__dev/seed-session that
// authed.test.mjs owns.

import { test, before, after } from "node:test";
import assert from "node:assert";

import { BASE, devSessionCookie, gatewayIsUp, launchBrowser } from "./helpers.mjs";

let browser;

before(async () => {
    assert.ok(
        await gatewayIsUp(),
        `gateway is not reachable at ${BASE}; run \`mise run dev\` in another terminal`,
    );
    // Pre-flight: the SPA must be deployed (GATEWAY_STATIC_DIR set). An undeployed
    // SPA answers 503 (rama_server::spa) — distinguish that from a dead server.
    const probe = await fetch(`${BASE}/app`);
    assert.equal(
        probe.status,
        200,
        `the SPA is not served at ${BASE}/app (got ${probe.status}). ` +
            "Run \`mise run build-web\` and restart the gateway with GATEWAY_STATIC_DIR=target/frontend/build.",
    );
    browser = await launchBrowser();
});

after(async () => {
    if (browser) await browser.close();
});

test("the SPA shell loads at /app (client bundle boots, not the 404/503 fallback)", async () => {
    const ctx = await browser.newContext();
    // A signed-out shell redirects itself into the OIDC flow as soon as
    // /api/v0/me answers 401 (see the next test). Park that request — a
    // route handler that never fulfils — so the shell stays put: what this
    // test pins is the boot, not the redirect.
    await ctx.route("**/api/v0/me", () => {});
    const page = await ctx.newPage();
    await page.goto(`${BASE}/app`, { waitUntil: "domcontentloaded" });

    // The app header is only in the DOM once Svelte has mounted and hydrated —
    // a 503 "not deployed" or the router's 404 would have no such markup.
    await page.waitForSelector("text=LLM Gateway", { timeout: 5000 });
    await ctx.close();
});

test("a signed-out visitor is redirected into the OIDC login flow", async () => {
    const ctx = await browser.newContext();
    const page = await ctx.newPage();
    await page.goto(`${BASE}/app`, { waitUntil: "domcontentloaded" });

    // No session → /api/v0/me 401s → the layout bounces to /login, which
    // bounces to /auth/login. The dev gateway has no real IdP configured, so
    // the flow ends on the gateway's OIDC error page — but the *redirect chain*
    // is what we assert: the SPA must not sit on the unsigned shell.
    await page.waitForURL((u) => u.pathname.startsWith("/auth/"), { timeout: 5000 });
    await ctx.close();
});

test("a signed-in user sees their identity from GET /api/v0/me", async () => {
    const ctx = await browser.newContext();
    await ctx.addCookies([
        {
            name: "id",
            value: await devSessionCookie(),
            url: BASE,
        },
    ]);
    const page = await ctx.newPage();
    await page.goto(`${BASE}/app`, { waitUntil: "networkidle" });

    // The /api/v0/me value renders into the header (badge + sign-out
    // affordance only exist for a known identity).
    await page.waitForSelector("text=alice@example.com", { timeout: 5000 });
    await page.waitForSelector('button:has-text("Sign out")', { timeout: 5000 });
    await ctx.close();
});
