import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import { BASE, devSessionCookie, gatewayIsUp, launchBrowser } from "./helpers.mjs";

test("Organization webhooks retain their published pin, rotate and pause public firing, and replay into private history", async () => {
    assert.ok(await gatewayIsUp(), `gateway is not reachable at ${BASE}`);
    const memberCookie = process.env.AIPLANE_DOT_MEMBER_SESSION_COOKIE?.trim().replace(/^id=/, "");
    assert.ok(memberCookie, "set AIPLANE_DOT_MEMBER_SESSION_COOKIE to the seeded Engineering member session");
    const capture = process.env.DOCS_ORG_DOT_WEBHOOK_CAPTURE;
    const browser = await launchBrowser();
    let admin;
    let member;
    let originalGroup;
    let organization;
    let dotId;
    let webhook;
    async function inventory() {
        const response = await member.request.get(`${BASE}/api/v0/webhooks?principal_id=${encodeURIComponent(dotId)}`, { timeout: 5000 });
        assert.equal(response.status(), 200);
        return response.json();
    }
    async function refreshWebhook() {
        webhook = (await inventory()).webhooks.find((row) => row.id === webhook.id);
        assert.ok(webhook);
    }
    async function runFor(sessionId, status) {
        for (let attempt = 0; attempt < 60; attempt++) {
            const response = await member.request.get(`${BASE}/api/v0/webhooks/${webhook.id}/runs`, { timeout: 5000 });
            assert.equal(response.status(), 200);
            const run = (await response.json()).runs.find((row) => row.session_id === sessionId);
            if (run?.status === status) return run;
            await new Promise((resolve) => setTimeout(resolve, 250));
        }
        assert.fail(`The webhook run did not reach ${status}.`);
    }
    async function publish(spec) {
        assert.equal((await member.request.put(`${BASE}/api/v0/agents/${dotId}/draft`, { data: { spec } })).status(), 200);
        assert.equal((await member.request.post(`${BASE}/api/v0/agents/${dotId}/publish`)).status(), 201);
        const response = await member.request.get(`${BASE}/api/v0/agents/${dotId}`);
        assert.equal(response.status(), 200);
        return (await response.json()).agent.live_version;
    }
    try {
        admin = await browser.newContext({ viewport: { width: 1280, height: 900 } });
        await admin.addCookies([{ name: "id", value: await devSessionCookie(), url: BASE }, { name: "lang", value: "en", url: BASE }]);
        assert.equal((await admin.request.post(`${BASE}/api/v0/admin/users/dev/impersonate`)).status(), 200);
        assert.equal((await (await admin.request.get(`${BASE}/api/v0/me`)).json()).id, "dev");
        member = await browser.newContext(capture
            ? { viewport: { width: 1400, height: 950 }, deviceScaleFactor: 2, colorScheme: "dark" }
            : { viewport: { width: 1280, height: 900 } });
        await member.addCookies([{ name: "id", value: memberCookie, url: BASE }, { name: "lang", value: "en", url: BASE }]);
        const groups = await admin.request.get(`${BASE}/api/v0/admin/groups`);
        assert.equal(groups.status(), 200);
        originalGroup = (await groups.json()).groups.find((group) => group.name === "engineering");
        assert.ok(originalGroup);
        assert.equal((await admin.request.put(`${BASE}/api/v0/admin/groups`, { data: { ...originalGroup, can_manage_agents: true, can_manage_dots: false } })).status(), 200);
        const createdOrganization = await admin.request.post(`${BASE}/api/v0/admin/organizations`, { data: { name: `Webhook organization ${Date.now()}`, description: "Principal webhook acceptance.", member_groups: [originalGroup.name] } });
        assert.equal(createdOrganization.status(), 201);
        organization = (await createdOrganization.json()).organization;
        const createdDot = await member.request.post(`${BASE}/api/v0/organizations/${organization.id}/dots`, { data: { name: "Harald", display: "Harald Webhooks", description: "Published webhook acceptance." } });
        assert.equal(createdDot.status(), 201);
        dotId = (await createdDot.json()).agent.id;
        const resources = await member.request.get(`${BASE}/api/v0/agent-resources?agent_id=${encodeURIComponent(dotId)}`);
        assert.equal(resources.status(), 200);
        const model = (await resources.json()).models.chat.find((row) => row.id === "demo-model");
        assert.ok(model);
        assert.equal((await member.request.post(`${BASE}/api/v0/system-principals/${dotId}/grants`, { data: { kind: "model", ref: model.id } })).status(), 201);
        const version = await publish({ main: { model: model.id, instructions: { orchestration: "Answer the explicit webhook event." }, tools: [] } });
        assert.ok(version > 0);
        const page = await member.newPage();
        await page.goto(`${BASE}/agents/${dotId}?tab=setup`, { waitUntil: "networkidle" });
        await page.getByRole("main").getByRole("link", { name: "Webhooks", exact: true }).click();
        await page.waitForURL((url) => url.pathname === "/webhooks" && url.searchParams.get("principal_id") === dotId);
        await page.getByRole("link", { name: "New webhook", exact: true }).click();
        await page.getByRole("textbox", { name: "Name", exact: true }).fill("Review support events");
        await page.getByRole("textbox", { name: "Prompt", exact: true }).fill("Organization webhook private probe");
        await page.getByRole("combobox", { name: "Published version", exact: true }).selectOption(String(version));
        assert.equal(await page.getByRole("combobox", { name: "Model", exact: true }).count(), 0);
        assert.equal(await page.getByRole("combobox", { name: "Run as", exact: true }).count(), 0);
        assert.equal(await page.getByRole("checkbox", { name: /^Wait for the response/ }).count(), 0);
        assert.equal(await page.getByRole("checkbox", { name: /^Continue one conversation/ }).count(), 0);
        const tools = page.getByRole("checkbox", { name: "Allow the agent’s tools", exact: true });
        await tools.focus();
        await page.keyboard.press("Space");
        assert.equal(await tools.isChecked(), true);
        await page.setViewportSize({ width: 390, height: 844 });
        assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true);
        await page.setViewportSize(capture ? { width: 1400, height: 950 } : { width: 1280, height: 900 });
        if (capture) {
            await page.evaluate(async () => {
                document.documentElement.setAttribute("data-theme", "dark");
                await document.fonts.ready;
            });
            await page.screenshot({ path: capture, animations: "disabled", fullPage: false });
            const entry = {
                file: capture, route: `/webhooks/new?principal_id=${dotId}`, selector: "viewport",
                viewport: { width: 1400, height: 950 }, scale: 2, width: 2800, height: 1900,
                data: `Synthetic dev-ui fixture; actual Engineering organization Writer without personal Dot management. Harald Webhooks, authored published version ${version}, actual model ${model.id}, explicitly enabled Dot tools, and filled trusted prompt. Captured before secret creation by spa-dots-organization-webhooks.test.mjs with DOCS_ORG_DOT_WEBHOOK_CAPTURE.`,
            };
            fs.writeFileSync("/tmp/dot-organization-webhook-capture-entry.json", `${JSON.stringify(entry, null, 2)}\n`);
        }
        const created = page.waitForResponse((response) => response.request().method() === "POST" && response.url() === `${BASE}/api/v0/webhooks`);
        await page.getByRole("button", { name: "Create webhook", exact: true }).click();
        const createdResponse = await created;
        assert.equal(createdResponse.status(), 201);
        const body = createdResponse.request().postDataJSON();
        assert.equal(body.principal_id, dotId);
        assert.equal(body.agent_version, version);
        assert.equal(body.tools_enabled, true);
        assert.equal(body.synchronous, false);
        assert.equal(body.reuse_conversation, false);
        assert.equal(Object.hasOwn(body, "model"), false);
        assert.equal(Object.hasOwn(body, "dot_id"), false);
        const saved = await createdResponse.json();
        webhook = saved.webhook;
        assert.equal(webhook.principal_id, dotId);
        assert.equal(webhook.agent_version, version);
        const secretField = page.getByRole("textbox", { name: "Your trigger URL", exact: true });
        await secretField.waitFor();
        const originalUrl = await secretField.inputValue();
        assert.equal(new URL(originalUrl).pathname, `/hooks/${saved.secret}`);
        assert.equal(new URL(page.url()).searchParams.has("secret"), false);
        assert.equal((await admin.request.get(`${BASE}/api/v0/webhooks?principal_id=${dotId}`)).status(), 404);
        const newerVersion = await publish({ main: { model: model.id, instructions: { orchestration: "A newer version must not replace the stored webhook pin." }, tools: [] } });
        assert.ok(newerVersion > version);
        const fire = await member.request.post(originalUrl, { data: { event: "support created", ticket: "synthetic-1" } });
        assert.equal(fire.status(), 202);
        const accepted = await fire.json();
        assert.deepEqual(Object.keys(accepted), ["session_id"]);
        const first = await runFor(accepted.session_id, "ok");
        assert.equal(first.agent_version, version);
        assert.equal(first.configuration_generation, webhook.configuration_generation);
        assert.equal((await member.request.get(`${BASE}/api/v0/chat/sessions/${first.session_id}`)).status(), 404);
        assert.equal((await admin.request.get(`${BASE}/api/v0/webhooks/${webhook.id}/runs`)).status(), 404);
        await page.getByRole("link", { name: "Done — back to webhooks", exact: true }).click();
        await page.getByRole("link", { name: "Open chat", exact: true }).click();
        await page.waitForURL((url) => url.pathname === `/agents/${dotId}` && url.searchParams.get("test_session") === first.session_id);
        const chat = page.locator("[data-test-chat]");
        await chat.getByText(`This private conversation uses published version ${version}.`, { exact: true }).waitFor();
        await chat.getByText("Hi! How can I help?", { exact: true }).waitFor();
        const debugUrl = `${BASE}/api/v0/agents/${dotId}/test/${first.session_id}/turns/${first.assistant_turn_id}/debug`;
        const debug = await member.request.get(debugUrl);
        assert.equal(debug.status(), 200);
        assert.equal((await debug.json()).agent_version, version);
        assert.equal((await admin.request.get(debugUrl)).status(), 404);
        await page.goto(`${BASE}/webhooks?principal_id=${dotId}`, { waitUntil: "networkidle" });
        page.on("dialog", (dialog) => dialog.accept());
        const rotated = page.waitForResponse((response) => response.request().method() === "POST" && response.url() === `${BASE}/api/v0/webhooks/${webhook.id}/rotate`);
        await page.getByRole("button", { name: "Rotate secret", exact: true }).click();
        const rotation = await rotated;
        assert.equal(rotation.status(), 200);
        assert.equal(rotation.request().postDataJSON().expected_configuration_generation, webhook.configuration_generation);
        const rotatedSecret = (await rotation.json()).secret;
        const rotatedUrl = `${BASE}/hooks/${rotatedSecret}`;
        assert.equal((await member.request.post(originalUrl, { data: {} })).status(), 404);
        await page.getByRole("textbox", { name: "Your trigger URL", exact: true }).waitFor();
        await page.locator('aside a[href="/webhooks"]').click();
        await page.waitForURL((url) => url.pathname === "/webhooks" && !url.searchParams.has("principal_id"));
        await page.getByRole("heading", { name: "Review support events", exact: true }).waitFor({ state: "detached" });
        assert.equal(await page.getByRole("textbox", { name: "Your trigger URL", exact: true }).count(), 0);
        assert.equal(await page.getByText("Harald Webhooks", { exact: true }).count(), 0);
        await page.goBack({ waitUntil: "networkidle" });
        await page.waitForURL((url) => url.searchParams.get("principal_id") === dotId);
        await page.getByRole("heading", { name: "Review support events", exact: true }).waitFor();
        assert.equal(await page.getByRole("textbox", { name: "Your trigger URL", exact: true }).count(), 0);
        const paused = page.waitForResponse((response) => response.request().method() === "POST" && response.url() === `${BASE}/api/v0/webhooks/${webhook.id}/toggle`);
        await page.getByRole("button", { name: "Pause", exact: true }).click();
        const pause = await paused;
        assert.equal(pause.status(), 200);
        assert.equal(pause.request().postDataJSON().expected_configuration_generation, webhook.configuration_generation);
        assert.equal((await member.request.post(rotatedUrl, { data: {} })).status(), 404);
        await refreshWebhook();
        assert.equal(webhook.enabled, false);
        const humanVersion = await publish({
            main: { model: model.id, instructions: { orchestration: "Ask a human about this scheduled request." }, tools: [] },
            state: { issue: { type: "enum", values: ["other"], set_by: ["llm"] } },
            router: { kind: "rules", order: ["staff"] },
            routes: { staff: { when: { slot: "issue", eq: "other" }, human: { inbox: "support", timeout: "30m" } } }
        });
        await page.getByRole("link", { name: "Edit", exact: true }).click();
        await page.getByRole("combobox", { name: "Published version", exact: true }).selectOption(String(humanVersion));
        await page.getByRole("textbox", { name: "Prompt", exact: true }).fill("Please ask a human about this scheduled request");
        const updated = page.waitForResponse((response) => response.request().method() === "PUT" && response.url() === `${BASE}/api/v0/webhooks/${webhook.id}`);
        await page.getByRole("button", { name: "Save changes", exact: true }).click();
        const update = await updated;
        assert.equal(update.status(), 200);
        assert.equal(update.request().postDataJSON().expected_configuration_generation, webhook.configuration_generation);
        await refreshWebhook();
        assert.notEqual(webhook.configuration_generation, first.configuration_generation);
        await page.getByRole("link", { name: "rerun", exact: true }).click();
        const replayed = page.waitForResponse((response) => response.request().method() === "POST" && response.url() === `${BASE}/api/v0/webhooks/${webhook.id}/rerun`);
        await page.getByRole("button", { name: "Rerun", exact: true }).click();
        const replay = await replayed;
        assert.equal(replay.status(), 202);
        assert.equal(replay.request().postDataJSON().expected_configuration_generation, webhook.configuration_generation);
        const pendingSession = (await replay.json()).session_id;
        const pending = await runFor(pendingSession, "waiting");
        assert.equal(pending.source, "rerun");
        assert.equal(pending.agent_version, humanVersion);
        assert.equal(pending.configuration_generation, webhook.configuration_generation);
        await page.waitForURL((url) => url.searchParams.get("test_session") === pending.session_id);
        await chat.getByText(`This private conversation uses published version ${humanVersion}.`, { exact: true }).waitFor();
        await page.goto(`${BASE}/webhooks?principal_id=${dotId}`, { waitUntil: "networkidle" });
        const pendingRotation = page.waitForResponse((response) => response.request().method() === "POST" && response.url() === `${BASE}/api/v0/webhooks/${webhook.id}/rotate`);
        await page.getByRole("button", { name: "Rotate secret", exact: true }).click();
        const pendingRotated = await pendingRotation;
        assert.equal(pendingRotated.status(), 200);
        assert.equal(pendingRotated.request().postDataJSON().expected_configuration_generation, pending.configuration_generation);
        const pendingSecret = (await pendingRotated.json()).secret;
        assert.equal((await member.request.post(rotatedUrl, { data: {} })).status(), 404);
        assert.equal((await member.request.post(`${BASE}/hooks/${pendingSecret}`, { data: {} })).status(), 404);
        assert.equal((await runFor(pending.session_id, "waiting")).assistant_turn_id, pending.assistant_turn_id);
        await page.goto(`${BASE}/agents/${dotId}?tab=try&test_session=${encodeURIComponent(pending.session_id)}`, { waitUntil: "networkidle" });
        await chat.getByText(`This private conversation uses published version ${humanVersion}.`, { exact: true }).waitFor();
        const stopped = page.waitForResponse((response) => response.request().method() === "POST" && response.url() === `${BASE}/api/v0/agents/${dotId}/test/${pending.session_id}/cancel`);
        await chat.getByRole("button", { name: "Stop", exact: true }).click();
        const stop = await stopped;
        assert.equal(stop.status(), 200);
        assert.deepEqual(stop.request().postDataJSON(), { turn_id: pending.assistant_turn_id });
        assert.equal((await runFor(pending.session_id, "error")).assistant_turn_id, pending.assistant_turn_id);
        await page.goto(`${BASE}/webhooks?principal_id=${dotId}`, { waitUntil: "networkidle" });
        const deleted = page.waitForResponse((response) => response.request().method() === "DELETE" && response.url() === `${BASE}/api/v0/webhooks/${webhook.id}`);
        await page.getByRole("button", { name: "Delete", exact: true }).click();
        assert.equal((await deleted).status(), 204);
        assert.equal((await inventory()).webhooks.length, 0);
        webhook = null;
    } finally {
        try {
            if (member && webhook) {
                await refreshWebhook();
                if (webhook.enabled) await member.request.post(`${BASE}/api/v0/webhooks/${webhook.id}/toggle`, { data: { enabled: false, expected_configuration_generation: webhook.configuration_generation }, timeout: 5000 });
                const response = await member.request.get(`${BASE}/api/v0/webhooks/${webhook.id}/runs`, { timeout: 5000 });
                if (response.status() === 200) {
                    const runs = (await response.json()).runs;
                    for (const run of runs.filter((row) => (row.status === null || row.status === "waiting") && row.session_id && row.assistant_turn_id)) {
                        await member.request.post(`${BASE}/api/v0/agents/${dotId}/test/${run.session_id}/cancel`, { data: { turn_id: run.assistant_turn_id }, timeout: 5000 });
                        await runFor(run.session_id, "error");
                    }
                }
                await member.request.delete(`${BASE}/api/v0/webhooks/${webhook.id}`, { timeout: 5000 });
            }
        } finally {
            try {
                if (member && dotId) await member.request.delete(`${BASE}/api/v0/agents/${dotId}`, { timeout: 5000 });
                if (admin && organization) await admin.request.put(`${BASE}/api/v0/admin/organizations/${organization.id}`, { data: { name: organization.name, description: organization.description, member_groups: [], expected_revision: organization.revision }, timeout: 5000 });
            } finally {
                try {
                    if (admin && originalGroup) await admin.request.put(`${BASE}/api/v0/admin/groups`, { data: originalGroup, timeout: 5000 });
                } finally {
                    try { if (admin) await admin.request.post(`${BASE}/api/v0/admin/impersonate/stop`, { timeout: 5000 }); }
                    finally { try { await member?.close(); } finally { try { await admin?.close(); } finally { await browser.close(); } } }
                }
            }
        }
    }
});
