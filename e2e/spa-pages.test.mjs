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
    const probe = await fetch(`${BASE}/`);
    assert.equal(probe.status, 200, "the SPA is not served at the root");
    adminOk = adminCookie
        ? (await fetch(`${BASE}/api/v0/admin/groups`, { headers: { cookie: `id=${adminCookie}` } })).ok
        : false;
    browser = await launchBrowser();
});

after(async () => {
    if (browser) await browser.close();
});

const PAGES = [
    ["/memory", "Add a memory"],
    ["/scheduled", "New scheduled action"],
    ["/webhooks", "New webhook"],
    ["/skills", "Skills"],
    ["/integrations", "Integrations"],
    ["/tokens", "Create token"],
    ["/usage", "Requests"],
    ["/tools", "Tools"],
    ["/admin/groups", "New group"],
    ["/admin/users", "Users"],
    ["/admin/models", "Default models"],
    ["/admin/limits", "Add or update a limit"],
    ["/admin/settings", "Save section"],
    ["/admin/tokens", "API tokens"],
    ["/admin/upstreams", "Pools"],
    ["/admin/skills", "Upload .skill"],
    ["/admin/connectors", "Connectors"],
    ["/admin/comfyui", "Reload catalog"],
    ["/admin/rag", "RAG collections"],
];

for (const [path, label] of PAGES) {
    const isAdminPage = path.startsWith("/admin");
    test(`page renders: ${path}`, async (t) => {
        // Decide to skip HERE, not in the test *name*: the module body runs
        // before before(), so `adminOk` was still false at declaration time
        // and every admin page skipped even with a working admin session.
        if (isAdminPage && !adminOk) {
            t.skip("no admin session — set GATEWAY_SESSION_COOKIE");
            return;
        }
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
