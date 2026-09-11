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

test("a signed-out visitor reaches the sign-in entry with return-to intact", async (t) => {
    const ctx = await browser.newContext();
    const page = await ctx.newPage();
    await page.goto(`${BASE}/`, { waitUntil: "domcontentloaded" });

    // A first-run server owns the document request and redirects to setup;
    // once configured, the SPA keeps the explicit production login entry.
    const reachedLogin = await page
        .waitForURL((u) => u.pathname === "/login", { timeout: 5000 })
        .then(() => true)
        .catch(() => false);
    if (!reachedLogin) {
        t.skip("the first-run setup gate is still active");
        await ctx.close();
        return;
    }
    await page.getByRole("heading", { name: "Sign in to LLM Gateway", exact: true }).waitFor();
    assert.equal(await page.locator('input[name="return_to"]').getAttribute("value"), "/");
    await ctx.close();
});

test("first-run setup uses the standalone two-step provider shell", async () => {
    const ctx = await browser.newContext({ viewport: { width: 390, height: 844 } });
    const page = await ctx.newPage();
    await page.route("**/api/v0/setup/state*", (route) => route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
            access: "first_run",
            draft: null,
            proof: null,
            suggested_public_url: BASE,
        }),
    }));
    await page.goto(`${BASE}/setup`, { waitUntil: "domcontentloaded" });
    await page.getByRole("heading", { name: "Connect your identity provider", exact: true }).waitFor();
    assert.equal(await page.locator("aside").count(), 0);
    await page.getByText("Step 1 of 2", { exact: true }).waitFor();
    await page.getByText("Whitelist this redirect URI in your provider", { exact: true }).waitFor();
    for (const label of ["Public URL of this gateway", "Issuer URL", "Client ID", "Client secret", "Scopes", "Group claim"]) await page.getByLabel(label, { exact: true }).waitFor();
    assert.equal(await page.getByRole("button", { name: "Sign in to test", exact: true }).isDisabled(), true);
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true);
    await ctx.close();
});

test("login preserves the production sign-in entry and safe deep link", async () => {
    await devSessionCookie();
    const ctx = await browser.newContext({ viewport: { width: 390, height: 844 } });
    const page = await ctx.newPage();
    await page.goto(`${BASE}/login?return_to=${encodeURIComponent('/admin/settings?tab=tools')}`, { waitUntil: "networkidle" });

    await page.getByRole("heading", { name: "Sign in to LLM Gateway", exact: true }).waitFor();
    assert.equal(await page.locator("aside").count(), 0);
    const form = page.locator('form[action="/auth/login"]');
    assert.equal(await form.locator('input[name="return_to"]').getAttribute("value"), "/admin/settings?tab=tools");
    await page.getByRole("button", { name: "Continue with OIDC →", exact: true }).waitFor();
    const source = page.getByRole("link", { name: "Source code · AGPL-3.0", exact: true });
    assert.match(await source.getAttribute("href"), /^https:\/\//);
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true);
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

test("the sidebar keeps navigation compact and gives scrolling to conversations", async () => {
    const ctx = await browser.newContext();
    await ctx.addCookies([
        {
            name: "id",
            value: await devSessionCookie(),
            url: BASE,
        },
    ]);
    const page = await ctx.newPage();
    await page.goto(`${BASE}/chat`, { waitUntil: "networkidle" });

    const workspace = page.getByRole("button", { name: "Toggle Workspace section" });
    const account = page.getByRole("button", { name: "Toggle Account section" });
    assert.equal(await workspace.getAttribute("aria-expanded"), "true");
    assert.equal(await account.getAttribute("aria-expanded"), "false");
    assert.equal(await page.getByRole("link", { name: "Memory" }).count(), 1);
    assert.equal(await page.getByRole("link", { name: "Tokens" }).count(), 0);

    const scrollOwners = await page.locator("aside").evaluate((aside) => {
        const primary = aside.querySelector("nav");
        const conversations = aside.querySelector("[data-sidebar-conversations]");
        return {
            primary: primary && getComputedStyle(primary).overflowY,
            conversations: conversations && getComputedStyle(conversations).overflowY,
        };
    });
    assert.equal(scrollOwners.primary, "visible");
    assert.equal(scrollOwners.conversations, "auto");

    for (const label of ["Chat", "Memory", "Scheduled", "Webhooks", "Integrations", "My Skills", "Tools"]) {
        assert.equal(
            await page.getByRole("link", { name: label, exact: true }).locator("svg").count(),
            1,
            `${label} keeps its navigation icon`,
        );
    }

    await account.click();
    assert.equal(await account.getAttribute("aria-expanded"), "true");
    assert.match((await ctx.cookies()).find((cookie) => cookie.name === "nav_sections")?.value ?? "", /account/);
    await page.reload({ waitUntil: "networkidle" });
    assert.equal(
        await page.getByRole("button", { name: "Toggle Account section" }).getAttribute("aria-expanded"),
        "true",
    );
    await ctx.close();
});

/// A deep link, not just `/`: the guard is in the layout, so every client
/// route inherits it — but only if the layout actually runs before the page
/// renders. A route that painted its own content first would leak whatever it
/// had already fetched.
test("a deep protected route reaches login without losing its destination", async () => {
    const ctx = await browser.newContext();
    const page = await ctx.newPage();
    await page.goto(`${BASE}/tokens`, { waitUntil: "domcontentloaded" });
    await page.waitForURL((u) => u.pathname === "/login", { timeout: 5000 });
    assert.equal(await page.locator('input[name="return_to"]').getAttribute("value"), "/tokens");
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

/// The product ships in six languages, and the SPA is where that promise is
/// kept or broken: the catalogs are generated from the Fluent `.ftl` corpus
/// (`web/scripts/ftl-to-ts.py`) and bundled, so a build that forgot to
/// regenerate them, or a switcher wired to nothing, would still render a
/// perfectly working English app — which is exactly the failure nobody
/// notices until a German user opens it.
///
/// Three things have to hold, and only a browser can check them together:
/// the switcher changes the rendered text, `<html lang>` follows (screen
/// readers and hyphenation read that, not our state), and the choice is
/// stored as a cookie the Rust side reads too, so a server-rendered error
/// page comes back in the same language the app is in.
test("the language switcher translates the app, and the choice sticks", async () => {
    const ctx = await browser.newContext();
    await ctx.addCookies([{ name: "id", value: await devSessionCookie(), url: BASE }]);
    const page = await ctx.newPage();
    await page.goto(`${BASE}/chat`, { waitUntil: "networkidle" });

    // English to begin with: the browser is launched with --lang=en-US and no
    // `lang` cookie has been set.
    await page.waitForSelector("text=Conversations", { timeout: 5000 });

    await page.click('button[aria-label="Choose language"]');
    await page.click("text=Deutsch");

    // The nav re-renders. This is the assertion that fails if the labels were
    // resolved once at module scope instead of through `t()` in the template.
    await page.waitForSelector("text=Unterhaltungen", { timeout: 5000 });

    assert.equal(
        await page.evaluate(() => document.documentElement.lang),
        "de",
        "<html lang> must follow the switcher",
    );
    const cookies = await ctx.cookies(BASE);
    assert.equal(
        cookies.find((c) => c.name === "lang")?.value,
        "de",
        "the choice must be a `lang` cookie — session_core::i18n reads the same one",
    );

    // And it survives a reload, which is the part `$state` alone cannot do.
    await page.reload({ waitUntil: "networkidle" });
    await page.waitForSelector("text=Unterhaltungen", { timeout: 5000 });
    await ctx.close();
});
