import { test } from "node:test";
import assert from "node:assert/strict";
import { devSessionCookie, gatewayIsUp, launchBrowser, BASE } from "./helpers.mjs";

test("existing automation forms retain Dot binding, filter reused chats and allow inactive edits", async () => {
    assert.ok(await gatewayIsUp(), `gateway is not reachable at ${BASE}`);
    const browser = await launchBrowser();
    const context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
    await context.addCookies([
        { name: "id", value: await devSessionCookie(), url: BASE },
        { name: "lang", value: "en", url: BASE },
    ]);
    const page = await context.newPage();
    const suffix = Date.now();
    const dotName = `Background Dot ${suffix}`;
    let dotId;
    let scheduleId;
    let webhookId;
    let webhookGeneration;

    async function choose(label, option) {
        await page.getByRole("combobox", { name: label, exact: true }).click();
        await page.getByRole("option", { name: option, exact: true }).click();
    }

    try {
        const created = await context.request.post(`${BASE}/api/v0/dots`, { data: { name: dotName } });
        assert.equal(created.status(), 201);
        dotId = (await created.json()).dot.id;
        const dotSessionResponse = await context.request.post(`${BASE}/api/v0/dots/${dotId}/sessions`, { data: {} });
        assert.equal(dotSessionResponse.status(), 201);
        const dotSession = (await dotSessionResponse.json()).session.id;
        const ordinaryResponse = await context.request.post(`${BASE}/api/v0/chat/sessions`, { data: {} });
        const ordinarySession = (await ordinaryResponse.json()).session.id;

        await page.goto(`${BASE}/dots`, { waitUntil: "networkidle" });
        const card = page.locator("article").filter({ has: page.getByRole("heading", { name: dotName, exact: true }) });
        await card.getByRole("link", { name: "Add scheduled work", exact: true }).click();
        await page.getByRole("combobox", { name: "Run as", exact: true }).getByText(dotName, { exact: true }).waitFor();
        await page.getByRole("textbox", { name: "Name", exact: true }).fill(`Dot schedule ${suffix}`);
        await page.getByRole("textbox", { name: "Prompt", exact: true }).fill("Summarize project progress.");
        await page.getByRole("radio", { name: "Advanced", exact: true }).check();
        await page.getByRole("textbox", { name: "Cron expression", exact: true }).fill("0 0 1 1 *");
        const scopedResponse = page.waitForResponse((response) => response.url().includes(`/api/v0/chat/sessions?dot_id=${dotId}`));
        await page.getByRole("checkbox", { name: /^Continue one conversation/ }).check();
        const scoped = await (await scopedResponse).json();
        assert.deepEqual(scoped.sessions.map((session) => session.id), [dotSession]);
        assert.ok(!scoped.sessions.some((session) => session.id === ordinarySession));
        const scheduleSaved = page.waitForResponse((response) => response.request().method() === "POST" && response.url().endsWith("/api/v0/scheduled"));
        await page.getByRole("button", { name: "Create scheduled action", exact: true }).click();
        const schedule = await (await scheduleSaved).json();
        assert.equal(schedule.dot_id, dotId);
        scheduleId = schedule.id;
        await page.waitForURL(/\/scheduled\?notice=created/);
        await page.getByRole("link", { name: `Dot: ${dotName}`, exact: true }).waitFor();

        await page.goto(`${BASE}/dots`, { waitUntil: "networkidle" });
        await card.getByRole("link", { name: "Add webhook", exact: true }).click();
        await page.getByRole("combobox", { name: "Run as", exact: true }).getByText(dotName, { exact: true }).waitFor();
        await page.getByRole("textbox", { name: "Name", exact: true }).fill(`Dot webhook ${suffix}`);
        await page.getByRole("textbox", { name: "Prompt", exact: true }).fill("Summarize the incoming event.");
        await page.getByRole("checkbox", { name: /^Wait for the response/ }).check();
        const webhookSaved = page.waitForResponse((response) => response.request().method() === "POST" && response.url().endsWith("/api/v0/webhooks"));
        await page.getByRole("button", { name: "Create webhook", exact: true }).click();
        const webhook = await (await webhookSaved).json();
        webhookId = webhook.webhook.id;
        assert.equal(webhook.webhook.dot_id, dotId);
        await page.getByRole("link", { name: "Done — back to webhooks", exact: true }).click();
        await page.getByRole("link", { name: `Dot: ${dotName}`, exact: true }).waitFor();
        const fired = await context.request.post(`${BASE}/hooks/${webhook.secret}`, { data: { event: "updated" } });
        assert.equal(fired.status(), 200);
        const runSession = (await fired.json()).session_id;
        const filtered = await context.request.get(`${BASE}/api/v0/chat/sessions?dot_id=${dotId}`);
        assert.ok((await filtered.json()).sessions.some((session) => session.id === runSession));
        await page.goto(`${BASE}/webhooks/${webhookId}/runs`, { waitUntil: "networkidle" });
        await page.getByRole("link", { name: `Dot: ${dotName}`, exact: true }).waitFor();

        await context.request.put(`${BASE}/api/v0/dots/${dotId}/active`, { data: { active: false } });
        await page.goto(`${BASE}/scheduled/${scheduleId}/edit`, { waitUntil: "networkidle" });
        await page.getByText("This Dot is inactive. You can edit this automation, but it cannot run until the Dot is activated.", { exact: true }).waitFor();
        await page.getByRole("textbox", { name: "Name", exact: true }).fill(`Renamed Dot schedule ${suffix}`);
        const editSaved = page.waitForResponse((response) => response.request().method() === "PUT" && response.url().endsWith(`/api/v0/scheduled/${scheduleId}`));
        await page.getByRole("button", { name: "Save changes", exact: true }).click();
        assert.equal((await (await editSaved).json()).dot_id, dotId);
        await page.waitForURL(/\/scheduled\?notice=saved/);

        await page.goto(`${BASE}/scheduled/${scheduleId}/edit`, { waitUntil: "networkidle" });
        await choose("Run as", "My personal assistant");
        await page.getByRole("combobox", { name: "Continue in", exact: true }).getByText("A new chat, opened by the next run", { exact: true }).waitFor();
        await page.setViewportSize({ width: 390, height: 844 });
        assert.ok(await page.locator("main").evaluate((element) => element.scrollWidth <= element.clientWidth), "automation editor must fit a mobile viewport");
    } finally {
        if (scheduleId) await context.request.delete(`${BASE}/api/v0/scheduled/${scheduleId}`);
        if (webhookId) await context.request.delete(`${BASE}/api/v0/webhooks/${webhookId}`);
        if (dotId) await context.request.put(`${BASE}/api/v0/dots/${dotId}/active`, { data: { active: false } });
        await context.close();
        await browser.close();
    }
});

test("pending Dot webhook is approved in Inbox and settles its exact run without writing user memory", async () => {
    assert.ok(await gatewayIsUp(), `gateway is not reachable at ${BASE}`);
    const browser = await launchBrowser();
    const context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
    await context.addCookies([
        { name: "id", value: await devSessionCookie(), url: BASE },
        { name: "lang", value: "en", url: BASE },
    ]);
    const page = await context.newPage();
    const suffix = Date.now();
    let dotId;
    let webhookId;
    let webhookGeneration;
    let sessionId;

    async function jsonGet(path) {
        const response = await context.request.get(`${BASE}${path}`);
        assert.equal(response.status(), 200);
        return response.json();
    }

    async function pollRun(runId, expectedStatus) {
        let observed;
        for (let attempt = 0; attempt < 60; attempt++) {
            const history = await jsonGet(`/api/v0/webhooks/${webhookId}/runs`);
            observed = history.runs.find((run) => run.id === runId);
            if (observed?.status === expectedStatus) return observed;
            await page.waitForTimeout(250);
        }
        assert.fail(`the exact webhook run did not reach ${expectedStatus}; last status: ${observed?.status}`);
    }

    try {
        const userMemoriesBefore = (await jsonGet("/api/v0/memories")).memories;
        const created = await context.request.post(`${BASE}/api/v0/dots`, {
            data: { name: `Approval Dot ${suffix}` },
        });
        assert.equal(created.status(), 201);
        dotId = (await created.json()).dot.id;
        const access = await context.request.put(`${BASE}/api/v0/dots/${dotId}/access`, {
            data: { tools: [{ id: "remember", permission: "always_ask" }] },
        });
        assert.equal(access.status(), 200);
        const hookCreated = await context.request.post(`${BASE}/api/v0/webhooks`, {
            data: {
                name: `Approval webhook ${suffix}`,
                prompt: "Dot background approval probe",
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
        webhookGeneration = hook.webhook.configuration_generation;
        const fired = await context.request.post(`${BASE}/hooks/${hook.secret}`, {
            data: { event: "approval-probe" },
        });
        const result = await fired.json();
        sessionId = result.session_id;
        assert.equal(fired.status(), 202);
        assert.equal(result.status, "waiting");
        assert.ok(sessionId);
        assert.deepEqual((await jsonGet(`/api/v0/dots/${dotId}/memories`)).memories, []);

        const history = await jsonGet(`/api/v0/webhooks/${webhookId}/runs`);
        assert.equal(history.runs.length, 1);
        const originalRun = history.runs[0];
        assert.equal(originalRun.status, "waiting");
        assert.equal(originalRun.session_id, sessionId);
        assert.ok(originalRun.assistant_turn_id);
        const deletion = await context.request.delete(`${BASE}/api/v0/webhooks/${webhookId}`);
        assert.equal(deletion.status(), 409);
        const paused = await context.request.post(`${BASE}/api/v0/webhooks/${webhookId}/toggle`, {
            data: { enabled: false, expected_configuration_generation: webhookGeneration },
        });
        assert.equal(paused.status(), 200);

        const inbox = await jsonGet("/api/v0/agents/inbox");
        const request = inbox.items.find((item) => item.turn_id === originalRun.assistant_turn_id);
        assert.ok(request, "the original assistant turn must appear in Inbox");
        assert.equal(request.session_id, sessionId);
        assert.equal(request.kind, "approval");
        assert.equal(request.call.name, "remember");
        const originalArguments = JSON.parse(request.call.arguments);
        assert.equal(originalArguments.content, "Approved background lesson.");
        await page.goto(`${BASE}/inbox?item=${encodeURIComponent(request.id)}`, { waitUntil: "networkidle" });
        const card = page.locator(`[id="inbox-item-${request.id}"]`);
        await card.getByRole("button", { name: "Approve once", exact: true }).waitFor();
        assert.deepEqual(JSON.parse(await card.locator("pre").innerText()), originalArguments);
        const answered = page.waitForResponse((response) => response.request().method() === "POST"
            && response.url().endsWith(`/api/v0/agents/inbox/${encodeURIComponent(request.id)}/answer`));
        await card.getByRole("button", { name: "Approve once", exact: true }).click();
        assert.equal((await answered).status(), 202);

        const completed = await pollRun(originalRun.id, "ok");
        assert.equal(completed.assistant_turn_id, originalRun.assistant_turn_id);
        assert.equal(completed.session_id, sessionId);
        const dotMemories = (await jsonGet(`/api/v0/dots/${dotId}/memories`)).memories;
        assert.equal(dotMemories.length, 1);
        assert.equal(dotMemories[0].content, "Approved background lesson.");
        assert.deepEqual((await jsonGet("/api/v0/memories")).memories, userMemoriesBefore);
        const settledInbox = await jsonGet("/api/v0/agents/inbox");
        assert.ok(!settledInbox.items.some((item) => item.turn_id === originalRun.assistant_turn_id));
        const hooks = await jsonGet("/api/v0/webhooks");
        const settledHook = hooks.webhooks.find((item) => item.id === webhookId);
        assert.equal(settledHook.enabled, false);
        assert.equal(settledHook.last_status, "ok");
    } finally {
        if (sessionId) await context.request.post(`${BASE}/api/v0/chat/sessions/${sessionId}/cancel`);
        if (webhookId) {
            await context.request.post(`${BASE}/api/v0/webhooks/${webhookId}/toggle`, { data: { enabled: false, expected_configuration_generation: webhookGeneration } });
            await context.request.delete(`${BASE}/api/v0/webhooks/${webhookId}`);
        }
        if (dotId) await context.request.put(`${BASE}/api/v0/dots/${dotId}/active`, { data: { active: false } });
        await context.close();
        await browser.close();
    }
});
