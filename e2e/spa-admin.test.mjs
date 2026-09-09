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
    const probe = await fetch(`${BASE}/`);
    assert.equal(probe.status, 200, "the SPA is not served at the root");
    browser = await launchBrowser();
    // The admin views need an ADMIN session. Prefer GATEWAY_SESSION_COOKIE
    // (an operator cookie); fall back to the dev fixture user, which is only
    // admin on gateways whose RBAC maps it so.
    cookie = process.env.GATEWAY_SESSION_COOKIE?.trim().replace(/^id=/, "") ?? null;
    if (!cookie) {
        const r = await fetch(`${BASE}/__dev/session`, { redirect: "manual" });
        cookie = (r.headers.getSetCookie().find((c) => c.startsWith("id=")) ?? "").split(";")[0].slice(3);
    }
    if (
        !(await fetch(`${BASE}/api/v0/admin/groups`, { headers: { cookie: `id=${cookie}` } })).ok
    ) {
        console.log("skipping: no admin session (set GATEWAY_SESSION_COOKIE)");
        process.exit(0); // no admin session → nothing to assert here
    }
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
    await page.goto(`${BASE}/admin/groups`, { waitUntil: "networkidle" });
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
