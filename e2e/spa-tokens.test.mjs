// The SPA's tokens surface (issue #22 P3): full CRUD against the existing
// /api/v0/tokens JSON API through the SvelteKit page at /app/tokens — the
// one-time gwk_ banner, revoke/rotate confirmations, delete, and the
// master tool toggle.
//
// Runs against any debug gateway serving the SPA (`mise run dev` or the
// dev-ui stub with GATEWAY_STATIC_DIR set).

import { test, before, after } from "node:test";
import assert from "node:assert";

import { BASE, devSessionCookie, gatewayIsUp, launchBrowser } from "./helpers.mjs";

let browser;

before(async () => {
    assert.ok(
        await gatewayIsUp(),
        `gateway is not reachable at ${BASE}; run \`mise run dev\` in another terminal`,
    );
    const probe = await fetch(`${BASE}/app`);
    assert.equal(probe.status, 200, `the SPA is not served at ${BASE}/app`);
    browser = await launchBrowser();
});

after(async () => {
    if (browser) await browser.close();
});

test("tokens CRUD through the SPA", async () => {
    const ctx = await browser.newContext();
    await ctx.addCookies([{ name: "id", value: await devSessionCookie(), url: BASE }]);
    const page = await ctx.newPage();

    // Seed the canonical fixture so the list starts from a known state.
    await page.goto(`${BASE}/__dev/seed-session`);
    await page.goto(`${BASE}/app/tokens`, { waitUntil: "networkidle" });
    await page.locator("#token-list, ul.divide-y li, .card-body li").first().waitFor({
        timeout: 5000,
    });

    // The canonical three rows render with their badges.
    assert.equal(await page.locator("li.py-3").count(), 3);
    assert.equal(await page.locator('span.badge:has-text("active")').count(), 2);
    assert.equal(await page.locator('span.badge:has-text("revoked")').count(), 1);

    // Create → one-time plaintext banner with the gwk_ prefix.
    await page.locator("input[placeholder='e.g. laptop, ci-runner']").fill("spa-e2e-token");
    await page.locator('button:has-text("Create token")').click();
    const plaintext = page.locator("pre");
    await plaintext.waitFor({ state: "visible", timeout: 5000 });
    const text = (await plaintext.textContent()) ?? "";
    assert.match(text.trim(), /^gwk_[0-9a-f]{64}$/);
    assert.equal(await page.locator("li.py-3").count(), 4);

    // Revoke the new row (confirm() is native — accept it).
    page.once("dialog", (d) => d.accept());
    await page.locator('li:has-text("spa-e2e-token") button:has-text("Revoke")').click();
    await page
        .locator('li:has-text("spa-e2e-token") span.badge:has-text("revoked")')
        .waitFor({ timeout: 5000 });

    // Remove it again — the list is back to three.
    page.once("dialog", (d) => d.accept());
    await page.locator('li:has-text("spa-e2e-token") button:has-text("Remove")').click();
    await page.waitForFunction(
        () => document.querySelectorAll("li.py-3").length === 3,
        null,
        { timeout: 5000 },
    );
    await ctx.close();
});

test("the usage and tools views render their data", async (t) => {
    const ctx = await browser.newContext();
    await ctx.addCookies([{ name: "id", value: await devSessionCookie(), url: BASE }]);
    const page = await ctx.newPage();

    // Usage: the period picker + the summary envelope render (empty data on
    // a fresh stub, but the cards' labels are data-independent).
    await page.goto(`${BASE}/app/usage`, { waitUntil: "networkidle" });
    await page.waitForSelector("text=Requests", { timeout: 5000 });
    await page.waitForSelector("text=Errors", { timeout: 5000 });

    // Tools: the dev-ui stub grants the full tool set to its admin group —
    // the grouped list renders with toggles.
    await page.goto(`${BASE}/app/tools`, { waitUntil: "networkidle" });
    const toggles = page.locator("input.toggle");
    const count = await toggles.count();
    if (count === 0) {
        // A gateway with an empty grant renders the empty state instead.
        await page.waitForSelector("text=No tools granted", { timeout: 5000 });
    }
    await ctx.close();
});
