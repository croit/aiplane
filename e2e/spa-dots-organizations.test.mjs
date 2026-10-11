import { test } from "node:test";
import assert from "node:assert/strict";
import { BASE, chooseSearchable, devSessionCookie, gatewayIsUp, launchBrowser } from "./helpers.mjs";

test("Organization membership and Dot configuration reuse the existing editor with private source-aware chat", async () => {
    assert.ok(await gatewayIsUp(), `gateway is not reachable at ${BASE}`);
    const browser = await launchBrowser();
    let admin;
    let member;
    let organization;
    let dotId;
    try {
        admin = await browser.newContext({ viewport: { width: 1280, height: 900 }, locale: "de-DE" });
        await admin.addCookies([
            { name: "id", value: await devSessionCookie(), url: BASE },
            { name: "lang", value: "en", url: BASE },
        ]);
        const groupsResponse = await admin.request.get(`${BASE}/api/v0/admin/groups`);
        assert.equal(groupsResponse.status(), 200);
        const groups = (await groupsResponse.json()).groups;
        const adminGroup = groups.find((group) => group.is_admin);
        const engineering = groups.find((group) => group.name === "engineering");
        assert.ok(adminGroup, "use the actual fixture administrator group");
        assert.ok(engineering, "use the actual fixture Engineering group");
        const name = `Organization Support ${Date.now()}`;
        const page = await admin.newPage();
        await page.goto(`${BASE}/admin/organizations`, { waitUntil: "networkidle" });
        await page.getByRole("button", { name: "Create organization", exact: true }).click();
        const creation = page.locator("dialog[open]");
        await creation.waitFor();
        await creation.getByRole("textbox", { name: "Name", exact: true }).fill(name);
        await creation.getByRole("textbox", { name: "Description", exact: true }).fill("Support configuration maintained by the actual member groups.");
        const picker = creation.getByRole("combobox", { name: "Member groups", exact: true });
        await picker.click();
        await page.getByRole("option", { name: adminGroup.name, exact: true }).click();
        await page.getByRole("option", { name: engineering.name, exact: true }).click();
        await page.keyboard.press("Escape");
        const created = page.waitForResponse((response) => response.request().method() === "POST"
            && response.url() === `${BASE}/api/v0/admin/organizations`);
        await creation.getByRole("button", { name: "Create organization", exact: true }).click();
        const creationResponse = await created;
        assert.equal(creationResponse.status(), 201);
        organization = (await creationResponse.json()).organization;
        assert.deepEqual([...organization.member_groups].sort(), [adminGroup.name, engineering.name].sort());
        const card = page.locator("section").filter({ has: page.getByRole("heading", { name, exact: true }) });
        await card.getByRole("button", { name: "Save", exact: true }).waitFor();
        assert.equal(await card.getByRole("button", { name: "Save", exact: true }).isDisabled(), true);
        const externalName = `${name} reviewed`;
        const changed = await admin.request.put(`${BASE}/api/v0/admin/organizations/${organization.id}`, {
            data: { name: externalName, description: organization.description, member_groups: organization.member_groups, expected_revision: organization.revision },
        });
        assert.equal(changed.status(), 200);
        organization = (await changed.json()).organization;
        await card.getByRole("textbox", { name: "Description", exact: true }).fill("Unsaved member configuration draft.");
        const stale = page.waitForResponse((response) => response.request().method() === "PUT"
            && response.url() === `${BASE}/api/v0/admin/organizations/${organization.id}`);
        await card.getByRole("button", { name: "Save", exact: true }).click();
        assert.equal((await stale).status(), 409);
        await card.getByRole("alert").getByText("The organization changed. Reload its current settings before saving again.", { exact: true }).waitFor();
        assert.equal(await card.getByRole("textbox", { name: "Description", exact: true }).inputValue(), "Unsaved member configuration draft.");
        await page.getByRole("button", { name: "Reload organization", exact: true }).click();
        const refreshedCard = page.locator("section").filter({ has: page.getByRole("heading", { name: externalName, exact: true }) });
        await refreshedCard.getByRole("textbox", { name: "Description", exact: true }).waitFor();
        assert.equal(await refreshedCard.getByRole("textbox", { name: "Description", exact: true }).inputValue(), organization.description);

        await page.goto(`${BASE}/organizations`, { waitUntil: "networkidle" });
        await page.locator(`aside a[href="/organizations"]`).waitFor();
        await page.getByRole("link", { name: externalName, exact: true }).click();
        await page.getByRole("heading", { name: externalName, exact: true }).waitFor();
        await page.getByRole("button", { name: "Create Dot", exact: true }).click();
        const createDot = page.locator("dialog[open]");
        await createDot.waitFor();
        await createDot.getByRole("textbox", { name: "Name", exact: true }).fill("Harald");
        await createDot.getByRole("textbox", { name: "Display name", exact: true }).fill("Harald Support");
        await createDot.getByRole("textbox", { name: "Description", exact: true }).fill("Support configuration for this organization.");
        const createdDot = page.waitForResponse((response) => response.request().method() === "POST"
            && response.url() === `${BASE}/api/v0/organizations/${organization.id}/dots`);
        await createDot.getByRole("button", { name: "Create", exact: true }).click();
        const dotResponse = await createdDot;
        assert.equal(dotResponse.status(), 201);
        const dot = (await dotResponse.json()).agent;
        dotId = dot.id;
        assert.equal(dot.dot.name, "Harald");
        assert.deepEqual(dot.dot.owner, { kind: "organization", id: organization.id, name: externalName });
        assert.notEqual(dot.name, "Harald");
        await page.waitForURL((url) => url.pathname === `/agents/${dotId}/setup/start`);
        await page.getByText(`Organization: ${externalName} Members with Write access can configure this organization Dot, use its saved draft or a published version in a private conversation, and schedule work on a selected published version. Automated test suites, delegation, organization webhooks and external channels are unavailable.`, { exact: true }).waitFor();
        assert.equal(await page.getByRole("button", { name: "Website", exact: true }).count(), 0);
        const personal = await admin.request.get(`${BASE}/api/v0/dots/${dotId}/access`);
        assert.equal(personal.status(), 404);

        const resourceResponse = await admin.request.get(`${BASE}/api/v0/agent-resources?agent_id=${encodeURIComponent(dotId)}`);
        assert.equal(resourceResponse.status(), 200);
        const resources = await resourceResponse.json();
        const model = resources.models.chat.find((item) => item.id === "demo-model");
        assert.ok(model, "use the actual fixture model from the existing target-scoped catalog");
        await page.goto(`${BASE}/agents/${dotId}/setup/basics`, { waitUntil: "networkidle" });
        await page.getByRole("textbox", { name: /What should the agent do\?/ }).fill("Review support requests and summarize the next steps.");
        await chooseSearchable(page, page.getByRole("combobox", { name: "Model", exact: true }), model.id);
        const savedDraft = page.waitForResponse((response) => response.request().method() === "PUT"
            && response.url() === `${BASE}/api/v0/agents/${dotId}/draft`);
        await page.getByRole("button", { name: "Save draft", exact: true }).click();
        assert.equal((await savedDraft).status(), 200);
        const published = page.waitForResponse((response) => response.request().method() === "POST"
            && response.url() === `${BASE}/api/v0/agents/${dotId}/publish`);
        await page.getByRole("button", { name: "Publish", exact: true }).click();
        assert.equal((await published).status(), 201);

        await page.goto(`${BASE}/agents/${dotId}?tab=try`, { waitUntil: "networkidle" });
        await page.getByRole("tab", { name: "Setup", exact: true }).waitFor();
        await page.getByRole("tab", { name: "Try it", exact: true }).waitFor();
        await page.getByRole("textbox", { name: "Your message", exact: true }).waitFor();
        assert.equal(await page.getByRole("tab", { name: "Tests", exact: true }).count(), 0);
        assert.equal(await page.getByRole("heading", { name: "Website", exact: true }).count(), 0);
        assert.equal(await page.getByText(dot.name, { exact: true }).count(), 0);
        await page.goto(`${BASE}/agents/${dotId}?tab=settings&sub=sharing`, { waitUntil: "networkidle" });
        await page.getByRole("tab", { name: "Sharing", exact: true }).waitFor();
        assert.equal(await page.getByRole("heading", { name: "Notification channels", exact: true }).count(), 0);
        assert.equal(await page.getByRole("heading", { name: "Embed keys", exact: true }).count(), 0);
        await page.goto(`${BASE}/agents/${dotId}?tab=setup&view=advanced&sub=edit`, { waitUntil: "networkidle" });
        const settings = page.getByRole("checkbox", { name: "Settings", exact: true });
        await settings.check();
        assert.equal(await page.getByRole("textbox", { name: "Accent colour", exact: true }).count(), 0);
        assert.equal(await page.getByRole("textbox", { name: "Allowed origins (one per line)", exact: true }).count(), 0);
        assert.equal(await page.getByRole("checkbox", { name: "Require passing tests to publish", exact: true }).count(), 0);

        await page.goto(`${BASE}/organizations/${organization.id}`, { waitUntil: "networkidle" });
        const dotCard = page.locator("section").filter({ has: page.getByRole("heading", { name: "Harald Support", exact: true }) });
        const inactive = page.waitForResponse((response) => response.request().method() === "POST"
            && response.url() === `${BASE}/api/v0/organizations/${organization.id}/dots/${dotId}/active`);
        await dotCard.getByRole("button", { name: "Deactivate", exact: true }).click();
        assert.equal((await inactive).status(), 200);
        await dotCard.getByText("Inactive", { exact: true }).waitFor();
        const detail = await admin.request.get(`${BASE}/api/v0/organizations/${organization.id}`);
        assert.equal((await detail.json()).active_count, 0);
        await page.setViewportSize({ width: 390, height: 844 });
        assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true);

        const memberCookie = process.env.AIPLANE_DOT_MEMBER_SESSION_COOKIE?.trim().replace(/^id=/, "");
        if (memberCookie) {
            member = await browser.newContext({ viewport: { width: 1280, height: 900 }, locale: "de-DE" });
            await member.addCookies([{ name: "id", value: memberCookie, url: BASE }, { name: "lang", value: "en", url: BASE }]);
            const memberPage = await member.newPage();
            await memberPage.goto(`${BASE}/organizations`, { waitUntil: "networkidle" });
            await memberPage.locator(`aside a[href="/organizations"]`).waitFor();
            await memberPage.getByRole("link", { name: externalName, exact: true }).click();
            await memberPage.getByText("No Dots are shared with you in this organization.", { exact: true }).waitFor();
            assert.equal(await memberPage.getByRole("heading", { name: "Harald Support", exact: true }).count(), 0);
            await memberPage.setViewportSize({ width: 390, height: 844 });
            assert.equal(await memberPage.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true);
        }
    } finally {
        try {
            if (admin && dotId) await admin.request.delete(`${BASE}/api/v0/agents/${dotId}`, { timeout: 5000 });
            if (admin && organization) await admin.request.put(`${BASE}/api/v0/admin/organizations/${organization.id}`, {
                data: { name: organization.name, description: organization.description, member_groups: [], expected_revision: organization.revision }, timeout: 5000,
            });
        } finally {
            try { await member?.close(); }
            finally { try { await admin?.close(); } finally { await browser.close(); } }
        }
    }
});

test("An organization member manages four active Dots without personal Dot permission and loses access when membership is removed", async () => {
    assert.ok(await gatewayIsUp(), `gateway is not reachable at ${BASE}`);
    const memberCookie = process.env.AIPLANE_DOT_MEMBER_SESSION_COOKIE?.trim().replace(/^id=/, "");
    assert.ok(memberCookie, "set AIPLANE_DOT_MEMBER_SESSION_COOKIE to the seeded Engineering member session");
    const browser = await launchBrowser();
    let admin;
    let member;
    let originalGroup;
    let organization;
    const dotIds = [];
    try {
        admin = await browser.newContext({ viewport: { width: 1280, height: 900 }, locale: "de-DE" });
        await admin.addCookies([{ name: "id", value: await devSessionCookie(), url: BASE }, { name: "lang", value: "en", url: BASE }]);
        member = await browser.newContext({ viewport: { width: 1280, height: 900 }, locale: "de-DE" });
        await member.addCookies([{ name: "id", value: memberCookie, url: BASE }, { name: "lang", value: "en", url: BASE }]);
        const groupsResponse = await admin.request.get(`${BASE}/api/v0/admin/groups`);
        assert.equal(groupsResponse.status(), 200);
        originalGroup = (await groupsResponse.json()).groups.find((group) => group.name === "engineering");
        assert.ok(originalGroup, "use the actual fixture Engineering group");
        const grant = await admin.request.put(`${BASE}/api/v0/admin/groups`, {
            data: { ...originalGroup, can_manage_agents: true, can_manage_dots: false },
        });
        assert.equal(grant.status(), 200);
        const meResponse = await member.request.get(`${BASE}/api/v0/me`);
        assert.equal(meResponse.status(), 200);
        const me = await meResponse.json();
        assert.equal(me.id, "eng");
        assert.equal(me.can_manage_agents, true);
        assert.equal(me.can_manage_dots, false);
        const orgCreated = await admin.request.post(`${BASE}/api/v0/admin/organizations`, {
            data: { name: `Member Support ${Date.now()}`, description: "Engineering-owned support configuration.", member_groups: [originalGroup.name] },
        });
        assert.equal(orgCreated.status(), 201);
        organization = (await orgCreated.json()).organization;
        const orgUrl = `${BASE}/api/v0/organizations/${organization.id}`;
        const page = await member.newPage();
        await page.goto(`${BASE}/organizations`, { waitUntil: "networkidle" });
        await page.locator('aside a[href="/organizations"]').waitFor();
        assert.equal(await page.locator('aside a[href="/dots"]').count(), 0);
        await page.getByRole("link", { name: organization.name, exact: true }).click();
        await page.getByRole("button", { name: "Create Dot", exact: true }).click();
        const creation = page.locator("dialog[open]");
        await creation.waitFor();
        await creation.getByRole("textbox", { name: "Name", exact: true }).fill("Harald");
        await creation.getByRole("textbox", { name: "Display name", exact: true }).fill("Harald Engineering");
        const created = page.waitForResponse((response) => response.request().method() === "POST" && response.url() === `${orgUrl}/dots`);
        await creation.getByRole("button", { name: "Create", exact: true }).click();
        const firstCreated = await created;
        assert.equal(firstCreated.status(), 201);
        const harald = (await firstCreated.json()).agent;
        dotIds.push(harald.id);
        assert.equal(harald.dot.name, "Harald");
        assert.equal(harald.access, "write");
        await page.waitForURL((url) => url.pathname === `/agents/${harald.id}/setup/start`);
        await page.getByRole("heading", { name: "Harald Engineering", exact: true }).waitFor();
        const detailResponse = await member.request.get(orgUrl);
        assert.equal(detailResponse.status(), 200);
        const initial = await detailResponse.json();
        assert.equal(initial.active_count, 1);
        assert.equal(initial.active_limit, 4);
        for (let index = 1; index < initial.active_limit; index++) {
            const added = await member.request.post(`${orgUrl}/dots`, {
                data: { name: `Capacity ${index}`, display: `Capacity ${index}`, description: "Active capacity acceptance fixture." },
            });
            assert.equal(added.status(), 201);
            dotIds.push((await added.json()).agent.id);
        }
        const overflow = await member.request.post(`${orgUrl}/dots`, { data: { name: "Over capacity", display: "Over capacity", description: "" } });
        assert.equal(overflow.status(), 409);
        await page.goto(`${BASE}/organizations/${organization.id}`, { waitUntil: "networkidle" });
        await page.getByText("4 of 4 Dots active", { exact: true }).waitFor();
        assert.equal(await page.getByRole("button", { name: "Create Dot", exact: true }).isDisabled(), true);
        const haraldCard = page.locator("section").filter({ has: page.getByRole("heading", { name: "Harald Engineering", exact: true }) });
        const deactivated = page.waitForResponse((response) => response.request().method() === "POST" && response.url() === `${orgUrl}/dots/${harald.id}/active`);
        await haraldCard.getByRole("button", { name: "Deactivate", exact: true }).click();
        assert.equal((await deactivated).status(), 200);
        await page.getByText("3 of 4 Dots active", { exact: true }).waitFor();
        assert.equal(await page.getByRole("button", { name: "Create Dot", exact: true }).isDisabled(), false);
        await page.setViewportSize({ width: 390, height: 844 });
        assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true);
        await page.setViewportSize({ width: 1280, height: 900 });
        const removed = await admin.request.put(`${BASE}/api/v0/admin/organizations/${organization.id}`, {
            data: { name: organization.name, description: organization.description, member_groups: [], expected_revision: organization.revision },
        });
        assert.equal(removed.status(), 200);
        organization = (await removed.json()).organization;
        await page.goto(`${BASE}/organizations`, { waitUntil: "networkidle" });
        assert.equal(await page.getByRole("link", { name: organization.name, exact: true }).count(), 0);
        const removedDetail = await member.request.get(orgUrl);
        assert.equal(removedDetail.status(), 404);
        const removedList = await member.request.get(`${BASE}/api/v0/organizations`);
        assert.equal(removedList.status(), 200);
        assert.equal((await removedList.json()).organizations.some((row) => row.id === organization.id), false);
    } finally {
        try {
            if (admin) {
                try {
                    for (const id of dotIds) await admin.request.delete(`${BASE}/api/v0/agents/${id}`, { timeout: 5000 });
                    if (organization) await admin.request.put(`${BASE}/api/v0/admin/organizations/${organization.id}`, {
                        data: { name: organization.name, description: organization.description, member_groups: [], expected_revision: organization.revision }, timeout: 5000,
                    });
                } finally {
                    if (originalGroup) await admin.request.put(`${BASE}/api/v0/admin/groups`, { data: originalGroup, timeout: 5000 });
                }
            }
        } finally {
            try { await member?.close(); }
            finally { try { await admin?.close(); } finally { await browser.close(); } }
        }
    }
});
