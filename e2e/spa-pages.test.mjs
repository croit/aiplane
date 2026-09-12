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
    ["/memory", "Add a memory", "Memory — LLM Gateway"],
    ["/scheduled", "Create scheduled action", "Scheduled actions — LLM Gateway"],
    ["/webhooks", "Create webhook", "Webhooks — LLM Gateway"],
    ["/skills", "Skills", "My Skills — LLM Gateway"],
    ["/integrations", "Integrations", "Integrations — LLM Gateway"],
    ["/tokens", "Create token", "API tokens — LLM Gateway"],
    ["/usage", "Requests", "Your usage — LLM Gateway"],
    ["/tools", "Tools", "Tools — LLM Gateway"],
    ["/admin/groups", "New group", "Gateway groups"],
    ["/admin/users", "Users", "Users — LLM Gateway"],
    ["/admin/models", "Default models", "Models — LLM Gateway"],
    ["/admin/limits", "Add or update a limit", "Rate limits & quotas"],
    ["/admin/settings", "Save section", "Settings"],
    ["/admin/tokens", "API tokens", "API tokens"],
    ["/admin/upstreams", "Pools", "Upstreams — LLM Gateway"],
    ["/admin/skills", "Upload .skill", "Skills — LLM Gateway"],
    ["/admin/connectors", "Connectors", "Connectors — LLM Gateway"],
    ["/admin/comfyui", "Reload catalog", "ComfyUI — Workflow catalog"],
    ["/rag", "Index a new collection", "RAG collections — LLM Gateway", true],
    ["/rag/new", "Queue indexing", "Index a new collection", true],
    ["/rag/profiles", "New profile", "Extraction profiles — LLM Gateway", true],
];

for (const [path, label, expectedTitle, explicitlyAdmin = false] of PAGES) {
    const isAdminPage = explicitlyAdmin || path.startsWith("/admin");
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
        await page.goto(`${BASE}${path}`, { waitUntil: "domcontentloaded", timeout: 15000 });
        await page.waitForSelector(`text=${label}`, { timeout: 5000 });
        assert.equal(await page.title(), expectedTitle);
        assert.deepEqual(bad, [], `${path} answered with API errors: ${bad.join("; ")}`);
        await ctx.close();
    });
}

test("RAG controls fit a mobile viewport", async (t) => {
    if (!adminOk || !adminCookie) {
        t.skip("no admin session — set GATEWAY_SESSION_COOKIE");
        return;
    }
    const ctx = await browser.newContext({ viewport: { width: 390, height: 844 }, colorScheme: "dark" });
    await ctx.addCookies([{ name: "id", value: adminCookie, url: BASE }]);
    const page = await ctx.newPage();
    // The collection form moved to its own route, so the mobile-width guard
    // follows it there; /rag itself is now just the list.
    for (const [path, heading] of [["/rag", "Configured collections"], ["/rag/new", "Index a new collection"], ["/rag/profiles", "New profile"]]) {
        await page.goto(`${BASE}${path}`, { waitUntil: "networkidle" });
        await page.getByRole("heading", { name: heading }).waitFor();
        const main = page.locator("main");
        assert.ok(
            await main.evaluate((element) => element.scrollWidth <= element.clientWidth),
            `${path} controls must not create horizontal content scrolling`,
        );
    }
    await ctx.close();
});

test("memory keeps its three semantic sections and direct editing", async () => {
    const ctx = await browser.newContext({ viewport: { width: 390, height: 844 }, colorScheme: "dark" });
    await ctx.addCookies([{ name: "id", value: await devSessionCookie(), url: BASE }]);
    const page = await ctx.newPage();
    await page.goto(`${BASE}/memory`, { waitUntil: "networkidle" });
    for (const heading of ["Preferences", "Project context", "Facts"]) {
        assert.equal(await page.getByRole("heading", { name: heading, exact: true }).count(), 1);
    }
    assert.equal(await page.getByRole("button", { name: "Remember", exact: true }).count(), 1);
    assert.equal(await page.getByRole("button", { name: "Edit", exact: true }).count(), 0);
    assert.ok(await page.locator("main").evaluate((element) => element.scrollWidth <= element.clientWidth));
    await ctx.close();
});

test("integrations preserve token connection, health feedback, and mobile layout", async (t) => {
    if (!adminOk || !adminCookie) {
        t.skip("no admin session — set GATEWAY_SESSION_COOKIE");
        return;
    }
    const connectorKey = `mobile_probe_${Date.now()}`;
    const connectorTitle = `Mobile probe ${Date.now()}`;
    const adminHeaders = {
        cookie: `id=${adminCookie}`,
        "content-type": "application/json",
    };
    const created = await fetch(`${BASE}/api/v0/admin/connectors`, {
        method: "PUT",
        headers: adminHeaders,
        body: JSON.stringify({
            key: connectorKey,
            title: connectorTitle,
            description: "A connector used to verify the complete user flow.",
            base_url: "http://127.0.0.1:9/mcp",
            auth_type: "static_bearer",
        }),
    });
    assert.equal(created.status, 200, await created.text());
    const enabled = await fetch(`${BASE}/api/v0/admin/connectors/${connectorKey}/toggle`, {
        method: "POST",
        headers: adminHeaders,
        body: JSON.stringify({ enabled: true }),
    });
    assert.equal(enabled.status, 200, await enabled.text());

    const ctx = await browser.newContext({ viewport: { width: 390, height: 844 }, colorScheme: "dark" });
    await ctx.addCookies([{ name: "id", value: await devSessionCookie(), url: BASE }]);
    const page = await ctx.newPage();
    try {
        await page.goto(`${BASE}/integrations`, { waitUntil: "networkidle" });
        const card = page.getByRole("heading", { name: connectorTitle }).locator("xpath=ancestor::section");
        await card.getByLabel("Your API token", { exact: true }).fill("test-token");
        const connected = page.waitForResponse((response) => response.url().endsWith(`/api/v0/integrations/${connectorKey}/token`));
        await card.getByRole("button", { name: "Connect", exact: true }).click();
        assert.equal((await connected).status(), 200);
        await card.getByText(/Couldn't load this connector's tools/).waitFor();
        assert.equal(await card.getByRole("button", { name: "Reconnect", exact: true }).count(), 1);
        assert.equal(await card.getByRole("button", { name: "Disconnect", exact: true }).count(), 1);
        assert.ok(await page.locator("main").evaluate((element) => element.scrollWidth <= element.clientWidth));
    } finally {
        await ctx.close();
        await fetch(`${BASE}/api/v0/admin/connectors/${connectorKey}`, {
            method: "DELETE",
            headers: { cookie: `id=${adminCookie}` },
        });
    }
});

test("personal skills preserve inline authoring, editing, rendering, and deletion on mobile", async () => {
    const ctx = await browser.newContext({ viewport: { width: 390, height: 844 }, colorScheme: "dark" });
    await ctx.addCookies([{ name: "id", value: await devSessionCookie(), url: BASE }]);
    const page = await ctx.newPage();
    const slug = `mobile-skill-${Date.now()}`;
    const title = `Mobile Skill ${Date.now()}`;
    await page.goto(`${BASE}/skills`, { waitUntil: "networkidle" });
    assert.equal(await page.locator('input[type="file"][accept=".skill,.zip"]').count(), 1);
    await page.getByRole("link", { name: "New skill", exact: true }).click();
    const editor = page.locator('textarea[name="content"]');
    await editor.fill(`---\nname: ${slug}\ntitle: ${title}\ndescription: Helps verify private skills.\n---\n\n# Instructions\n\nFirst version.\n`);
    const created = page.waitForResponse((response) => response.url().endsWith('/api/v0/skills') && response.request().method() === 'POST');
    await page.getByRole("button", { name: "Save", exact: true }).click();
    assert.equal((await created).status(), 201);
    await page.getByRole("heading", { name: title, exact: true }).waitFor();
    await page.getByText("Helps verify private skills.", { exact: true }).waitFor();
    await page.getByText("First version.", { exact: true }).waitFor();

    await page.getByRole("link", { name: "Edit", exact: true }).click();
    await editor.fill(`---\nname: ${slug}\ntitle: ${title}\ndescription: Helps verify private skills.\n---\n\n# Instructions\n\nUpdated version.\n`);
    await page.getByRole("button", { name: "Save", exact: true }).click();
    await page.getByText("Updated version.", { exact: true }).waitFor();
    assert.ok(await page.locator("main").evaluate((element) => element.scrollWidth <= element.clientWidth));

    page.once("dialog", (dialog) => dialog.accept());
    const deleted = page.waitForResponse((response) => response.url().endsWith(`/api/v0/skills/${slug}`) && response.request().method() === "DELETE");
    await page.getByRole("button", { name: "Delete", exact: true }).click();
    assert.equal((await deleted).status(), 204);
    await page.waitForURL(`${BASE}/skills`);
    assert.equal(await page.getByRole("link", { name: title, exact: true }).count(), 0);
    await ctx.close();
});

test("scheduled actions keep the complete schedule workflow on mobile", async () => {
    const ctx = await browser.newContext({ viewport: { width: 390, height: 844 }, colorScheme: "dark" });
    await ctx.addCookies([{ name: "id", value: await devSessionCookie(), url: BASE }]);
    const page = await ctx.newPage();
    await page.goto(`${BASE}/scheduled`, { waitUntil: "networkidle" });

    for (const mode of ["Hourly", "Daily", "Weekly", "Monthly", "Advanced"]) {
        assert.equal(await page.getByRole("radio", { name: mode, exact: true }).count(), 1);
    }
    assert.equal(await page.getByRole("textbox", { name: "Timezone", exact: true }).count(), 1);
    assert.equal(await page.getByRole("checkbox", { name: /Allow tools/ }).isChecked(), true);
    assert.equal(await page.getByRole("checkbox", { name: /Reuse the previous run/ }).isChecked(), false);

    await page.getByRole("radio", { name: "Hourly", exact: true }).check();
    assert.equal(await page.getByRole("spinbutton", { name: "Hour", exact: true }).count(), 0);
    assert.equal(await page.getByRole("spinbutton", { name: "Minute", exact: true }).count(), 1);
    assert.ok(await page.locator("main").evaluate((element) => element.scrollWidth <= element.clientWidth));
    await ctx.close();
});

test("webhooks preserve security, reuse, reveal, and edit workflows on mobile", async () => {
    const ctx = await browser.newContext({ viewport: { width: 390, height: 844 }, colorScheme: "dark" });
    await ctx.addCookies([{ name: "id", value: await devSessionCookie(), url: BASE }]);
    const page = await ctx.newPage();
    const hookName = `Mobile deploy digest ${Date.now()}`;
    await page.goto(`${BASE}/webhooks`, { waitUntil: "networkidle" });

    await page.getByRole("textbox", { name: "Name", exact: true }).fill(hookName);
    await page.getByRole("textbox", { name: "Prompt", exact: true }).fill("Summarise this deploy payload");
    await page.getByRole("checkbox", { name: /Wait for the response/ }).check();
    await page.getByRole("checkbox", { name: /Allow tools/ }).check();
    await page.getByText(/Anyone with the trigger URL can send content/).waitFor();
    await page.getByRole("checkbox", { name: /Reuse the conversation/ }).check();
    await page.getByRole("spinbutton", { name: "Rounds of history to replay" }).fill("3");
    const createResponse = page.waitForResponse((response) => response.url().endsWith("/api/v0/webhooks") && response.request().method() === "POST");
    await page.getByRole("button", { name: "Create webhook", exact: true }).click();
    const created = await createResponse;
    assert.equal(created.status(), 201, await created.text());

    const revealedUrl = page.getByRole("textbox", { name: "Your trigger URL" });
    await revealedUrl.waitFor();
    // Derived from BASE, not hardcoded to :8080 — the suite has to run
    // against a gateway on any port (a second dev-ui alongside the first).
    const { port } = new URL(BASE);
    assert.match(await revealedUrl.inputValue(), new RegExp(`^https?://(?:localhost|127\\.0\\.0\\.1):${port}/hooks/gwh_`));
    const row = page.getByText(hookName, { exact: true }).locator("xpath=ancestor::li");
    await row.getByText(/Waits for response/).waitFor();
    await row.getByRole("link", { name: "Edit", exact: true }).click();
    await page.getByRole("heading", { name: "Edit webhook", exact: true }).waitFor();
    assert.equal(await page.title(), "Edit webhook — LLM Gateway");
    assert.equal(await page.getByRole("checkbox", { name: /Wait for the response/ }).isChecked(), true);
    assert.equal(await page.getByRole("checkbox", { name: /Allow tools/ }).isChecked(), true);
    assert.equal(await page.getByRole("checkbox", { name: /Reuse the conversation/ }).isChecked(), true);
    assert.ok(await page.locator("main").evaluate((element) => element.scrollWidth <= element.clientWidth));

    await page.getByRole("link", { name: /Back/ }).click();
    page.once("dialog", (dialog) => dialog.accept());
    await page.getByText(hookName, { exact: true }).locator("xpath=ancestor::li").getByRole("button", { name: "Delete", exact: true }).click();
    await page.getByText(hookName, { exact: true }).waitFor({ state: "detached" });
    await ctx.close();
});
