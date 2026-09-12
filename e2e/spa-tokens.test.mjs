// The SPA's tokens surface (issue #22 P3): full CRUD against the existing
// /api/v0/tokens JSON API through the SvelteKit page at /tokens — the
// one-time gwk_ banner, revoke/rotate confirmations, delete, and the
// master tool toggle.
//
// Runs against any debug gateway serving the SPA (`mise run dev` or the
// dev-ui stub with GATEWAY_STATIC_DIR set).

import { test, before, after } from "node:test";
import assert from "node:assert";

import { BASE, devSessionCookie, gatewayIsUp, launchBrowser } from "./helpers.mjs";

let browser;

/**
 * Open one of a token row's editors. Each used to be a `<details>` that
 * expanded in place; they are dialogs now, so "open" means clicking the row's
 * Edit button and waiting for the dialog to be on screen.
 */
async function openEditor(page, row, label) {
    await row.getByText(label, { exact: true }).locator("xpath=..").getByRole("button", { name: "Edit", exact: true }).click();
    const modal = page.locator("dialog[open]");
    await modal.waitFor();
    return modal;
}

async function closeEditor(page) {
    const modal = page.locator("dialog[open]");
    if (await modal.count()) {
        // Scoped to the footer: the backdrop carries its own close control,
        // the same shape every dialog in the app uses.
        await modal.locator(".modal-action").getByRole("button", { name: "Close", exact: true }).click();
        await modal.waitFor({ state: "hidden" });
    }
}

before(async () => {
    assert.ok(
        await gatewayIsUp(),
        `gateway is not reachable at ${BASE}; run \`mise run dev\` in another terminal`,
    );
    const probe = await fetch(`${BASE}/`);
    assert.equal(probe.status, 200, `the SPA is not served at ${BASE}/`);
    browser = await launchBrowser();
});

after(async () => {
    if (browser) await browser.close();
});

test("tokens preserve scopes, quotas, identity, and CRUD on mobile", async () => {
    const ctx = await browser.newContext({ viewport: { width: 390, height: 844 } });
    await ctx.addCookies([{ name: "id", value: await devSessionCookie(), url: BASE }]);
    const page = await ctx.newPage();

    // Seed the canonical fixture so the list starts from a known state.
    await page.goto(`${BASE}/__dev/seed-session`);
    await page.goto(`${BASE}/tokens`, { waitUntil: "networkidle" });
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

    const row = page.locator("li.py-3").filter({ hasText: "spa-e2e-token" });
    let saved = page.waitForResponse((response) => response.url().includes("/tools") && response.request().method() === "PUT");
    let refreshed = page.waitForResponse((response) => response.url().endsWith("/api/v0/tokens/details") && response.request().method() === "GET");
    await row.getByRole("checkbox", { name: "Tool use", exact: true }).check();
    assert.equal((await saved).status(), 200);
    assert.equal((await refreshed).status(), 200);
    let capabilities = await openEditor(page, row, "Capabilities");
    const firstTool = capabilities.getByRole("checkbox", { name: /Toggle/ }).first();
    await firstTool.waitFor();
    saved = page.waitForResponse((response) => response.url().includes("/tools") && response.request().method() === "PUT");
    refreshed = page.waitForResponse((response) => response.url().endsWith("/api/v0/tokens/details") && response.request().method() === "GET");
    await firstTool.uncheck();
    assert.equal((await saved).status(), 200);
    assert.equal((await refreshed).status(), 200);
    assert.equal(await capabilities.getByRole("checkbox", { name: /Toggle/ }).first().isChecked(), false);
    await closeEditor(page);
    const mcp = row.getByRole("checkbox", { name: "Allow ask-mode MCP tools over API", exact: true });
    await mcp.waitFor();
    await page.waitForTimeout(100);
    assert.equal(await mcp.isEnabled(), true);
    const initialMcp = await mcp.isChecked();
    saved = page.waitForResponse((response) => response.url().includes("/mcp-policy") && response.request().method() === "PUT");
    refreshed = page.waitForResponse((response) => response.url().endsWith("/api/v0/tokens/details") && response.request().method() === "GET");
    if (initialMcp) await mcp.uncheck();
    else await mcp.check();
    assert.equal((await saved).status(), 200);
    assert.equal((await refreshed).status(), 200);
    assert.equal(await row.getByRole("checkbox", { name: "Allow ask-mode MCP tools over API", exact: true }).isChecked(), !initialMcp);

    const models = await openEditor(page, row, "Models: all");
    await models.getByText("Limit this token to specific models", { exact: true }).click();
    await models.locator("input.checkbox").first().check();
    await models.getByRole("button", { name: "Save models", exact: true }).click();
    await row.getByText("Models: 1 selected", { exact: true }).waitFor();

    const quota = await openEditor(page, row, "Quota: none");
    await quota.getByRole("spinbutton", { name: "max", exact: true }).fill("42");
    await quota.getByRole("button", { name: "Add quota", exact: true }).click();
    // Close first: the dialog's own title carries the same summary text, so
    // asserting on the row while it is open is ambiguous.
    await quota.getByText("42 requests / day", { exact: false }).waitFor();
    await closeEditor(page);
    await row.getByText("Quota: 1 rule(s)", { exact: true }).waitFor();
    await page.getByRole("heading", { name: "Account", exact: true }).waitFor();
    assert.ok(await page.locator("main").evaluate((element) => element.scrollWidth <= element.clientWidth));

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
    const ctx = await browser.newContext({
        viewport: { width: 390, height: 844 },
        geolocation: { latitude: 48.137, longitude: 11.575 },
        permissions: ["geolocation"],
    });
    const usageCookie = process.env.GATEWAY_SESSION_COOKIE?.trim().replace(/^id=/, "") ?? await devSessionCookie();
    await ctx.addCookies([{ name: "id", value: usageCookie, url: BASE }]);
    const page = await ctx.newPage();

    await page.goto(`${BASE}/usage`, { waitUntil: "networkidle" });
    await page.getByRole("heading", { name: "Your usage", exact: true }).waitFor();
    const periodChanged = page.waitForResponse((response) => response.url().includes("/api/v0/usage?period=24h"));
    await page.getByLabel("Period").selectOption("24h");
    assert.equal((await periodChanged).status(), 200);
    await page.getByRole("heading", { name: "Your limits", exact: true }).waitFor();
    for (const label of ["Source", "Backend", "Token"]) {
        await page.getByLabel(label, { exact: true }).waitFor();
    }
    for (const heading of ["By backend", "By source", "By model", "By API token"]) {
        await page.getByRole("heading", { name: heading, exact: true }).waitFor();
    }
    const filtered = page.waitForResponse((response) => response.url().includes("source=chat"));
    await page.getByLabel("Source", { exact: true }).selectOption("chat");
    assert.equal((await filtered).status(), 200);
    assert.match(page.url(), /source=chat/);
    const allUsers = page.getByRole("button", { name: "All users", exact: true });
    if (await allUsers.count()) {
        const widened = page.waitForResponse((response) => response.url().includes("scope=all"));
        await allUsers.click();
        assert.equal((await widened).status(), 200);
        await page.getByRole("heading", { name: "Usage — all users", exact: true }).waitFor();
        await page.getByRole("heading", { name: "By user", exact: true }).waitFor();
        await page.getByText("active in range", { exact: true }).waitFor();
    }
    assert.ok(await page.locator("main").evaluate((element) => element.scrollWidth <= element.clientWidth));

    // Tools: the dev-ui stub grants the full tool set to its admin group —
    // the grouped list renders with toggles.
    await page.goto(`${BASE}/tools`, { waitUntil: "networkidle" });
    const toggles = page.locator("input.toggle");
    const count = await toggles.count();
    if (count === 0) {
        // A gateway with an empty grant renders the empty state instead.
        await page.waitForSelector("text=No tools granted", { timeout: 5000 });
    }
    await page.getByRole("heading", { name: "Location", exact: true }).waitFor();
    assert.equal(await page.locator("code", { hasText: "get_user_location" }).count(), 1);
    const shared = page.waitForResponse((response) => response.url().endsWith("/api/v0/me/location") && response.request().method() === "POST");
    await page.getByRole("button", { name: "Share precise location", exact: true }).click();
    assert.equal((await shared).status(), 200);
    await page.getByText(/Shared — accuracy/).waitFor();
    const forgotten = page.waitForResponse((response) => response.url().endsWith("/api/v0/me/location") && response.request().method() === "DELETE");
    await page.getByRole("button", { name: "Stop sharing", exact: true }).click();
    assert.equal((await forgotten).status(), 200);
    await page.getByText("Not shared.", { exact: true }).waitFor();
    assert.ok(await page.locator("main").evaluate((element) => element.scrollWidth <= element.clientWidth));
    await ctx.close();
});
