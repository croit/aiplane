import { test } from "node:test";
import assert from "node:assert/strict";
import { devSessionCookie, gatewayIsUp, launchBrowser, BASE } from "./helpers.mjs";

test("Dot action overrides preserve the tool default, reload and require correction of unavailable actions", async () => {
    assert.ok(await gatewayIsUp(), `gateway is not reachable at ${BASE}`);
    const browser = await launchBrowser();
    let context;
    let dotId;
    try {
        context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
        await context.addCookies([
            { name: "id", value: await devSessionCookie(), url: BASE },
            { name: "lang", value: "en", url: BASE },
        ]);
        const page = await context.newPage();
        const name = `Action Dot ${Date.now()}`;
        const created = await context.request.post(`${BASE}/api/v0/dots`, { data: { name } });
        assert.equal(created.status(), 201);
        dotId = (await created.json()).dot.id;
        const accessUrl = `${BASE}/api/v0/dots/${dotId}/access`;
        const catalogueResponse = await context.request.get(accessUrl);
        assert.equal(catalogueResponse.status(), 200);
        const catalogue = await catalogueResponse.json();
        const actualBrowser = catalogue.tools.find((tool) => tool.id === "browse_page");
        assert.ok(actualBrowser, "the seeded gateway must expose its actual browse_page tool");
        assert.ok(actualBrowser.actions.some((action) => action.id === "read"));
        assert.ok(actualBrowser.actions.some((action) => action.id === "fill"));

        await page.goto(`${BASE}/dots`, { waitUntil: "networkidle" });
        const card = page.locator("article").filter({ has: page.getByRole("heading", { name, exact: true }) });
        const access = card.locator("details").filter({ has: page.getByText("Access and approvals", { exact: true }) });
        await access.locator("summary").first().click();
        await access.getByRole("textbox").fill("browse_page");
        const browserRow = access.locator("li").filter({ has: page.locator("code").filter({ hasText: /^browse_page$/ }) });
        await browserRow.getByRole("checkbox").check();
        await browserRow.getByRole("combobox", { name: "Approval rule", exact: true }).selectOption("always_ask");
        const actions = browserRow.locator("details").filter({ has: page.getByText("Actions", { exact: true }) });
        await actions.locator("summary").click();
        const read = actions.getByRole("combobox", { name: "Rule for read", exact: true });
        const fill = actions.getByRole("combobox", { name: "Rule for fill", exact: true });
        assert.equal(await read.inputValue(), "");
        await read.selectOption("allow");
        await fill.selectOption("deny");
        const saved = page.waitForResponse((response) => response.request().method() === "PUT" && response.url() === accessUrl);
        await access.getByRole("button", { name: "Save", exact: true }).click();
        const savedResponse = await saved;
        assert.equal(savedResponse.status(), 200);
        const body = savedResponse.request().postDataJSON();
        const sentBrowser = body.tools.find((tool) => tool.id === "browse_page");
        assert.equal(sentBrowser.permission, "always_ask");
        assert.deepEqual(sentBrowser.actions, [{ id: "read", rule: "allow" }, { id: "fill", rule: "deny" }]);
        await access.getByRole("status").getByText("Access and approval rules saved.", { exact: true }).waitFor();

        await access.getByRole("button", { name: "Reload access", exact: true }).click();
        await page.waitForFunction(() => {
            const label = Array.from(document.querySelectorAll("label")).find((element) => element.querySelector("span")?.textContent.trim() === "Rule for read");
            return label?.querySelector("select")?.value === "allow";
        });
        assert.equal(await fill.inputValue(), "deny");
        assert.equal(await browserRow.getByRole("combobox", { name: "Approval rule", exact: true }).inputValue(), "always_ask");
        await read.selectOption("");
        const inheritanceSaved = page.waitForResponse((response) => response.request().method() === "PUT" && response.url() === accessUrl);
        await access.getByRole("button", { name: "Save", exact: true }).click();
        assert.deepEqual((await inheritanceSaved).request().postDataJSON().tools.find((tool) => tool.id === "browse_page").actions, [{ id: "fill", rule: "deny" }]);
        await access.getByRole("status").getByText("Access and approval rules saved.", { exact: true }).waitFor();

        await page.route(accessUrl, async (route) => {
            if (route.request().method() !== "GET") return route.continue();
            const response = await route.fetch();
            const data = await response.json();
            const row = data.tools.find((tool) => tool.id === "browse_page");
            row.actions = row.actions.filter((action) => action.id !== "fill");
            await route.fulfill({ response, json: data });
        });
        await access.getByRole("button", { name: "Reload access", exact: true }).click();
        await actions.getByText("This saved action is no longer available. Remove its rule before saving.", { exact: true }).waitFor();
        assert.equal(await access.getByRole("button", { name: "Save", exact: true }).isDisabled(), true);
        await actions.getByRole("button", { name: "Remove rule for fill", exact: true }).click();
        await page.unroute(accessUrl);
        const corrected = page.waitForResponse((response) => response.request().method() === "PUT" && response.url() === accessUrl);
        await access.getByRole("button", { name: "Save", exact: true }).click();
        assert.deepEqual((await corrected).request().postDataJSON().tools.find((tool) => tool.id === "browse_page").actions, []);
        await access.getByRole("status").getByText("Access and approval rules saved.", { exact: true }).waitFor();
        await read.focus();
        assert.equal(await read.evaluate((element) => document.activeElement === element), true);
        await page.setViewportSize({ width: 390, height: 844 });
        assert.ok(await page.locator("main").evaluate((element) => element.scrollWidth <= element.clientWidth), "action controls must fit a mobile viewport");
    } finally {
        try {
            if (dotId && context) await context.request.put(`${BASE}/api/v0/dots/${dotId}/active`, { data: { active: false }, timeout: 5000 });
        } finally {
            try { await context?.close(); }
            finally { await browser.close(); }
        }
    }
});

test("Approve once rechecks a pending Dot browser action after its rule changes to Deny", async () => {
    assert.ok(await gatewayIsUp(), `gateway is not reachable at ${BASE}`);
    const browser = await launchBrowser();
    let context;
    let dotId;
    let webhookId;
    let sessionId;
    try {
        context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
        await context.addCookies([
            { name: "id", value: await devSessionCookie(), url: BASE },
            { name: "lang", value: "en", url: BASE },
        ]);
        const page = await context.newPage();
        async function jsonGet(path) {
            const response = await context.request.get(`${BASE}${path}`);
            assert.equal(response.status(), 200, path);
            return response.json();
        }
        const suffix = Date.now();
        const created = await context.request.post(`${BASE}/api/v0/dots`, {
            data: { name: `Revoked action Dot ${suffix}` },
        });
        assert.equal(created.status(), 201);
        dotId = (await created.json()).dot.id;
        const accessUrl = `${BASE}/api/v0/dots/${dotId}/access`;
        const catalogue = await jsonGet(`/api/v0/dots/${dotId}/access`);
        assert.ok(catalogue.tools.find((tool) => tool.id === "browse_page")?.actions
            .some((action) => action.id === "navigate"), "the real browser catalogue must declare navigate");
        async function saveNavigateRule(rule) {
            const response = await context.request.put(accessUrl, {
                data: { tools: [{ id: "browse_page", permission: "always_ask", actions: [{ id: "navigate", rule }] }] },
            });
            assert.equal(response.status(), 200);
        }
        await saveNavigateRule("ask");
        const hookCreated = await context.request.post(`${BASE}/api/v0/webhooks`, {
            data: {
                name: `Revoked action webhook ${suffix}`,
                prompt: "Dot action approval probe",
                model: "demo-model",
                dot_id: dotId,
                tools_enabled: true,
                synchronous: true,
                reuse_conversation: false,
                reuse_rounds: 5,
            },
        });
        assert.equal(hookCreated.status(), 201);
        const hook = await hookCreated.json();
        webhookId = hook.webhook.id;
        const fired = await context.request.post(`${BASE}/hooks/${hook.secret}`, {
            data: { event: "action-approval-probe" },
        });
        const result = await fired.json();
        sessionId = result.session_id;
        assert.equal(fired.status(), 202);
        assert.equal(result.status, "waiting");
        assert.ok(sessionId);
        const history = await jsonGet(`/api/v0/webhooks/${webhookId}/runs`);
        assert.equal(history.runs.length, 1);
        const originalRun = history.runs[0];
        assert.equal(originalRun.status, "waiting");
        assert.equal(originalRun.session_id, sessionId);
        assert.ok(originalRun.assistant_turn_id);
        const inbox = await jsonGet("/api/v0/agents/inbox");
        const request = inbox.items.find((item) => item.turn_id === originalRun.assistant_turn_id);
        assert.ok(request, "the original pending browser call must appear in Inbox");
        assert.equal(request.session_id, sessionId);
        assert.equal(request.kind, "approval");
        assert.equal(request.call.name, "browse_page");
        const originalArguments = { action: "navigate", url: "https://example.com" };
        assert.deepEqual(JSON.parse(request.call.arguments), originalArguments);

        await saveNavigateRule("deny");
        await page.goto(`${BASE}/inbox?item=${encodeURIComponent(request.id)}`, { waitUntil: "networkidle" });
        const card = page.locator(`[id="inbox-item-${request.id}"]`);
        await card.getByRole("button", { name: "Approve once", exact: true }).waitFor();
        assert.deepEqual(JSON.parse(await card.locator("pre").innerText()), originalArguments);
        const answered = page.waitForResponse((response) => response.request().method() === "POST"
            && response.url().endsWith(`/api/v0/agents/inbox/${encodeURIComponent(request.id)}/answer`));
        await card.getByRole("button", { name: "Approve once", exact: true }).click();
        assert.equal((await answered).status(), 202);

        let settledRun;
        for (let attempt = 0; attempt < 60; attempt++) {
            settledRun = (await jsonGet(`/api/v0/webhooks/${webhookId}/runs`)).runs
                .find((run) => run.id === originalRun.id);
            if (settledRun?.status === "ok") break;
            await page.waitForTimeout(250);
        }
        assert.equal(settledRun?.status, "ok", `the original run did not settle; last status: ${settledRun?.status}`);
        assert.equal(settledRun.assistant_turn_id, originalRun.assistant_turn_id);
        assert.equal(settledRun.session_id, sessionId);
        const detail = await jsonGet(`/api/v0/chat/sessions/${sessionId}`);
        const originalTurn = detail.turns.find((item) => item.turn.id === originalRun.assistant_turn_id);
        assert.ok(originalTurn);
        assert.equal(originalTurn.turn.status, "completed");
        assert.equal(originalTurn.turn.content, "The browser action probe finished.");
        assert.equal(originalTurn.tool_calls.length, 1);
        const call = originalTurn.tool_calls[0];
        assert.equal(call.id, "dot-action-browser");
        assert.equal(call.name, "browse_page");
        assert.deepEqual(JSON.parse(call.arguments_json), originalArguments);
        assert.equal(call.status, "errored");
        const output = JSON.parse(call.output_json);
        assert.equal(output.error, "`browse_page` action `navigate` is denied by the current tool rules. Change the rule before retrying.");
        assert.doesNotMatch(call.output_json, /fixture|sandbox containers|connection refused/i);
        assert.ok(!(await jsonGet("/api/v0/agents/inbox")).items
            .some((item) => item.turn_id === originalRun.assistant_turn_id));
    } finally {
        try {
            if (context) {
                try {
                    if (sessionId) await context.request.post(`${BASE}/api/v0/chat/sessions/${sessionId}/cancel`, { timeout: 5000 });
                } finally {
                    try {
                        if (webhookId) {
                            await context.request.post(`${BASE}/api/v0/webhooks/${webhookId}/toggle`, { data: { enabled: false }, timeout: 5000 });
                            await context.request.delete(`${BASE}/api/v0/webhooks/${webhookId}`, { timeout: 5000 });
                        }
                    } finally {
                        if (dotId) await context.request.put(`${BASE}/api/v0/dots/${dotId}/active`, { data: { active: false }, timeout: 5000 });
                    }
                }
            }
        } finally {
            try { await context?.close(); }
            finally { await browser.close(); }
        }
    }
});
