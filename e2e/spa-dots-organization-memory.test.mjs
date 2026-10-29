import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { BASE, devSessionCookie, gatewayIsUp, launchBrowser } from "./helpers.mjs";

test("Organization memory reuses scoped Dot notes with actual reader and writer authority", async () => {
    assert.ok(await gatewayIsUp());
    const memberCookie = process.env.AIPLANE_DOT_MEMBER_SESSION_COOKIE?.trim().replace(/^id=/, "");
    assert.ok(memberCookie, "provide the actual Engineering fixture session");
    const browser = await launchBrowser();
    const capture = process.env.DOCS_ORG_DOT_MEMORY_CAPTURE
        ?? (process.env.DOTS_SCREENSHOT_DIR ? path.join(process.env.DOTS_SCREENSHOT_DIR, "organization-dot-memory.png") : null);
    let admin;
    let member;
    let originalGroup;
    let organization;
    let dotId;
    try {
        admin = await browser.newContext({ viewport: { width: 1280, height: 900 } });
        await admin.addCookies([{ name: "id", value: await devSessionCookie(), url: BASE }, { name: "lang", value: "en", url: BASE }]);
        member = await browser.newContext(capture
            ? { viewport: { width: 1400, height: 950 }, deviceScaleFactor: 2, colorScheme: "dark" }
            : { viewport: { width: 1280, height: 900 } });
        await member.addCookies([{ name: "id", value: memberCookie, url: BASE }, { name: "lang", value: "en", url: BASE }]);
        const groupsResponse = await admin.request.get(`${BASE}/api/v0/admin/groups`);
        assert.equal(groupsResponse.status(), 200);
        const groups = (await groupsResponse.json()).groups;
        originalGroup = groups.find((group) => group.name === "engineering");
        const adminGroup = groups.find((group) => group.is_admin);
        assert.ok(originalGroup);
        assert.ok(adminGroup);
        assert.equal((await admin.request.put(`${BASE}/api/v0/admin/groups`, {
            data: { ...originalGroup, can_manage_agents: true, can_manage_dots: false },
        })).status(), 200);
        const createdOrg = await admin.request.post(`${BASE}/api/v0/admin/organizations`, {
            data: { name: `Memory Organization ${Date.now()}`, description: "Scoped memory acceptance.", member_groups: [originalGroup.name, adminGroup.name] },
        });
        assert.equal(createdOrg.status(), 201);
        organization = (await createdOrg.json()).organization;
        const createdDot = await member.request.post(`${BASE}/api/v0/organizations/${organization.id}/dots`, {
            data: { name: "Harald", display: "Harald Memory", description: "Scoped lessons." },
        });
        assert.equal(createdDot.status(), 201);
        dotId = (await createdDot.json()).agent.id;
        const memoryUrl = `${BASE}/api/v0/dots/${dotId}/memories`;
        const shared = await admin.request.post(memoryUrl, { data: { scope: "organization", kind: "fact", content: "Reviewed organization knowledge." } });
        assert.equal(shared.status(), 201);
        const adminPage = await admin.newPage();
        await adminPage.goto(`${BASE}/agents/${dotId}?tab=setup`, { waitUntil: "networkidle" });
        const adminPanel = adminPage.locator("details").filter({ has: adminPage.locator("summary").getByText("Agent memory", { exact: true }) });
        await adminPanel.locator("summary").click();
        const adminShared = adminPanel.getByRole("region", { name: organization.name, exact: true });
        const sharedInput = adminShared.getByRole("textbox", { name: "Content", exact: true });
        await sharedInput.waitFor();
        assert.equal(await adminShared.getByRole("button", { name: "Add", exact: true }).count(), 3);
        await sharedInput.fill("Reviewed organization knowledge, corrected.");
        const sharedUpdated = adminPage.waitForResponse((response) => response.request().method() === "PUT" && response.url().startsWith(`${memoryUrl}/`));
        await adminShared.getByRole("button", { name: "Save", exact: true }).click();
        const sharedUpdatedResponse = await sharedUpdated;
        assert.equal(sharedUpdatedResponse.status(), 200);
        assert.equal(sharedUpdatedResponse.request().postDataJSON().scope, "organization");
        const page = await member.newPage();
        await page.goto(`${BASE}/agents/${dotId}?tab=setup`, { waitUntil: "networkidle" });
        const panel = page.locator("details").filter({ has: page.locator("summary").getByText("Agent memory", { exact: true }) });
        await panel.locator("summary").click();
        const own = panel.getByRole("region", { name: "Harald", exact: true });
        const global = panel.getByRole("region", { name: organization.name, exact: true });
        await global.getByText("Reviewed organization knowledge, corrected.", { exact: true }).waitFor();
        assert.equal(await global.getByRole("button", { name: "Add", exact: true }).count(), 0);
        assert.equal(await global.getByRole("textbox").count(), 0);
        const facts = own.locator("section").filter({ has: page.getByRole("heading", { name: "Facts", exact: true }) });
        await facts.getByRole("button", { name: "Add", exact: true }).click();
        const modal = page.locator("dialog[open]");
        await modal.getByRole("textbox", { name: "Content", exact: true }).fill("A lesson for Harald only.");
        const added = page.waitForResponse((response) => response.url() === memoryUrl && response.request().method() === "POST");
        await modal.getByRole("button", { name: "Remember", exact: true }).click();
        const addedResponse = await added;
        assert.equal(addedResponse.status(), 201);
        assert.equal(addedResponse.request().postDataJSON().scope, "dot");
        const note = await addedResponse.json();
        const input = own.getByRole("textbox", { name: "Content", exact: true });
        await input.waitFor();
        assert.equal(await input.inputValue(), "A lesson for Harald only.");
        const oversized = "é".repeat(1001);
        await input.fill(oversized);
        await own.getByRole("button", { name: "Save", exact: true }).click();
        await panel.getByRole("alert").getByText("This note is longer than 2,000 UTF-8 bytes. Shorten it before saving.", { exact: true }).waitFor();
        assert.equal(await input.inputValue(), oversized);
        assert.equal(await input.isEditable(), true);
        assert.equal(await own.getByRole("button", { name: "Save", exact: true }).isEnabled(), true);
        const afterValidation = await member.request.get(memoryUrl);
        assert.equal(afterValidation.status(), 200);
        assert.equal((await afterValidation.json()).memories.find((memory) => memory.id === note.id).content, "A lesson for Harald only.");
        await input.fill("An improved lesson for Harald only.");
        const updated = page.waitForResponse((response) => response.url() === `${memoryUrl}/${note.id}` && response.request().method() === "PUT");
        await own.getByRole("button", { name: "Save", exact: true }).click();
        const updatedResponse = await updated;
        assert.equal(updatedResponse.status(), 200);
        assert.equal(updatedResponse.request().postDataJSON().scope, "dot");
        if (capture) {
            await page.evaluate(async () => {
                document.documentElement.setAttribute("data-theme", "dark");
                await document.fonts.ready;
            });
            await input.waitFor();
            assert.equal(await input.inputValue(), "An improved lesson for Harald only.");
            await global.getByText("Reviewed organization knowledge, corrected.", { exact: true }).waitFor();
            await panel.scrollIntoViewIfNeeded();
            fs.mkdirSync(path.dirname(capture), { recursive: true });
            await panel.screenshot({ path: capture, animations: "disabled" });
            const size = await panel.boundingBox();
            assert.ok(size);
            const entry = {
                file: capture, route: `/agents/${dotId}?tab=setup`,
                selector: 'details:has(> summary:text-is("Agent memory"))',
                viewport: { width: 1400, height: 950 }, scale: 2,
                width: Math.round(size.width * 2), height: Math.round(size.height * 2),
                data: "Synthetic dev-ui fixture; actual Engineering organization Writer without personal Dot management. Existing memory editor shows Harald's editable Dot lesson and actual organization knowledge as read-only, after real scoped writes. Captured by spa-dots-organization-memory.test.mjs with DOCS_ORG_DOT_MEMORY_CAPTURE or DOTS_SCREENSHOT_DIR.",
            };
            fs.writeFileSync("/tmp/dot-organization-memory-capture-entry.json", `${JSON.stringify(entry, null, 2)}\n`);
        }
        assert.equal((await member.request.post(memoryUrl, { data: { scope: "organization", kind: "fact", content: "Forbidden shared edit." } })).status(), 403);
        await page.setViewportSize({ width: 390, height: 844 });
        assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true);
        const currentAdmin = await admin.request.get(`${BASE}/api/v0/me`);
        assert.equal(currentAdmin.status(), 200);
        const adminIdentity = await currentAdmin.json();
        const retainedWriter = await admin.request.post(`${BASE}/api/v0/agents/${dotId}/shares`, {
            data: { subject_kind: "user", subject_id: adminIdentity.id, access: "write" },
        });
        assert.equal(retainedWriter.status(), 201);
        const sharedRead = await admin.request.post(`${BASE}/api/v0/agents/${dotId}/shares`, {
            data: { subject_kind: "user", subject_id: "eng", access: "read" },
        });
        assert.equal(sharedRead.status(), 201);
        await panel.getByRole("button", { name: "Reload memory", exact: true }).click();
        await own.getByText("An improved lesson for Harald only.", { exact: true }).waitFor();
        assert.equal(await own.getByRole("textbox").count(), 0);
        assert.equal(await own.getByRole("button", { name: "Add", exact: true }).count(), 0);
        assert.equal((await member.request.put(`${memoryUrl}/${note.id}`, {
            data: { scope: "dot", kind: "fact", content: "Forbidden reader edit." },
        })).status(), 404);
        const listed = await member.request.get(memoryUrl);
        assert.equal(listed.status(), 200);
        const actual = await listed.json();
        assert.equal(actual.scopes.every((scope) => !scope.writable), true);
        assert.equal(actual.memories.find((memory) => memory.id === note.id).content, "An improved lesson for Harald only.");
        const revoked = await admin.request.put(`${BASE}/api/v0/admin/organizations/${organization.id}`, {
            data: { name: organization.name, description: organization.description, member_groups: [], expected_revision: organization.revision },
        });
        assert.equal(revoked.status(), 200);
        organization = (await revoked.json()).organization;
        const reloadDenied = page.waitForResponse((response) => response.url() === memoryUrl && response.request().method() === "GET");
        await panel.getByRole("button", { name: "Reload memory", exact: true }).click();
        assert.equal((await reloadDenied).status(), 404);
        await panel.getByRole("alert").waitFor();
        assert.equal(await panel.getByRole("region").count(), 0);
        assert.equal(await panel.getByText("An improved lesson for Harald only.", { exact: true }).count(), 0);
        assert.equal(await panel.getByText("Reviewed organization knowledge, corrected.", { exact: true }).count(), 0);
        assert.equal(await panel.getByRole("textbox").count(), 0);
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
            try { await member?.close(); }
            finally { try { await admin?.close(); } finally { await browser.close(); } }
        }
    }
});
