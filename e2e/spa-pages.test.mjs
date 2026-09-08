// Page-by-page smoke over EVERY SPA surface (issue #22 P4/P5): each view must
// render its data-independent anchor label and answer without unexpected
// 4xx/5xx. Data creation is covered by the dedicated spa-*.test.mjs files;
// this file is the "did a page regress into nothing" net.

import { test, before, after } from "node:test";
import assert from "node:assert";

import { BASE, devSessionCookie, gatewayIsUp, launchBrowser } from "./helpers.mjs";

let browser;
// The admin views need an admin session. The /__dev fixture user is a plain
// user on gateways whose RBAC doesn't map it to admin, so an operator-level
// cookie can be supplied via GATEWAY_SESSION_COOKIE; without it the admin
// page tests SKIP instead of failing (a 403 there is the gateway working).
let adminCookie = process.env.GATEWAY_SESSION_COOKIE?.trim().replace(/^id=/, "") ?? null;
let adminOk = false;

before(async () => {
    assert.ok(await gatewayIsUp(), `gateway is not reachable at ${BASE}`);
    const probe = await fetch(`${BASE}/app`);
    assert.equal(probe.status, 200, "the SPA is not served at /app");
    adminOk = adminCookie
        ? (await fetch(`${BASE}/api/v0/admin/groups`, { headers: { cookie: `id=${adminCookie}` } })).ok
        : false;
    browser = await launchBrowser();
});

after(async () => {
    if (browser) await browser.close();
});

const PAGES = [
    ["/app/memory", "Add a memory"],
    ["/app/scheduled", "New scheduled action"],
    ["/app/webhooks", "New webhook"],
    ["/app/skills", "Skills"],
    ["/app/integrations", "Integrations"],
    ["/app/tokens", "Create token"],
    ["/app/usage", "Requests"],
    ["/app/tools", "Tools"],
    ["/app/admin/groups", "New group"],
    ["/app/admin/users", "Users"],
    ["/app/admin/models", "Feature defaults"],
    ["/app/admin/limits", "Add / update a rule"],
    ["/app/admin/settings", "Save section"],
    ["/app/admin/tokens", "Token register"],
    ["/app/admin/upstreams", "Pools"],
    ["/app/admin/skills", "Upload .skill"],
    ["/app/admin/connectors", "New connector"],
    ["/app/admin/comfyui", "Reload catalog"],
    ["/app/admin/rag", "RAG collections"],
];

for (const [path, label] of PAGES) {
    const isAdminPage = path.startsWith("/app/admin");
    (isAdminPage && !adminOk ? test.skip : test)(`page renders: ${path}${isAdminPage && !adminOk ? " (skipped: no admin session — set GATEWAY_SESSION_COOKIE)" : ""}`, async () => {
        const ctx = await browser.newContext({ colorScheme: "dark" });
        await ctx.addCookies([
            { name: "id", value: isAdminPage && adminCookie ? adminCookie : await devSessionCookie(), url: BASE },
        ]);
        const page = await ctx.newPage();
        const bad = [];
        page.on("response", (r) => {
            if (r.status() >= 400 && r.url().includes("/api/")) bad.push(`${r.status()} ${r.url()}`);
        });
        await page.goto(`${BASE}${path}`, { waitUntil: "networkidle", timeout: 15000 });
        await page.waitForSelector(`text=${label}`, { timeout: 5000 });
        assert.deepEqual(bad, [], `${path} answered with API errors: ${bad.join("; ")}`);
        await ctx.close();
    });
}
