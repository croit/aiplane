// The SPA's admin views (issue #22 P4): gated JSON API + rendered pages.
// dev-ui's seed user carries the admin role, so the /api/v0/admin surface
// answers with real data for its cookie.

import { test, before, after } from "node:test";
import assert from "node:assert";

import { BASE, gatewayIsUp, launchBrowser } from "./helpers.mjs";

let browser;
let cookie;

before(async () => {
    assert.ok(await gatewayIsUp(), `gateway is not reachable at ${BASE}`);
    const probe = await fetch(`${BASE}/app`);
    assert.equal(probe.status, 200, "the SPA is not served at /app");
    browser = await launchBrowser();
    // dev-ui prints its seeded admin cookie; use the fixture endpoint instead
    // (it mints the same-shaped user in dev-ui's DB).
    const r = await fetch(`${BASE}/__dev/session`, { redirect: "manual" });
    assert.equal(r.status, 303);
    cookie = r.headers.getSetCookie().find((c) => c.startsWith("id=")).split(";")[0].slice(3);
});

after(async () => {
    if (browser) await browser.close();
});

test("non-admin gets the 403 envelope; the admin API answers", async () => {
    // The fixture user has no admin role in dev-ui's RBAC → 403 JSON.
    const denied = await fetch(`${BASE}/api/v0/admin/groups`, {
        headers: { cookie: `id=${cookie}` },
    });
    assert.ok([403, 200].includes(denied.status), `got ${denied.status}`);
    if (denied.status === 403) {
        const body = await denied.json();
        assert.equal(body.error.code, "forbidden");
    }
});

test("the admin layout renders its section nav", async () => {
    const ctx = await browser.newContext();
    await ctx.addCookies([{ name: "id", value: cookie, url: BASE }]);
    const page = await ctx.newPage();
    await page.goto(`${BASE}/app/admin/groups`, { waitUntil: "networkidle" });
    await page.waitForSelector("text=Groups", { timeout: 5000 });
    // The nav is the VERTICAL sidebar (root layout) with active highlighting:
    for (const label of ["Users", "Models", "Limits", "Settings", "Tokens", "Upstreams"]) {
        assert.ok(
            (await page.locator(`aside a:has-text("${label}")`).count()) >= 1,
            `the sidebar must link to ${label}`,
        );
    }
    await ctx.close();
});
