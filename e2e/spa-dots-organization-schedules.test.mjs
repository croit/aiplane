import { test } from "node:test";
import assert from "node:assert/strict";
import { BASE, chooseSearchable, devSessionCookie, gatewayIsUp, launchBrowser } from "./helpers.mjs";

test("An organization schedule fires its pinned published version as a principal and reopens private history", async () => {
    assert.ok(await gatewayIsUp(), `gateway is not reachable at ${BASE}`);
    const memberCookie = process.env.AIPLANE_DOT_MEMBER_SESSION_COOKIE?.trim().replace(/^id=/, "");
    assert.ok(memberCookie, "set AIPLANE_DOT_MEMBER_SESSION_COOKIE to the seeded Engineering member session");
    const browser = await launchBrowser();
    let admin;
    let member;
    let originalGroup;
    let organization;
    let dotId;
    let schedule;
    let pendingSchedule;
    let pendingRun;
    async function cleanupSchedule(action) {
        if (!member || !action) return;
        const response = await member.request.get(`${BASE}/api/v0/scheduled/${action.id}/runs`, { timeout: 5000 });
        if (response.status() !== 200) return;
        const actual = await response.json();
        if (actual.action.enabled) await member.request.post(`${BASE}/api/v0/scheduled/${action.id}/toggle`, { data: { enabled: false, expected_configuration_generation: actual.action.configuration_generation }, timeout: 5000 });
        const unfinished = actual.runs.filter((run) => (run.status === null || run.status === "waiting") && run.session_id && run.assistant_turn_id);
        for (const run of unfinished) await member.request.post(`${BASE}/api/v0/agents/${dotId}/test/${run.session_id}/cancel`, { data: { turn_id: run.assistant_turn_id }, timeout: 5000 });
        if (unfinished.length) {
            for (let attempt = 0; attempt < 60; attempt++) {
                const current = await member.request.get(`${BASE}/api/v0/scheduled/${action.id}/runs`, { timeout: 5000 });
                if (current.status() !== 200) break;
                if (!(await current.json()).runs.some((run) => run.status === null || run.status === "waiting")) break;
                await new Promise((resolve) => setTimeout(resolve, 250));
            }
        }
        await member.request.delete(`${BASE}/api/v0/scheduled/${action.id}`, { timeout: 5000 });
    }
    try {
        admin = await browser.newContext({ viewport: { width: 1280, height: 900 } });
        await admin.addCookies([{ name: "id", value: await devSessionCookie(), url: BASE }, { name: "lang", value: "en", url: BASE }]);
        assert.equal((await admin.request.post(`${BASE}/api/v0/admin/users/dev/impersonate`)).status(), 200);
        assert.equal((await (await admin.request.get(`${BASE}/api/v0/me`)).json()).id, "dev");
        member = await browser.newContext({ viewport: { width: 1280, height: 900 } });
        await member.addCookies([{ name: "id", value: memberCookie, url: BASE }, { name: "lang", value: "en", url: BASE }]);
        const groups = await admin.request.get(`${BASE}/api/v0/admin/groups`);
        assert.equal(groups.status(), 200);
        originalGroup = (await groups.json()).groups.find((group) => group.name === "engineering");
        assert.ok(originalGroup);
        assert.equal((await admin.request.put(`${BASE}/api/v0/admin/groups`, { data: { ...originalGroup, can_manage_agents: true, can_manage_dots: false } })).status(), 200);
        const created = await admin.request.post(`${BASE}/api/v0/admin/organizations`, { data: { name: `Scheduled organization ${Date.now()}`, description: "Principal scheduled execution acceptance.", member_groups: [originalGroup.name] } });
        assert.equal(created.status(), 201);
        organization = (await created.json()).organization;
        const orgUrl = `${BASE}/api/v0/organizations/${organization.id}`;
        const dotResponse = await member.request.post(`${orgUrl}/dots`, { data: { name: "Harald", display: "Harald Scheduled", description: "Published schedule acceptance." } });
        assert.equal(dotResponse.status(), 201);
        dotId = (await dotResponse.json()).agent.id;
        const inventory = await member.request.get(`${BASE}/api/v0/agent-resources?agent_id=${encodeURIComponent(dotId)}`);
        assert.equal(inventory.status(), 200);
        const model = (await inventory.json()).models.chat.find((entry) => entry.id === "demo-model");
        assert.ok(model);
        assert.equal((await member.request.post(`${BASE}/api/v0/system-principals/${dotId}/grants`, { data: { kind: "model", ref: model.id } })).status(), 201);
        assert.equal((await member.request.put(`${BASE}/api/v0/agents/${dotId}/draft`, { data: { spec: { main: { model: model.id, instructions: { orchestration: "Answer the explicit scheduled request." }, tools: [] } } } })).status(), 200);
        assert.equal((await member.request.post(`${BASE}/api/v0/agents/${dotId}/publish`)).status(), 201);
        const agent = (await (await member.request.get(`${BASE}/api/v0/agents/${dotId}`)).json()).agent;
        const version = agent.live_version;
        assert.ok(version > 0);
        const page = await member.newPage();
        await page.goto(`${BASE}/agents/${dotId}?tab=setup`, { waitUntil: "networkidle" });
        await page.getByRole("link", { name: "Scheduled actions", exact: true }).click();
        await page.getByRole("link", { name: "New scheduled action", exact: true }).click();
        await page.getByRole("textbox", { name: "Name", exact: true }).fill("Check support");
        await page.getByRole("textbox", { name: "Prompt", exact: true }).fill("Organization scheduled private probe");
        const versionPicker = page.getByRole("combobox", { name: "Published version", exact: true });
        await versionPicker.selectOption(String(version));
        assert.equal(await page.getByRole("combobox", { name: "Run as", exact: true }).count(), 0);
        assert.equal(await page.getByRole("combobox", { name: "Model", exact: true }).count(), 0);
        assert.equal(await page.getByRole("checkbox", { name: /^Continue one conversation/ }).count(), 0);
        await page.getByRole("radio", { name: "Advanced", exact: true }).check();
        await page.getByRole("textbox", { name: "Cron expression", exact: true }).fill("* * * * *");
        await chooseSearchable(page, page.getByRole("combobox", { name: "Timezone", exact: true }), "UTC");
        const savedPromise = page.waitForResponse((response) => response.request().method() === "POST" && response.url() === `${BASE}/api/v0/scheduled`);
        await page.getByRole("button", { name: "Create scheduled action", exact: true }).click();
        const saved = await savedPromise;
        assert.equal(saved.status(), 201);
        const sent = saved.request().postDataJSON();
        assert.equal(sent.principal_id, dotId);
        assert.equal(sent.agent_version, version);
        assert.equal(sent.timezone, "UTC");
        assert.equal(sent.reuse_conversation, false);
        assert.equal(Object.hasOwn(sent, "model"), false);
        schedule = await saved.json();
        assert.equal(schedule.principal_id, dotId);
        assert.equal(schedule.agent_version, version);
        assert.equal(schedule.model, null);
        assert.ok(schedule.configuration_generation);
        assert.equal((await member.request.put(`${BASE}/api/v0/agents/${dotId}/draft`, { data: { spec: { main: { model: model.id, instructions: { orchestration: "A later version must not rebase a schedule." }, tools: [] } } } })).status(), 200);
        assert.equal((await member.request.post(`${BASE}/api/v0/agents/${dotId}/publish`)).status(), 201);
        let runs;
        for (let attempt = 0; attempt < 90; attempt++) {
            const response = await member.request.get(`${BASE}/api/v0/scheduled/${schedule.id}/runs`, { timeout: 5000 });
            assert.equal(response.status(), 200);
            runs = await response.json();
            if (runs.runs.some((run) => run.status === "ok")) break;
            await page.waitForTimeout(1000);
        }
        const run = runs.runs.find((entry) => entry.status === "ok");
        assert.ok(run, "the real scheduled worker must finish within ninety finite polls");
        assert.equal(run.agent_version, version);
        assert.equal(run.configuration_generation, schedule.configuration_generation);
        assert.equal((await member.request.post(`${BASE}/api/v0/scheduled/${schedule.id}/toggle`, { data: { enabled: false, expected_configuration_generation: schedule.configuration_generation } })).status(), 200);
        assert.equal((await member.request.get(`${BASE}/api/v0/chat/sessions/${run.session_id}`)).status(), 404);
        await page.goto(`${BASE}/scheduled?principal_id=${encodeURIComponent(dotId)}`, { waitUntil: "networkidle" });
        await page.getByRole("link", { name: "Open chat", exact: true }).click();
        await page.waitForURL((url) => url.pathname === `/agents/${dotId}` && url.searchParams.get("test_session") === run.session_id);
        const chat = page.locator("[data-test-chat]");
        await chat.getByText(`This private conversation uses published version ${version}.`, { exact: true }).waitFor({ timeout: 15000 });
        await chat.getByText("Hi! How can I help?", { exact: true }).waitFor();
        assert.equal(await chat.getByRole("combobox", { name: "Conversation source", exact: true }).inputValue(), "published");
        const debug = await member.request.get(`${BASE}/api/v0/agents/${dotId}/test/${run.session_id}/turns/${run.assistant_turn_id}/debug`);
        assert.equal(debug.status(), 200);
        const actual = await debug.json();
        assert.equal(actual.source, "published");
        assert.equal(actual.agent_version, version);
        assert.equal((await admin.request.get(`${BASE}/api/v0/scheduled/${schedule.id}/runs`)).status(), 404);
        await page.setViewportSize({ width: 390, height: 844 });
        assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true);
        await chat.getByRole("button", { name: "New conversation", exact: true }).click();
        await page.waitForURL((url) => !url.searchParams.has("test_session"));
        assert.equal(await chat.getByRole("combobox", { name: "Conversation source", exact: true }).inputValue(), "draft");
        assert.equal(await chat.getByText("Organization scheduled private probe", { exact: true }).count(), 0);
        const humanSpec = {
            main: { model: model.id, instructions: { orchestration: "Ask the human about the explicit scheduled request." }, tools: [] },
            state: { issue: { type: "enum", values: ["other"], set_by: ["llm"] } },
            router: { kind: "rules", order: ["staff"] },
            routes: { staff: { when: { slot: "issue", eq: "other" }, human: { inbox: "support", timeout: "30m" } } },
        };
        assert.equal((await member.request.put(`${BASE}/api/v0/agents/${dotId}/draft`, { data: { spec: humanSpec } })).status(), 200);
        assert.equal((await member.request.post(`${BASE}/api/v0/agents/${dotId}/publish`)).status(), 201);
        const humanVersion = (await (await member.request.get(`${BASE}/api/v0/agents/${dotId}`)).json()).agent.live_version;
        const pendingCreated = await member.request.post(`${BASE}/api/v0/scheduled`, { data: {
            principal_id: dotId, agent_version: humanVersion, name: "Ask support", prompt: "Please ask a human about this scheduled request",
            cron: "* * * * *", timezone: "UTC", tools_enabled: true, reuse_conversation: false, reuse_rounds: 5,
        } });
        assert.equal(pendingCreated.status(), 201);
        pendingSchedule = await pendingCreated.json();
        for (let attempt = 0; attempt < 90; attempt++) {
            const response = await member.request.get(`${BASE}/api/v0/scheduled/${pendingSchedule.id}/runs`, { timeout: 5000 });
            assert.equal(response.status(), 200);
            pendingRun = (await response.json()).runs.find((entry) => entry.status === "waiting");
            if (pendingRun) break;
            await page.waitForTimeout(1000);
        }
        assert.ok(pendingRun, "the actual scripted model must suspend the scheduled turn within ninety finite polls");
        const disabled = await member.request.post(`${BASE}/api/v0/scheduled/${pendingSchedule.id}/toggle`, { data: { enabled: false, expected_configuration_generation: pendingSchedule.configuration_generation } });
        assert.equal(disabled.status(), 200);
        await page.setViewportSize({ width: 1280, height: 900 });
        await page.goto(`${BASE}/scheduled/${pendingSchedule.id}/runs`, { waitUntil: "networkidle" });
        await page.getByRole("link", { name: "open chat", exact: true }).click();
        await page.waitForURL((url) => url.pathname === `/agents/${dotId}` && url.searchParams.get("test_session") === pendingRun.session_id);
        await chat.getByText(`This private conversation uses published version ${humanVersion}.`, { exact: true }).waitFor({ timeout: 15000 });
        const stopResponsePromise = page.waitForResponse((response) => response.request().method() === "POST" && response.url() === `${BASE}/api/v0/agents/${dotId}/test/${pendingRun.session_id}/cancel`);
        await chat.getByRole("button", { name: "Stop", exact: true }).click();
        const stopped = await stopResponsePromise;
        assert.equal(stopped.status(), 200);
        assert.deepEqual(stopped.request().postDataJSON(), { turn_id: pendingRun.assistant_turn_id });
        assert.deepEqual(await stopped.json(), { ok: true });
        let terminal;
        for (let attempt = 0; attempt < 60; attempt++) {
            const response = await member.request.get(`${BASE}/api/v0/scheduled/${pendingSchedule.id}/runs`, { timeout: 5000 });
            assert.equal(response.status(), 200);
            terminal = (await response.json()).runs.find((entry) => entry.id === pendingRun.id);
            if (terminal?.status === "error") break;
            await page.waitForTimeout(250);
        }
        assert.equal(terminal.status, "error");
        assert.equal(terminal.session_id, pendingRun.session_id);
        assert.equal(terminal.assistant_turn_id, pendingRun.assistant_turn_id);
        assert.match(terminal.error, /cancelled/);
        const terminalDebug = await member.request.get(`${BASE}/api/v0/agents/${dotId}/test/${pendingRun.session_id}/turns/${pendingRun.assistant_turn_id}/debug`);
        assert.equal(terminalDebug.status(), 200);
        assert.equal((await terminalDebug.json()).agent_version, humanVersion);
        const events = await member.request.get(`${BASE}/api/v0/agents/${dotId}/test/${pendingRun.session_id}/events`, { timeout: 5000 });
        assert.equal(events.status(), 200);
        const snapshot = (await events.text()).split("\n").filter((line) => line.startsWith("data:")).map((line) => JSON.parse(line.slice(5).trim())).find((event) => event.type === "snapshot");
        assert.equal(snapshot.turns.find((entry) => entry.turn.id === pendingRun.assistant_turn_id).turn.status, "cancelled");
    } finally {
        try {
            try { await cleanupSchedule(pendingSchedule); }
            finally {
                try { await cleanupSchedule(schedule); }
                finally { if (member && dotId) await member.request.delete(`${BASE}/api/v0/agents/${dotId}`, { timeout: 5000 }); }
            }
            if (admin && organization) await admin.request.put(`${BASE}/api/v0/admin/organizations/${organization.id}`, { data: { name: organization.name, description: organization.description, member_groups: [], expected_revision: organization.revision }, timeout: 5000 });
        } finally {
            try { if (admin && originalGroup) await admin.request.put(`${BASE}/api/v0/admin/groups`, { data: originalGroup, timeout: 5000 }); }
            finally {
                try { if (admin) await admin.request.post(`${BASE}/api/v0/admin/impersonate/stop`, { timeout: 5000 }); }
                finally { try { await member?.close(); } finally { try { await admin?.close(); } finally { await browser.close(); } } }
            }
        }
    }
});
