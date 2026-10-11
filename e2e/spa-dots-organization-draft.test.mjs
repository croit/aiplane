import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import { BASE, devSessionCookie, gatewayIsUp, launchBrowser } from "./helpers.mjs";

test("An organization Writer uses private saved-draft chat without personal permission and loses private access on revocation", async () => {
    assert.ok(await gatewayIsUp(), `gateway is not reachable at ${BASE}`);
    const memberCookie = process.env.AIPLANE_DOT_MEMBER_SESSION_COOKIE?.trim().replace(/^id=/, "");
    assert.ok(memberCookie, "set AIPLANE_DOT_MEMBER_SESSION_COOKIE to the seeded Engineering member session");
    const browser = await launchBrowser();
    const capture = process.env.DOCS_ORG_DOT_DRAFT_CAPTURE;
    let admin;
    let member;
    let originalGroup;
    let organization;
    let dotId;
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
        member = await browser.newContext(capture
            ? { viewport: { width: 1400, height: 950 }, deviceScaleFactor: 2, colorScheme: "dark", locale: "de-DE" }
            : { viewport: { width: 1280, height: 900 }, locale: "de-DE" });
        await member.addCookies([{ name: "id", value: memberCookie, url: BASE }, { name: "lang", value: "en", url: BASE }]);
        const groupsResponse = await admin.request.get(`${BASE}/api/v0/admin/groups`);
        assert.equal(groupsResponse.status(), 200);
        originalGroup = (await groupsResponse.json()).groups.find((group) => group.name === "engineering");
        assert.ok(originalGroup, "use the actual fixture Engineering group");
        const permission = await admin.request.put(`${BASE}/api/v0/admin/groups`, {
            data: { ...originalGroup, can_manage_agents: true, can_manage_dots: false },
        });
        assert.equal(permission.status(), 200);
        const me = await member.request.get(`${BASE}/api/v0/me`);
        assert.equal(me.status(), 200);
        const identity = await me.json();
        assert.equal(identity.can_manage_agents, true);
        assert.equal(identity.can_manage_dots, false);
        const createdOrganization = await admin.request.post(`${BASE}/api/v0/admin/organizations`, {
            data: { name: `Private Draft ${Date.now()}`, description: "Private draft acceptance.", member_groups: [originalGroup.name] },
        });
        assert.equal(createdOrganization.status(), 201);
        organization = (await createdOrganization.json()).organization;
        const orgUrl = `${BASE}/api/v0/organizations/${organization.id}`;
        assert.equal((await admin.request.get(orgUrl)).status(), 404);
        const createdDot = await member.request.post(`${orgUrl}/dots`, {
            data: { name: "Harald", display: "Harald Draft", description: "Private saved-draft acceptance." },
        });
        assert.equal(createdDot.status(), 201);
        const dot = (await createdDot.json()).agent;
        dotId = dot.id;
        assert.equal(dot.access, "write");
        assert.equal(dot.dot.active, true);
        const resourcesResponse = await member.request.get(`${BASE}/api/v0/agent-resources?agent_id=${encodeURIComponent(dotId)}`);
        assert.equal(resourcesResponse.status(), 200);
        const resources = await resourcesResponse.json();
        const model = resources.models.chat.find((entry) => entry.id === "demo-model");
        assert.ok(model, "use the actual fixture model from the target inventory");
        const granted = await member.request.post(`${BASE}/api/v0/system-principals/${dotId}/grants`, {
            data: { kind: "model", ref: model.id },
        });
        assert.equal(granted.status(), 201);
        const saved = await member.request.put(`${BASE}/api/v0/agents/${dotId}/draft`, {
            data: { spec: { main: { model: model.id, instructions: { orchestration: "Answer the explicit private draft request." }, tools: [] } } },
        });
        assert.equal(saved.status(), 200);
        const page = await member.newPage();
        await page.goto(`${BASE}/agents/${dotId}?tab=try`, { waitUntil: "networkidle" });
        await page.getByRole("tab", { name: "Try it", exact: true }).waitFor();
        assert.equal(await page.getByRole("tab", { name: "Tests", exact: true }).count(), 0);
        const chat = page.locator("[data-test-chat]");
        const composer = chat.getByRole("textbox", { name: "Message as a visitor…", exact: true });
        await composer.fill("Organization private draft probe");
        const startedResponse = page.waitForResponse((response) => response.request().method() === "POST"
            && response.url() === `${BASE}/api/v0/agents/${dotId}/test/messages`);
        await composer.press("Enter");
        const started = await startedResponse;
        assert.equal(started.status(), 202);
        const turn = await started.json();
        assert.equal(turn.draft_version, 0);
        assert.ok(turn.session_id);
        assert.ok(turn.turn_id);
        await chat.getByText("Hi! How can I help?", { exact: true }).waitFor({ timeout: 15000 });
        const debugUrl = `${BASE}/api/v0/agents/${dotId}/test/${turn.session_id}/turns/${turn.turn_id}/debug`;
        const historyUrl = `${BASE}/api/v0/agents/${dotId}/test/${turn.session_id}/events`;
        let debug;
        for (let attempt = 0; attempt < 60; attempt++) {
            debug = await member.request.get(debugUrl, { timeout: 5000 });
            if (debug.status() !== 409) break;
            await page.waitForTimeout(250);
        }
        assert.equal(debug.status(), 200);
        const inspected = await debug.json();
        assert.equal(inspected.turn_id, turn.turn_id);
        assert.equal(inspected.draft_version, 0);
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
                file: capture, route: `/agents/${dotId}?tab=try`, selector: "[data-test-chat]",
                viewport: { width: 1400, height: 950 }, scale: 2,
                width: Math.round(size.width * 2), height: Math.round(size.height * 2),
                data: "Synthetic dev-ui fixture; actual organization member Writer without personal Dot management. Harald Draft's saved draft, real private TestChat answer and settled debug. Captured by spa-dots-organization-draft.test.mjs with DOCS_ORG_DOT_DRAFT_CAPTURE.",
            };
            fs.writeFileSync("/tmp/dot-organization-draft-capture-entry.json", `${JSON.stringify(entry, null, 2)}\n`);
        }
        const personalHistory = await member.request.get(`${BASE}/api/v0/chat/sessions/${turn.session_id}`);
        assert.equal(personalHistory.status(), 404);
        const outsiderDebug = await admin.request.get(debugUrl);
        assert.equal(outsiderDebug.status(), 404);
        const outsiderHistory = await admin.request.get(historyUrl, { timeout: 5000 });
        assert.equal(outsiderHistory.status(), 404);
        const outsiderStart = await admin.request.post(`${BASE}/api/v0/agents/${dotId}/test/messages`, {
            data: { message: "Outsider draft request" },
        });
        assert.equal(outsiderStart.status(), 403);
        const recovery = await admin.request.get(`${BASE}/api/v0/agents/${dotId}`);
        assert.equal(recovery.status(), 200);
        await page.setViewportSize({ width: 390, height: 844 });
        await composer.focus();
        assert.equal(await composer.evaluate((element) => element === document.activeElement), true);
        assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true);
        const inactive = await member.request.post(`${orgUrl}/dots/${dotId}/active`, { data: { active: false } });
        assert.equal(inactive.status(), 200);
        const inactiveStart = await member.request.post(`${BASE}/api/v0/agents/${dotId}/test/messages`, {
            data: { message: "Inactive draft request", session_id: turn.session_id },
        });
        assert.equal(inactiveStart.status(), 403);
        const activated = await member.request.post(`${orgUrl}/dots/${dotId}/active`, { data: { active: true } });
        assert.equal(activated.status(), 200);
        const revoked = await admin.request.put(`${BASE}/api/v0/admin/organizations/${organization.id}`, {
            data: { name: organization.name, description: organization.description, member_groups: [], expected_revision: organization.revision },
        });
        assert.equal(revoked.status(), 200);
        organization = (await revoked.json()).organization;
        const revokedStart = await member.request.post(`${BASE}/api/v0/agents/${dotId}/test/messages`, {
            data: { message: "Revoked draft request", session_id: turn.session_id },
        });
        assert.equal(revokedStart.status(), 404);
        assert.equal((await member.request.get(debugUrl)).status(), 404);
        assert.equal((await member.request.get(historyUrl, { timeout: 5000 })).status(), 404);
        await page.reload({ waitUntil: "networkidle" });
        assert.equal(await page.getByRole("tab", { name: "Try it", exact: true }).count(), 0);
        assert.equal(await page.locator("[data-test-chat]").count(), 0);
    } finally {
        try {
            if (admin) {
                try {
                    if (dotId) await admin.request.delete(`${BASE}/api/v0/agents/${dotId}`, { timeout: 5000 });
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
