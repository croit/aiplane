// The SvelteKit SPA (issue #22) served by the gateway at the root.
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
    const probe = await fetch(`${BASE}/`);
    assert.equal(
        probe.status,
        200,
        `the SPA is not served at ${BASE} (got ${probe.status}). ` +
            "Run \`mise run build-web\` and restart the gateway with GATEWAY_STATIC_DIR=target/frontend/build.",
    );
    browser = await launchBrowser();
});

after(async () => {
    if (browser) await browser.close();
});

test("the SPA shell loads at the root (client bundle boots, not the 404/503 fallback)", async () => {
    const ctx = await browser.newContext();
    // A signed-out shell redirects itself into the OIDC flow as soon as
    // /api/v0/me answers 401 (see the next test). Park that request — a
    // route handler that never fulfils — so the shell stays put: what this
    // test pins is the boot, not the redirect.
    await ctx.route("**/api/v0/me", () => {});
    const page = await ctx.newPage();
    await page.goto(`${BASE}/`, { waitUntil: "domcontentloaded" });

    // The app header is only in the DOM once Svelte has mounted and hydrated —
    // a 503 "not deployed" or the router's 404 would have no such markup.
    await page.waitForSelector("text=LLM Gateway", { timeout: 5000 });
    await ctx.close();
});

test("a signed-out visitor is redirected into the OIDC login flow", async (t) => {
    const ctx = await browser.newContext();
    const page = await ctx.newPage();
    await page.goto(`${BASE}/`, { waitUntil: "domcontentloaded" });

    // No session → /api/v0/me 401s → the layout bounces to /auth/login.
    // With a REAL provider configured the chain continues off-site (authentik
    // & co), so the /auth/ assertion only holds on the OIDC-less stub.
    await page
        .waitForURL((u) => u.pathname.startsWith("/auth/"), { timeout: 5000 })
        .catch(() => t.skip("a real OIDC provider is configured — the flow leaves the origin"));
    await ctx.close();
});

test("the SPA is a PWA: manifest, service worker, and push wiring", async () => {
    const ctx = await browser.newContext();
    await ctx.addCookies([
        { name: "id", value: await devSessionCookie(), url: BASE },
    ]);
    // Serve the PWA files at all (the build ships them into static/).
    for (const path of ["/sw.js", "/manifest.webmanifest"]) {
        const r = await fetch(`${BASE}${path}`);
        assert.equal(r.status, 200, `${path} must be served`);
    }

    const page = await ctx.newPage();
    await page.goto(`${BASE}/`, { waitUntil: "networkidle" });
    // The manifest is linked…
    const manifest = page.locator('link[rel="manifest"]');
    assert.equal(await manifest.count(), 1);
    // …and the service worker registers and activates. (Asserted via
    // getRegistrations rather than `serviceWorker.ready`, which never
    // resolves in the headless shell even with an activated worker.)
    const sw = await page.evaluate(async () => {
        const [reg] = await navigator.serviceWorker.getRegistrations();
        return reg?.active
            ? { scope: reg.scope, state: reg.active.state, script: reg.active.scriptURL }
            : null;
    });
    assert.ok(sw, "the SPA service worker must register");
    assert.match(sw.scope, /\/$/, "the SW must control the root scope");
    assert.equal(sw.state, "activated");
    assert.match(sw.script, /\/sw\.js$/);
    await ctx.close();
});

test("a signed-in user sees their identity from GET /api/v0/me", async (t) => {
    const ctx = await browser.newContext();
    await ctx.addCookies([
        {
            name: "id",
            value: await devSessionCookie(),
            url: BASE,
        },
    ]);
    const page = await ctx.newPage();
    // The root funnels into the chat surface — follow it and wait for the
    // identity to render in the sidebar footer.
    await page.goto(`${BASE}/chat`, { waitUntil: "networkidle" });

    // The /api/v0/me value renders into the sidebar footer (email + sign-out
    // affordance only exist for a known identity).
    await page.waitForSelector("text=alice@example.com", { timeout: 10_000 });
    await page.waitForSelector('button[aria-label="Sign out"]', { timeout: 5000 });
    await ctx.close();
});

/// A deep link, not just `/`: the guard is in the layout, so every client
/// route inherits it — but only if the layout actually runs before the page
/// renders. A route that painted its own content first would leak whatever it
/// had already fetched.
test("a deep protected route bounces an anonymous visitor too", async (t) => {
    const ctx = await browser.newContext();
    const page = await ctx.newPage();
    await page.goto(`${BASE}/tokens`, { waitUntil: "domcontentloaded" });
    await page
        .waitForURL((u) => u.pathname.startsWith("/auth/"), { timeout: 5000 })
        .catch(() => t.skip("a real OIDC provider is configured — the flow leaves the origin"));
    await ctx.close();
});

/// Signing out has to end the session server-side, not just navigate away.
/// The check that matters is the second one: after the click, the API the SPA
/// runs on must refuse the old cookie.
test("signing out ends the session", async () => {
    const ctx = await browser.newContext();
    const cookie = await devSessionCookie();
    await ctx.addCookies([{ name: "id", value: cookie, url: BASE }]);
    const page = await ctx.newPage();
    await page.goto(`${BASE}/chat`, { waitUntil: "networkidle" });
    await page.waitForSelector('button[aria-label="Sign out"]', { timeout: 5000 });
    await page.click('button[aria-label="Sign out"]');

    // The cookie is dead: a fresh request carrying it is refused.
    const res = await fetch(`${BASE}/api/v0/me`, { headers: { cookie: `id=${cookie}` } });
    assert.equal(res.status, 401, "the session must be gone server-side, not just in the tab");
    await ctx.close();
});
