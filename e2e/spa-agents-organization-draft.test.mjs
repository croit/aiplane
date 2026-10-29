import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import { BASE, devSessionCookie, gatewayIsUp, launchBrowser } from "./helpers.mjs";

test("An organization Writer uses private saved-draft chat without personal permission and loses private access on revocation", async () => {
    assert.ok(await gatewayIsUp(), `gateway is not reachable at ${BASE}`);
    const memberCookie = process.env.AIPLANE_AGENT_MEMBER_SESSION_COOKIE?.trim().replace(/^id=/, "");
    assert.ok(memberCookie, "set AIPLANE_AGENT_MEMBER_SESSION_COOKIE to the seeded Engineering member session");
    const browser = await launchBrowser();
    const capture = process.env.DOCS_ORG_AGENT_DRAFT_CAPTURE;
    const publishedCapture = process.env.DOCS_ORG_AGENT_PUBLISHED_CAPTURE;
    let admin;
    let member;
    let originalGroup;
    let organization;
    let agentId;
    let releasePost;
    try {
        admin = await browser.newContext({ viewport: { width: 1280, height: 900 }, locale: "de-DE" });
        await admin.addCookies([{ name: "id", value: await devSessionCookie(), url: BASE }, { name: "lang", value: "en", url: BASE }]);
        const impersonated = await admin.request.post(`${BASE}/api/v0/admin/users/dev/impersonate`);
        assert.equal(impersonated.status(), 200);
        const adminMe = await admin.request.get(`${BASE}/api/v0/me`);
        assert.equal(adminMe.status(), 200);
        const administrator = await adminMe.json();
        assert.equal(administrator.id, "dev");
        assert.deepEqual(administrator.roles, ["platform-admins"]);
        assert.equal(administrator.role_ids.includes("engineering"), false);
        member = await browser.newContext(capture || publishedCapture
            ? { viewport: { width: 1400, height: 950 }, deviceScaleFactor: 2, colorScheme: "dark", locale: "de-DE" }
            : { viewport: { width: 1280, height: 900 }, locale: "de-DE" });
        await member.addCookies([{ name: "id", value: memberCookie, url: BASE }, { name: "lang", value: "en", url: BASE }]);
        const groupsResponse = await admin.request.get(`${BASE}/api/v0/admin/groups`);
        assert.equal(groupsResponse.status(), 200);
        originalGroup = (await groupsResponse.json()).groups.find((group) => group.name === "engineering");
        assert.ok(originalGroup, "use the actual fixture Engineering group");
        const permission = await admin.request.put(`${BASE}/api/v0/admin/groups`, {
            data: { ...originalGroup, can_manage_agents: true, can_manage_personal_agents: false },
        });
        assert.equal(permission.status(), 200);
        const me = await member.request.get(`${BASE}/api/v0/me`);
        assert.equal(me.status(), 200);
        const identity = await me.json();
        assert.equal(identity.can_manage_agents, true);
        assert.equal(identity.can_manage_personal_agents, false);
        const createdOrganization = await admin.request.post(`${BASE}/api/v0/admin/organizations`, {
            data: { name: `Private Draft ${Date.now()}`, description: "Private draft acceptance.", member_groups: [originalGroup.name] },
        });
        assert.equal(createdOrganization.status(), 201);
        organization = (await createdOrganization.json()).organization;
        const orgUrl = `${BASE}/api/v0/organizations/${organization.id}`;
        assert.equal((await admin.request.get(orgUrl)).status(), 404);
        const createdAgent = await member.request.post(`${orgUrl}/agents`, {
            data: { name: "Harald", display: "Harald Draft", description: "Private saved-draft acceptance." },
        });
        assert.equal(createdAgent.status(), 201);
        const agent = (await createdAgent.json()).agent;
        agentId = agent.id;
        assert.equal(agent.access, "write");
        assert.equal(agent.ownership.active, true);
        const resourcesResponse = await member.request.get(`${BASE}/api/v0/agent-resources?agent_id=${encodeURIComponent(agentId)}`);
        assert.equal(resourcesResponse.status(), 200);
        const resources = await resourcesResponse.json();
        const model = resources.models.chat.find((entry) => entry.id === "demo-model");
        assert.ok(model, "use the actual fixture model from the target inventory");
        const granted = await member.request.post(`${BASE}/api/v0/system-principals/${agentId}/grants`, {
            data: { kind: "model", ref: model.id },
        });
        assert.equal(granted.status(), 201);
        const saved = await member.request.put(`${BASE}/api/v0/agents/${agentId}/draft`, {
            data: { spec: { main: { model: model.id, instructions: { orchestration: "Answer the explicit private draft request." }, tools: [] } } },
        });
        assert.equal(saved.status(), 200);
        const page = await member.newPage();
        await page.goto(`${BASE}/agents/${agentId}?tab=try`, { waitUntil: "networkidle" });
        await page.getByRole("tab", { name: "Try it", exact: true }).waitFor();
        assert.equal(await page.getByRole("tab", { name: "Tests", exact: true }).count(), 0);
        const chat = page.locator("[data-test-chat]");
        const composer = chat.getByRole("textbox", { name: "Your message", exact: true });
        await composer.fill("Organization private draft probe");
        const startedResponse = page.waitForResponse((response) => response.request().method() === "POST"
            && response.url() === `${BASE}/api/v0/agents/${agentId}/test/messages`);
        await composer.press("Enter");
        const started = await startedResponse;
        assert.equal(started.status(), 202);
        const turn = await started.json();
        assert.equal(turn.source, 'draft');
        assert.equal(turn.agent_version, 0);
        assert.ok(turn.session_id);
        assert.ok(turn.turn_id);
        await chat.getByText("Hi! How can I help?", { exact: true }).waitFor({ timeout: 15000 });
        const debugUrl = `${BASE}/api/v0/agents/${agentId}/test/${turn.session_id}/turns/${turn.turn_id}/debug`;
        const historyUrl = `${BASE}/api/v0/agents/${agentId}/test/${turn.session_id}/events`;
        let debug;
        for (let attempt = 0; attempt < 60; attempt++) {
            debug = await member.request.get(debugUrl, { timeout: 5000 });
            if (debug.status() !== 409) break;
            await page.waitForTimeout(250);
        }
        assert.equal(debug.status(), 200);
        const inspected = await debug.json();
        assert.equal(inspected.turn_id, turn.turn_id);
        assert.equal(inspected.source, 'draft');
        assert.equal(inspected.agent_version, 0);
        assert.deepEqual(inspected.debug.tool_calls, []);
        if (capture) {
            await page.evaluate(async () => {
                document.documentElement.setAttribute("data-theme", "dark");
                await document.fonts.ready;
            });
            await chat.screenshot({ path: capture, animations: "disabled" });
            const size = await chat.boundingBox();
            assert.ok(size);
            const entry = {
                file: capture, route: `/agents/${agentId}?tab=try`, selector: "[data-test-chat]",
                viewport: { width: 1400, height: 950 }, scale: 2,
                width: Math.round(size.width * 2), height: Math.round(size.height * 2),
                data: "Synthetic dev-ui fixture; actual organization member Writer without personal agent management. Harald Draft's saved draft, real private TestChat answer and settled debug. Captured by spa-agents-organization-draft.test.mjs with DOCS_ORG_AGENT_DRAFT_CAPTURE.",
            };
            fs.writeFileSync("/tmp/agent-organization-draft-capture-entry.json", `${JSON.stringify(entry, null, 2)}\n`);
        }
        const personalHistory = await member.request.get(`${BASE}/api/v0/chat/sessions/${turn.session_id}`);
        assert.equal(personalHistory.status(), 404);
        const outsiderDebug = await admin.request.get(debugUrl);
        assert.equal(outsiderDebug.status(), 404);
        const outsiderHistory = await admin.request.get(historyUrl, { timeout: 5000 });
        assert.equal(outsiderHistory.status(), 404);
        const outsiderStart = await admin.request.post(`${BASE}/api/v0/agents/${agentId}/test/messages`, {
            data: { message: "Outsider draft request", source: 'draft' },
        });
        assert.equal(outsiderStart.status(), 403);
        const recovery = await admin.request.get(`${BASE}/api/v0/agents/${agentId}`);
        assert.equal(recovery.status(), 200);
        const published = await member.request.post(`${BASE}/api/v0/agents/${agentId}/publish`);
        assert.equal(published.status(), 201);
        const publishedAgent = await member.request.get(`${BASE}/api/v0/agents/${agentId}`);
        assert.equal(publishedAgent.status(), 200);
        const liveVersion = (await publishedAgent.json()).agent.live_version;
        assert.ok(liveVersion > 0);
        await page.reload({ waitUntil: "networkidle" });
        const source = chat.getByRole("combobox", { name: "Conversation source", exact: true });
        assert.equal(await source.inputValue(), "draft");
        await source.selectOption("published");
        await page.waitForURL((url) => !url.searchParams.has("test_session"));
        assert.equal(await source.inputValue(), "published");
        await composer.fill("Organization published private probe");
        const heldPost = new Promise((resolve) => { releasePost = resolve; });
        const messagesUrl = `${BASE}/api/v0/agents/${agentId}/test/messages`;
        await page.route(messagesUrl, async (route) => {
            if (route.request().method() === "POST") await heldPost;
            await route.continue();
        });
        const requested = page.waitForRequest((request) => request.method() === "POST" && request.url() === messagesUrl);
        const publishedStarted = page.waitForResponse((response) => response.request().method() === "POST"
            && response.url() === `${BASE}/api/v0/agents/${agentId}/test/messages`);
        await composer.press("Enter");
        await requested;
        assert.equal(await source.isDisabled(), true);
        assert.equal(await chat.getByRole("button", { name: "New conversation", exact: true }).isDisabled(), true);
        releasePost();
        releasePost = null;
        const publishedResponse = await publishedStarted;
        await page.unroute(messagesUrl);
        assert.equal(publishedResponse.status(), 202);
        assert.equal(publishedResponse.request().postDataJSON().source, "published");
        const publishedTurn = await publishedResponse.json();
        assert.equal(publishedTurn.source, "published");
        assert.equal(publishedTurn.agent_version, liveVersion);
        await chat.getByText(`This private conversation uses published version ${liveVersion}.`, { exact: true }).waitFor();
        await chat.getByText("Hi! How can I help?", { exact: true }).waitFor({ timeout: 15000 });
        const publishedDebugUrl = `${BASE}/api/v0/agents/${agentId}/test/${publishedTurn.session_id}/turns/${publishedTurn.turn_id}/debug`;
        let publishedDebug;
        for (let attempt = 0; attempt < 60; attempt++) {
            publishedDebug = await member.request.get(publishedDebugUrl, { timeout: 5000 });
            if (publishedDebug.status() !== 409) break;
            await page.waitForTimeout(250);
        }
        assert.equal(publishedDebug.status(), 200);
        const publishedView = await publishedDebug.json();
        assert.equal(publishedView.source, "published");
        assert.equal(publishedView.agent_version, liveVersion);
        assert.equal(publishedView.turn_id, publishedTurn.turn_id);
        if (publishedCapture) {
            await page.evaluate(async () => {
                document.documentElement.setAttribute("data-theme", "dark");
                await document.fonts.ready;
            });
            await chat.screenshot({ path: publishedCapture, animations: "disabled" });
            const size = await chat.boundingBox();
            assert.ok(size);
            const entry = {
                file: publishedCapture, route: `/agents/${agentId}?tab=try`, selector: "[data-test-chat]",
                viewport: { width: 1400, height: 950 }, scale: 2,
                width: Math.round(size.width * 2), height: Math.round(size.height * 2),
                data: `Synthetic dev-ui fixture; actual Engineering organization Writer without personal agent management. Harald Draft's private conversation uses actual published version ${liveVersion}, with a real answer and settled source-aware debug. Captured by spa-agents-organization-draft.test.mjs with DOCS_ORG_AGENT_PUBLISHED_CAPTURE.`,
            };
            fs.writeFileSync("/tmp/agent-organization-published-capture-entry.json", `${JSON.stringify(entry, null, 2)}\n`);
        }
        await page.setViewportSize({ width: 390, height: 844 });
        assert.equal(await source.inputValue(), "published");
        await composer.focus();
        assert.equal(await composer.evaluate((element) => element === document.activeElement), true);
        assert.equal(await source.evaluate((element) => element.getBoundingClientRect().width <= window.innerWidth), true);
        assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true);
        await page.setViewportSize({ width: capture || publishedCapture ? 1400 : 1280, height: capture || publishedCapture ? 950 : 900 });
        const edited = await member.request.put(`${BASE}/api/v0/agents/${agentId}/draft`, {
            data: { spec: { main: { model: model.id, instructions: { orchestration: "A newer published task must not replace the open conversation." }, tools: [] } } },
        });
        assert.equal(edited.status(), 200);
        assert.equal((await member.request.post(`${BASE}/api/v0/agents/${agentId}/publish`)).status(), 201);
        const latest = await member.request.get(`${BASE}/api/v0/agents/${agentId}`);
        assert.equal(latest.status(), 200);
        assert.ok((await latest.json()).agent.live_version > liveVersion);
        await composer.fill("Continue the original published private conversation");
        const continuedResponse = page.waitForResponse((response) => response.request().method() === "POST"
            && response.url() === `${BASE}/api/v0/agents/${agentId}/test/messages`);
        await composer.press("Enter");
        const continued = await continuedResponse;
        assert.equal(continued.status(), 202);
        const continuedTurn = await continued.json();
        assert.equal(continuedTurn.session_id, publishedTurn.session_id);
        assert.equal(continuedTurn.source, "published");
        assert.equal(continuedTurn.agent_version, liveVersion);
        await chat.getByText(`This private conversation uses published version ${liveVersion}.`, { exact: true }).waitFor();
        for (let attempt = 0; attempt < 60; attempt++) {
            if (await chat.getByText("Hi! How can I help?", { exact: true }).count() === 2) break;
            await page.waitForTimeout(250);
        }
        assert.equal(await chat.getByText("Hi! How can I help?", { exact: true }).count(), 2);
        await chat.getByRole("button", { name: "New conversation", exact: true }).click();
        assert.equal(await source.inputValue(), "draft");
        assert.equal(await chat.getByText(`This private conversation uses published version ${liveVersion}.`, { exact: true }).count(), 0);
        const abandonedPost = new Promise((resolve) => { releasePost = resolve; });
        await page.route(messagesUrl, async (route) => {
            if (route.request().method() === "POST") await abandonedPost;
            await route.continue();
        });
        await composer.fill("A reply from the previous chat must not reattach after navigation");
        const abandonedRequest = page.waitForRequest((request) => request.method() === "POST" && request.url() === messagesUrl);
        const abandonedResponse = page.waitForResponse((response) => response.request().method() === "POST" && response.url() === messagesUrl);
        await composer.press("Enter");
        await abandonedRequest;
        await page.getByRole("tab", { name: "Setup", exact: true }).click();
        await page.waitForURL((url) => url.searchParams.get("tab") === "setup");
        await page.locator("[data-test-chat]").waitFor({ state: "detached" });
        assert.equal(await page.locator("[data-test-chat]").count(), 0);
        await page.getByRole("tab", { name: "Try it", exact: true }).click();
        await composer.waitFor();
        releasePost();
        releasePost = null;
        const detachedResponse = await abandonedResponse;
        assert.equal(detachedResponse.status(), 202);
        const detached = await detachedResponse.json();
        await page.unroute(messagesUrl);
        let detachedDebug;
        for (let attempt = 0; attempt < 60; attempt++) {
            detachedDebug = await member.request.get(`${BASE}/api/v0/agents/${agentId}/test/${detached.session_id}/turns/${detached.turn_id}/debug`, { timeout: 5000 });
            if (detachedDebug.status() !== 409) break;
            await page.waitForTimeout(250);
        }
        assert.equal(detachedDebug.status(), 200);
        assert.equal(new URL(page.url()).searchParams.has("test_session"), false);
        assert.equal(await chat.getByText("A reply from the previous chat must not reattach after navigation", { exact: true }).count(), 0);
        assert.equal(await source.inputValue(), "draft");
        await page.setViewportSize({ width: 390, height: 844 });
        await composer.focus();
        assert.equal(await composer.evaluate((element) => element === document.activeElement), true);
        assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true);
        const inactive = await member.request.post(`${orgUrl}/agents/${agentId}/active`, { data: { active: false } });
        assert.equal(inactive.status(), 200);
        const inactiveStart = await member.request.post(`${BASE}/api/v0/agents/${agentId}/test/messages`, {
            data: { message: "Inactive draft request", session_id: turn.session_id, source: 'draft' },
        });
        assert.equal(inactiveStart.status(), 403);
        const activated = await member.request.post(`${orgUrl}/agents/${agentId}/active`, { data: { active: true } });
        assert.equal(activated.status(), 200);
        const revoked = await admin.request.put(`${BASE}/api/v0/admin/organizations/${organization.id}`, {
            data: { name: organization.name, description: organization.description, member_groups: [], expected_revision: organization.revision },
        });
        assert.equal(revoked.status(), 200);
        organization = (await revoked.json()).organization;
        const revokedStart = await member.request.post(`${BASE}/api/v0/agents/${agentId}/test/messages`, {
            data: { message: "Revoked draft request", session_id: turn.session_id, source: 'draft' },
        });
        assert.equal(revokedStart.status(), 404);
        assert.equal((await member.request.get(debugUrl)).status(), 404);
        assert.equal((await member.request.get(historyUrl, { timeout: 5000 })).status(), 404);
        await page.reload({ waitUntil: "networkidle" });
        assert.equal(await page.getByRole("tab", { name: "Try it", exact: true }).count(), 0);
        assert.equal(await page.locator("[data-test-chat]").count(), 0);
    } finally {
        releasePost?.();
        try {
            if (admin) {
                try {
                    if (organization && originalGroup) {
                        const restoredMembership = await admin.request.put(`${BASE}/api/v0/admin/organizations/${organization.id}`, {
                            data: { name: organization.name, description: organization.description, member_groups: [originalGroup.name], expected_revision: organization.revision }, timeout: 5000,
                        });
                        if (restoredMembership.status() === 200) organization = (await restoredMembership.json()).organization;
                    }
                    if (member && agentId) await member.request.delete(`${BASE}/api/v0/agents/${agentId}`, { timeout: 5000 });
                    if (organization) await admin.request.put(`${BASE}/api/v0/admin/organizations/${organization.id}`, {
                        data: { name: organization.name, description: organization.description, member_groups: [], expected_revision: organization.revision }, timeout: 5000,
                    });
                } finally {
                    if (originalGroup) await admin.request.put(`${BASE}/api/v0/admin/groups`, { data: originalGroup, timeout: 5000 });
                }
            }
        } finally {
            try {
                if (admin) await admin.request.post(`${BASE}/api/v0/admin/impersonate/stop`, { timeout: 5000 });
            } finally {
                try { await member?.close(); }
                finally { try { await admin?.close(); } finally { await browser.close(); } }
            }
        }
    }
});
