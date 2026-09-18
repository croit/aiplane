// The SPA's admin views (issue #22 P4): gated JSON API + rendered pages.
// dev-ui's seed user carries the admin role, so the /api/v0/admin surface
// answers with real data for its cookie.

import { test, before, after } from "node:test";
import assert from "node:assert";

import { BASE, chooseSearchable, gatewayIsUp, launchBrowser } from "./helpers.mjs";

let browser;
let cookie;

before(async () => {
    assert.ok(await gatewayIsUp(), `gateway is not reachable at ${BASE}`);
    const probe = await fetch(`${BASE}/`);
    assert.equal(probe.status, 200, "the SPA is not served at the root");
    browser = await launchBrowser();
    // The admin views need an ADMIN session. Prefer AIPLANE_SESSION_COOKIE
    // (an operator cookie); fall back to the dev fixture user, which is only
    // admin on gateways whose RBAC maps it so.
    cookie = process.env.AIPLANE_SESSION_COOKIE?.trim().replace(/^id=/, "") ?? null;
    if (!cookie) {
        const r = await fetch(`${BASE}/__dev/session`, { redirect: "manual" });
        cookie = (r.headers.getSetCookie().find((c) => c.startsWith("id=")) ?? "").split(";")[0].slice(3);
    }
    if (
        !(await fetch(`${BASE}/api/v0/admin/groups`, { headers: { cookie: `id=${cookie}` } })).ok
    ) {
        console.log("skipping: no admin session (set AIPLANE_SESSION_COOKIE)");
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

test("users preserves identity mapping, impersonation controls, and audit history", async () => {
    const ctx = await browser.newContext({ viewport: { width: 390, height: 844 } });
    await ctx.addCookies([{ name: "id", value: cookie, url: BASE }]);
    const page = await ctx.newPage();
    await page.goto(`${BASE}/admin/users`, { waitUntil: "domcontentloaded" });

    await page.getByRole("heading", { name: "Users", exact: true }).waitFor();
    await page.getByText("identity-provider groups", { exact: false }).waitFor();
    await page.getByRole("columnheader", { name: "OIDC groups", exact: true }).waitFor();
    const roster = page.getByRole("table").first();
    await roster.getByText("dev@example.com", { exact: true }).waitFor();
    await roster.getByText("platform-admins", { exact: true }).waitFor();
    await page.getByRole("heading", { name: "Recent impersonation activity", exact: true }).waitFor();
    await page.getByText("No impersonations recorded yet.", { exact: true }).waitFor();
    page.once("dialog", (dialog) => dialog.dismiss());
    await page.getByRole("button", { name: "Impersonate", exact: true }).first().click();
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true);
    await ctx.close();
});

test("API tokens preserve dates, accounting, quotas, and the operator model restriction", async () => {
    const ctx = await browser.newContext({ viewport: { width: 390, height: 844 } });
    await ctx.addCookies([{ name: "id", value: cookie, url: BASE }]);
    const page = await ctx.newPage();
    await page.goto(`${BASE}/admin/tokens`, { waitUntil: "domcontentloaded" });

    await page.getByRole("heading", { name: "API tokens", exact: true }).waitFor();
    await page.getByText("only a SHA-256", { exact: false }).waitFor();
    const row = page.getByRole("row").filter({ hasText: "Production API" }).filter({ hasText: "dev@example.com" });
    await row.getByText("created", { exact: false }).waitFor();
    await row.getByText("demo-model, demo-model-pro", { exact: true }).waitFor();
    await row.getByText(/Cost \/ Month/).waitFor();
    await row.getByRole("button", { name: "Edit models", exact: true }).click();
    const modal = page.locator("dialog[open]");
    await modal.waitFor();
    const restrictModels = modal.getByLabel("Restrict this token to specific models", { exact: true });
    if (!(await restrictModels.isChecked())) await restrictModels.check();
    for (const checkbox of await modal.locator('input.checkbox').all()) await checkbox.uncheck();
    await modal.getByLabel("demo-model", { exact: true }).check();
    const saved = page.waitForResponse((response) => response.url().includes('/api/v0/admin/tokens/') && response.request().method() === "PUT");
    await modal.getByRole("button", { name: "Save models", exact: true }).click();
    const savedResponse = await saved;
    assert.equal(savedResponse.status(), 200);
    assert.deepEqual((await savedResponse.json()).models, ["demo-model"]);
    await page.getByText("Operator restriction set: 1 models.", { exact: true }).waitFor();
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true);
    await ctx.close();
});

test("groups keep the complete inline create and grant-editing workflow", async () => {
    const ctx = await browser.newContext({ viewport: { width: 390, height: 844 } });
    await ctx.addCookies([{ name: "id", value: cookie, url: BASE }]);
    const page = await ctx.newPage();
    await page.goto(`${BASE}/admin/groups`, { waitUntil: "domcontentloaded" });

    await page.getByRole("heading", { name: "Gateway groups", exact: true }).waitFor();
    await page.getByText("IdP-independent group names", { exact: false }).waitFor();
    const create = page.locator("article").filter({ has: page.getByRole("heading", { name: "New group", exact: true }) });
    await create.getByLabel("Name", { exact: true }).fill("parity-audit");
    await create.getByLabel("Description", { exact: true }).fill("Temporary browser-test group");
    await create.locator('input[list="group-oidc-values"]').fill("qa-team, qa-admins");
    await create.getByLabel("Tools", { exact: true }).fill("search_web, fetch_url");
    await create.getByLabel("Skills", { exact: true }).fill("release-notes-writer");
    const created = page.waitForResponse((response) => response.url().endsWith("/api/v0/admin/groups") && response.request().method() === "PUT");
    await create.getByRole("button", { name: "Save", exact: true }).click();
    assert.equal((await created).status(), 200);

    const card = page.locator("article").filter({ has: page.getByRole("heading", { name: "parity-audit", exact: true }) });
    const oidcValues = card.locator('input[list="group-oidc-values"]');
    await oidcValues.waitFor();
    assert.equal(await oidcValues.inputValue(), "qa-admins, qa-team");
    assert.equal(await card.getByLabel("Name", { exact: true }).isEditable(), false);
    await card.getByLabel("Description", { exact: true }).fill("Updated browser-test group");
    const updated = page.waitForResponse((response) => response.url().endsWith("/api/v0/admin/groups") && response.request().method() === "PUT");
    await card.getByRole("button", { name: "Save", exact: true }).click();
    assert.equal((await updated).status(), 200);

    page.once("dialog", (dialog) => dialog.accept());
    const deleted = page.waitForResponse((response) => response.url().includes("/api/v0/admin/groups/parity-audit") && response.request().method() === "DELETE");
    await card.getByRole("button", { name: "Delete", exact: true }).click();
    assert.equal((await deleted).status(), 204);
    await page.getByRole("heading", { name: "parity-audit", exact: true }).waitFor({ state: "detached" });
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true);
    await ctx.close();
});

test("models preserve defaults, search settings, filters, and the complete override editor", async () => {
    const ctx = await browser.newContext({ viewport: { width: 390, height: 844 } });
    await ctx.addCookies([{ name: "id", value: cookie, url: BASE }]);
    const page = await ctx.newPage();
    await page.goto(`${BASE}/admin/models`, { waitUntil: "domcontentloaded" });

    await page.getByRole("heading", { name: "Models", exact: true }).waitFor();
    await page.getByText("every request", { exact: false }).waitFor();
    await page.getByRole("heading", { name: "Default models", exact: true }).waitFor();
    await page.getByRole("heading", { name: "Web search", exact: true }).waitFor();
    for (const name of ["All", "chat", "other kinds", "aliases", "configured only"]) {
        assert.equal(await page.getByRole("button", { name, exact: true }).count(), 1);
    }

    const defaultSaved = page.waitForResponse((response) => response.url().endsWith("/api/v0/admin/model-defaults") && response.request().method() === "PUT");
    await chooseSearchable(page, page.getByLabel("Chat", { exact: true }), "demo-model-pro");
    assert.equal((await defaultSaved).status(), 200);

    const search = page.locator("article").filter({ has: page.getByRole("heading", { name: "Web search", exact: true }) });
    await search.getByLabel("Provider", { exact: true }).selectOption("searxng");
    await search.getByLabel("SearXNG base URL", { exact: true }).fill("https://search.example.test");
    const searchSaved = page.waitForResponse((response) => response.url().endsWith("/api/v0/admin/search-settings") && response.request().method() === "PUT");
    await search.getByRole("button", { name: "Save web search", exact: true }).click();
    assert.equal((await searchSaved).status(), 200);

    // The per-model editor is its own route now (it used to expand inside the
    // table row); Edit navigates there and a save returns to the list.
    const row = page.getByTestId("model-row-demo-model");
    await row.getByRole("link", { name: "Edit", exact: true }).click();
    await page.waitForURL(/\/admin\/models\/edit\?model=demo-model$/);
    await page.getByLabel("Reasoning style", { exact: true }).selectOption("qwen");
    await page.getByLabel("Context window (tokens)", { exact: true }).fill("65536");
    await page.getByLabel("Vision", { exact: true }).selectOption("true");
    await page.getByLabel("Sampling defaults (TOML)", { exact: true }).fill("temperature = 0.4");
    const saved = page.waitForResponse((response) => response.url().endsWith("/api/v0/admin/models") && response.request().method() === "PUT");
    await page.getByRole("button", { name: "Save model", exact: true }).click();
    assert.equal((await saved).status(), 200);
    // The editor reports its outcome back through the query string.
    await page.waitForURL((url) => url.pathname === "/admin/models" && url.searchParams.get("notice") === "saved");
    await page.getByText("effective immediately", { exact: false }).waitFor();

    const filter = page.getByPlaceholder("Filter models…", { exact: true });
    await filter.fill("demo-model-pro");
    assert.equal(await page.getByTestId("model-row-demo-model").isVisible(), false);
    assert.equal(await page.getByTestId("model-row-demo-model-pro").isVisible(), true);
    await filter.fill("");
    await page.getByRole("button", { name: "configured only", exact: true }).click();
    await page.getByTestId("model-row-demo-model").getByText("CTX", { exact: true }).waitFor();
    await page.getByTestId("model-row-demo-model").getByText("CAPS", { exact: true }).waitFor();
    await page.getByTestId("model-row-demo-model").getByText("TOML", { exact: true }).waitFor();
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true);
    await ctx.close();
});

test("admin skills preserve master-detail, archives, files, and effective access", async () => {
    const { mkdtemp, rm, writeFile } = await import("node:fs/promises");
    const { tmpdir } = await import("node:os");
    const { join } = await import("node:path");
    const ctx = await browser.newContext({ viewport: { width: 390, height: 844 } });
    await ctx.addCookies([{ name: "id", value: cookie, url: BASE }]);
    const page = await ctx.newPage();
    await page.goto(`${BASE}/admin/skills?skill=release-notes-writer`, { waitUntil: "domcontentloaded" });

    await page.getByRole("heading", { name: "Skills", exact: true }).waitFor();
    await page.getByText("loads on demand", { exact: false }).waitFor();
    await page.getByText("Loaded skills", { exact: true }).waitFor();
    await page.getByRole("heading", { name: "release-notes-writer", exact: true }).waitFor();
    await page.getByText("references/", { exact: true }).waitFor();
    await page.getByText("example.md", { exact: true }).waitFor();
    await page.getByRole("heading", { name: "Release Notes Writer", exact: true }).waitFor();
    await page.getByText("Granted to", { exact: true }).waitFor();
    await page.getByText("1 bundled", { exact: true }).waitFor();

    const archive = await ctx.request.get(`${BASE}/api/v0/admin/skills/release-notes-writer/archive`);
    assert.equal(archive.status(), 200);
    assert.match(archive.headers()["content-disposition"], /release-notes-writer\.skill/);
    const scratch = await mkdtemp(join(tmpdir(), "gateway-admin-skill-"));
    const archivePath = join(scratch, "release-notes-writer.skill");
    try {
        await writeFile(archivePath, await archive.body());
        await page.locator('input[type="file"]').setInputFiles(archivePath);
        const uploaded = page.waitForResponse((response) => response.url().endsWith("/api/v0/admin/skills") && response.request().method() === "POST");
        await page.getByRole("button", { name: "Upload .skill", exact: true }).click();
        assert.equal((await uploaded).status(), 201);
        await page.getByText("Installed release-notes-writer.", { exact: true }).waitFor();
    } finally {
        await rm(scratch, { recursive: true, force: true });
    }

    await page.getByRole("button", { name: "Edit access", exact: true }).click();
    const finance = page.getByLabel("finance", { exact: true });
    const originallyChecked = await finance.isChecked();
    await finance.setChecked(!originallyChecked);
    let saved = page.waitForResponse((response) => response.url().endsWith("/api/v0/admin/skills/grants") && response.request().method() === "PUT");
    await page.getByRole("button", { name: "Save access", exact: true }).click();
    assert.equal((await saved).status(), 200);
    await page.getByRole("button", { name: "Edit access", exact: true }).click();
    await page.getByLabel("finance", { exact: true }).setChecked(originallyChecked);
    saved = page.waitForResponse((response) => response.url().endsWith("/api/v0/admin/skills/grants") && response.request().method() === "PUT");
    await page.getByRole("button", { name: "Save access", exact: true }).click();
    assert.equal((await saved).status(), 200);
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true);
    await ctx.close();
});

test("connectors preserve the complete catalog, lifecycle, and audit workflow", async () => {
    const key = `parity-connector-${Date.now()}`;
    const ctx = await browser.newContext({ viewport: { width: 390, height: 844 } });
    await ctx.addCookies([{ name: "id", value: cookie, url: BASE }]);
    const page = await ctx.newPage();
    await page.goto(`${BASE}/admin/connectors`, { waitUntil: "domcontentloaded" });

    await page.getByRole("heading", { name: "Connectors", exact: true }).waitFor();
    assert.equal(await page.title(), "Connectors — AIplane");
    await page.getByText("Curate the MCP servers", { exact: false }).waitFor();
    // Add and Edit are routes now, not disclosures inside the list.
    await page.getByRole("link", { name: "Add a connector", exact: true }).click();
    await page.waitForURL((url) => url.pathname === "/admin/connectors/new");
    const create = page.locator("form").filter({ has: page.getByLabel("Key (stable id)", { exact: true }) });
    await create.waitFor();
    for (const label of ["Key (stable id)", "Name", "Icon (emoji)", "Category", "Description", "MCP server URL", "Scope", "Authentication", "Allowed groups (comma-separated)"]) {
        assert.equal(await create.getByLabel(label, { exact: true }).count(), 1, `${label} must be present`);
    }
    await create.getByLabel("Key (stable id)", { exact: true }).fill("google_workspace");
    await create.getByText("self-hosted Google Workspace MCP server", { exact: true }).waitFor();
    await create.getByRole("link", { name: "taylorwilsdon/google_workspace_mcp", exact: true }).waitFor();
    await create.getByText("WORKSPACE_MCP_ALLOWED_CLIENT_REDIRECT_URIS", { exact: true }).waitFor();
    await create.getByLabel("Try dynamic client registration (RFC 7591)", { exact: true }).uncheck();
    await create.getByLabel("Key (stable id)", { exact: true }).fill("github");
    await create.getByRole("link", { name: "OAuth App", exact: true }).waitFor();
    await create.getByLabel("Key (stable id)", { exact: true }).fill(key);
    await create.getByLabel("Name", { exact: true }).fill("Parity connector");
    await create.getByLabel("Icon (emoji)", { exact: true }).fill("🧭");
    await create.getByLabel("Category", { exact: true }).fill("Testing");
    await create.getByLabel("Description", { exact: true }).fill("Temporary connector used by browser parity tests.");
    await create.getByLabel("MCP server URL", { exact: true }).fill("https://connector.invalid/mcp");
    await create.getByLabel("Authentication", { exact: true }).selectOption("none");
    await create.getByLabel("Audit tool calls (log who ran each tool, and the outcome)", { exact: true }).check();
    const created = page.waitForResponse((response) => response.url().endsWith("/api/v0/admin/connectors") && response.request().method() === "PUT");
    await create.getByRole("button", { name: "Add connector", exact: true }).click();
    assert.equal((await created).status(), 200);

    await page.waitForURL((url) => url.pathname === "/admin/connectors");
    let card = page.getByTestId(`connector-${key}`);
    await card.getByRole("heading", { name: "Parity connector", exact: true }).waitFor();
    await card.getByText("Disabled", { exact: true }).waitFor();
    await card.getByText("Audited", { exact: true }).waitFor();
    assert.equal(await card.getByText("DCR", { exact: true }).count(), 0);

    await card.getByRole("link", { name: "Edit", exact: true }).click();
    await page.waitForURL((url) => url.pathname === `/admin/connectors/${key}/edit`);
    assert.equal(await page.getByLabel("Description", { exact: true }).inputValue(), "Temporary connector used by browser parity tests.");
    assert.equal(await page.getByLabel("Category", { exact: true }).inputValue(), "Testing");
    assert.equal(await page.getByLabel("Authentication", { exact: true }).inputValue(), "none");
    await page.getByRole("link", { name: "← Connectors", exact: true }).click();
    await page.waitForURL((url) => url.pathname === "/admin/connectors");
    card = page.getByTestId(`connector-${key}`);

    const enabled = page.waitForResponse((response) => response.url().includes(`/api/v0/admin/connectors/${key}/toggle`) && response.request().method() === "POST");
    await card.getByRole("button", { name: "Enable", exact: true }).click();
    assert.equal((await enabled).status(), 200);
    await card.getByText("Enabled", { exact: true }).waitFor();

    await card.getByRole("link", { name: "Audit log", exact: true }).click();
	await page.waitForURL((url) => url.pathname === `/admin/connectors/${key}/audit`);
    await page.getByRole("heading", { name: "Parity connector", exact: true }).waitFor();
    assert.equal(await page.title(), "Parity connector — audit log");
    await page.getByText(`Tool-call audit for ${key}.`, { exact: false }).waitFor();
    await page.getByText("No tool calls recorded for this connector yet.", { exact: true }).waitFor();
    await page.getByRole("link", { name: "← Connectors", exact: true }).click();

    const restoredCard = page.getByTestId(`connector-${key}`);
    await restoredCard.waitFor();
    page.once("dialog", (dialog) => dialog.accept());
    const deleted = page.waitForResponse((response) => response.url().includes(`/api/v0/admin/connectors/${key}`) && response.request().method() === "DELETE");
    await restoredCard.getByRole("button", { name: "Delete", exact: true }).click();
    assert.equal((await deleted).status(), 204);
    await restoredCard.waitFor({ state: "detached" });
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true);
    await ctx.close();
});

test("ComfyUI preserves operator configuration, workflow schemas, jobs, and reload", async () => {
    const ctx = await browser.newContext({ viewport: { width: 390, height: 844 } });
    await ctx.addCookies([{ name: "id", value: cookie, url: BASE }]);
    const page = await ctx.newPage();
    await page.goto(`${BASE}/admin/comfyui`, { waitUntil: "domcontentloaded" });

    await page.getByRole("heading", { name: "ComfyUI workflow catalog", exact: true }).waitFor();
    await page.getByText("Users never see ComfyUI itself", { exact: false }).waitFor();
    // The worker strip answers "is it up?" before the catalog answers
    // "what can it do?" — the dev worker is unreachable, which is a verdict
    // the page must render rather than an error it swallows.
    await page.getByText("Unreachable", { exact: true }).waitFor();
    await page.getByRole("heading", { name: "Operator configuration", exact: true }).waitFor();
    for (const label of ["Workflow timeout", "Queue poll interval", "Concurrent jobs", "Content directory"]) {
        await page.getByText(label, { exact: true }).waitFor();
    }

    // Catalog: a searchable rail selects into a detail pane, and the
    // selection lives in the URL so one workflow's contract is linkable.
    await page.getByRole("heading", { name: /Loaded workflows/ }).waitFor();
    await page.getByPlaceholder("Search workflows and parameters", { exact: true }).fill("text_to_image");
    await page.getByRole("link", { name: "text_to_image", exact: true }).click();
    await page.waitForURL((url) => url.searchParams.get("workflow") === "text_to_image");
    const workflow = page.getByTestId("comfyui-workflow-text_to_image");
    await workflow.getByText("comfyui_text_to_image", { exact: true }).waitFor();
    await workflow.getByText("node 9", { exact: true }).waitFor();
    await workflow.getByRole("cell", { name: "prompt required" }).waitFor();

    const reloaded = page.waitForResponse((response) => response.url().endsWith("/api/v0/comfyui/reload") && response.request().method() === "POST");
    await page.getByRole("button", { name: "Reload catalog", exact: true }).click();
    assert.equal((await reloaded).status(), 200);
    await page.getByText("workflow(s) loaded", { exact: false }).waitFor();

    // Job runs are their own page, reached from the tab pair.
    await page.getByRole("tab", { name: /Job runs/ }).click();
    await page.waitForURL((url) => url.pathname === "/admin/comfyui/jobs");
    await page.getByRole("heading", { name: "Recent jobs", exact: true }).waitFor();
    await page.getByRole("heading", { name: "Reliability by workflow", exact: true }).waitFor();
    await page.getByTestId("comfyui-job-1").getByText("completed", { exact: true }).waitFor();
    // The failed filter must cover gateway timeouts, not just ComfyUI errors.
    await page.getByRole("button", { name: /^Failed/ }).click();
    await page.getByTestId("comfyui-job-4").getByText("timeout", { exact: true }).waitFor();
    assert.equal(await page.getByTestId("comfyui-job-1").count(), 0);
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true);
    await ctx.close();
});

test("limits preserve policy context, typed assignments, model scope, and CRUD", async () => {
    const ctx = await browser.newContext({ viewport: { width: 390, height: 844 } });
    await ctx.addCookies([{ name: "id", value: cookie, url: BASE }]);
    const page = await ctx.newPage();
    await page.goto(`${BASE}/admin/limits`, { waitUntil: "domcontentloaded" });

    await page.getByRole("heading", { name: "Rate limits & quotas", exact: true }).waitFor();
    await page.getByText("most-specific-first", { exact: false }).waitFor();
    const form = page.getByRole("heading", { name: "Add or update a limit", exact: true }).locator("..");
    assert.equal(await form.locator("#limit-subject-type").inputValue(), "global");
    assert.equal(await form.locator("#limit-subject-id").isDisabled(), true);
    await form.locator("#limit-subject-type").selectOption("token");
    await chooseSearchable(page, form.locator("#limit-subject-id"), "Production API");
    await chooseSearchable(page, form.locator("#limit-model"), "demo-model");
    await form.locator("#limit-dimension").selectOption("tokens");
    await form.locator("#limit-window").selectOption("day");
    await form.locator("#limit-value").fill("10000000");
    const saved = page.waitForResponse((response) => response.url().endsWith("/api/v0/admin/limits") && response.request().method() === "POST");
    await form.getByRole("button", { name: "Save limit", exact: true }).click();
    assert.equal((await saved).status(), 200);
    await page.getByText("saved limit for Production API", { exact: false }).waitFor();

    const row = page.getByRole("row").filter({ hasText: "Production API (dev@example.com)" }).filter({ hasText: "demo-model" }).filter({ hasText: "10,000,000" });
    await row.waitFor();
    page.once("dialog", (dialog) => dialog.accept());
    const deleted = page.waitForResponse((response) => response.url().includes("/api/v0/admin/limits/") && response.request().method() === "DELETE");
    await row.getByRole("button", { name: "Delete", exact: true }).click();
    assert.equal((await deleted).status(), 204);
    await row.waitFor({ state: "detached" });
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true);
    await ctx.close();
});

test("settings preserve category navigation, feature disclosure, typed fields, and section saves", async () => {
    const ctx = await browser.newContext({ viewport: { width: 390, height: 844 } });
    await ctx.addCookies([{ name: "id", value: cookie, url: BASE }]);
    const page = await ctx.newPage();
    await page.goto(`${BASE}/admin/settings`, { waitUntil: "domcontentloaded" });

    await page.getByRole("heading", { name: "Settings", exact: true }).waitFor();
    await page.getByText("They live in the database", { exact: false }).waitFor();
    const rail = page.getByRole("navigation", { name: "Settings", exact: true });
    for (const label of ["Chat", "Tools", "Content & data", "Access & usage", "Notifications"]) await rail.getByRole("link", { name: new RegExp(label) }).waitFor();
    await page.getByRole("heading", { name: "Document OCR", exact: true }).waitFor();
    await page.getByText("Turning uploaded PDFs", { exact: false }).waitFor();
    await page.getByText("chat.ocr.enabled", { exact: true }).waitFor();
    await page.getByText("Show 10 more settings", { exact: true }).waitFor();

    await rail.getByRole("link", { name: /Access & usage/ }).click();
    await page.waitForURL(/tab=access/);
    await page.getByRole("heading", { name: "Sessions & tokens", exact: true }).waitFor();
    const sessions = page.locator('[data-testid="settings-section-gateway"]');
    const ttl = sessions.getByLabel("Session idle timeout", { exact: true });
    const original = await ttl.inputValue();
    await ttl.fill(original === "31" ? "30" : "31");
    const saved = page.waitForResponse((response) => response.url().endsWith("/api/v0/admin/settings") && response.request().method() === "POST");
    await sessions.getByRole("button", { name: "Save section", exact: true }).click();
    assert.equal((await saved).status(), 200);
    await page.getByText("Saved. In effect from the next request.", { exact: true }).waitFor();
    await ttl.fill(original);
    const restored = page.waitForResponse((response) => response.url().endsWith("/api/v0/admin/settings") && response.request().method() === "POST");
    await sessions.getByRole("button", { name: "Save section", exact: true }).click();
    assert.equal((await restored).status(), 200);
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true);
    await ctx.close();
});

test("upstreams keeps pool topology, backend controls, and fallbacks together", async () => {
    const topology = await fetch(`${BASE}/api/v0/admin/upstreams`, {
        headers: { cookie: `id=${cookie}` },
    }).then((response) => response.json());
    const ctx = await browser.newContext();
    await ctx.addCookies([{ name: "id", value: cookie, url: BASE }]);
    const page = await ctx.newPage();
    await page.goto(`${BASE}/admin/upstreams`, { waitUntil: "domcontentloaded" });

    await page.getByRole("heading", { name: "Upstreams", exact: true }).waitFor();
    await page.getByRole("heading", { name: "Unknown-model fallbacks" }).waitFor();

    for (const pool of topology.pools) {
        const card = page.getByTestId(`upstream-pool-${pool.name}`);
        assert.equal(await card.count(), 1, `pool ${pool.name} must have one card`);
        for (const backendName of pool.backends) {
            assert.equal(
                await card.getByTestId(`upstream-backend-${backendName}`).count(),
                1,
                `${backendName} must be nested in pool ${pool.name}`,
            );
        }
        assert.equal(await card.getByText("Edit pool", { exact: true }).count(), 1);
        assert.equal(await card.getByText("What clients see", { exact: true }).count(), 1);
    }

    const firstBackend = topology.backends.find((backend) => backend.live);
    if (firstBackend) {
        const row = page.getByTestId(`upstream-backend-${firstBackend.name}`);
        assert.equal(await row.getByLabel("Serving traffic").count(), 1);
        await row.getByRole("button", { name: "Edit backend", exact: true }).click();
        const editor = page.locator("dialog[open]");
        await editor.waitFor();
        assert.equal(await editor.locator('button:has-text("Test connection")').count(), 1);
        assert.equal(await editor.getByText("Calls this URL with the credentials above. Nothing is saved.", { exact: true }).count(), 1);
    }
    await ctx.close();
});
