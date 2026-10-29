import { test } from "node:test";
import assert from "node:assert/strict";
import { BASE, devSessionCookie, gatewayIsUp, launchBrowser } from "./helpers.mjs";

test("A group can grant personal Dot management independently and revoke it through the existing editor", async () => {
    assert.ok(await gatewayIsUp(), `gateway is not reachable at ${BASE}`);
    const memberCookie = process.env.AIPLANE_DOT_MEMBER_SESSION_COOKIE?.trim().replace(/^id=/, "");
    assert.ok(memberCookie, "set AIPLANE_DOT_MEMBER_SESSION_COOKIE to the seeded Engineering member session");
    const browser = await launchBrowser();
    let admin;
    let member;
    let originalGroup;
    let dotId;
    try {
        admin = await browser.newContext({ viewport: { width: 1280, height: 900 } });
        await admin.addCookies([
            { name: "id", value: await devSessionCookie(), url: BASE },
            { name: "lang", value: "en", url: BASE },
        ]);
        member = await browser.newContext({ viewport: { width: 1280, height: 900 } });
        await member.addCookies([
            { name: "id", value: memberCookie, url: BASE },
            { name: "lang", value: "en", url: BASE },
        ]);
        const groups = await admin.request.get(`${BASE}/api/v0/admin/groups`);
        assert.equal(groups.status(), 200);
        originalGroup = (await groups.json()).groups.find((group) => group.name === "engineering");
        assert.ok(originalGroup, "use the actual seeded Engineering group");
        assert.equal(originalGroup.is_admin, false);
        const baseline = await admin.request.put(`${BASE}/api/v0/admin/groups`, {
            data: { ...originalGroup, can_manage_agents: false, can_manage_dots: false },
        });
        assert.equal(baseline.status(), 200);
        async function memberMe() {
            const response = await member.request.get(`${BASE}/api/v0/me`);
            assert.equal(response.status(), 200);
            return response.json();
        }
        assert.equal((await memberMe()).id, "eng");
        assert.equal((await memberMe()).can_manage_agents, false);
        assert.equal((await memberMe()).can_manage_dots, false);
        const memberPage = await member.newPage();
        await memberPage.goto(BASE, { waitUntil: "networkidle" });
        assert.equal(await memberPage.locator('aside a[href="/agents"]').count(), 0);
        const denied = await member.request.get(`${BASE}/api/v0/dots`);
        assert.equal(denied.status(), 403);

        const page = await admin.newPage();
        await page.goto(`${BASE}/admin/groups`, { waitUntil: "networkidle" });
        const groupCard = page.locator("article").filter({ has: page.getByRole("heading", { name: "engineering", exact: true }) });
        const dotPermission = groupCard.getByRole("checkbox", { name: "Personal agent manager", exact: true });
        const agentPermission = groupCard.getByRole("checkbox", { name: "Agent manager", exact: true });
        assert.equal(await agentPermission.isChecked(), false);
        assert.equal(await dotPermission.isChecked(), false);
        await dotPermission.check();
        const saved = page.waitForResponse((response) => response.request().method() === "PUT"
            && response.url() === `${BASE}/api/v0/admin/groups`);
        await groupCard.getByRole("button", { name: "Save", exact: true }).click();
        assert.equal((await saved).status(), 200);
        await page.reload({ waitUntil: "networkidle" });
        assert.equal(await dotPermission.isChecked(), true);
        assert.equal(await agentPermission.isChecked(), false);
        const grantedMe = await memberMe();
        assert.equal(grantedMe.can_manage_dots, true);
        assert.equal(grantedMe.can_manage_agents, false);
        await memberPage.reload({ waitUntil: "networkidle" });
        await memberPage.locator('aside a[href="/agents"]').waitFor();
        await memberPage.locator('aside a[href="/agents"]').click();
        await memberPage.getByRole("button", { name: "New agent", exact: true }).click();
        const dialog = memberPage.locator("dialog[open]");
        const name = `Permission Dot ${Date.now()}`;
        await dialog.getByRole("textbox", { name: "Name", exact: true }).fill(name);
        const created = memberPage.waitForResponse((response) => response.request().method() === "POST"
            && response.url() === `${BASE}/api/v0/dots`);
        await dialog.getByRole("button", { name: "Create agent", exact: true }).click();
        const creation = await created;
        assert.equal(creation.status(), 201);
        dotId = (await creation.json()).dot.id;
        await memberPage.waitForURL((url) => url.pathname === `/agents/${dotId}`);
        const dotCard = memberPage.locator("main");
        const deactivated = memberPage.waitForResponse((response) => response.request().method() === "PUT"
            && response.url() === `${BASE}/api/v0/dots/${dotId}/active`);
        await dotCard.getByRole("button", { name: "Deactivate", exact: true }).click();
        assert.equal((await deactivated).status(), 200);
        const adminDenied = await member.request.get(`${BASE}/api/v0/admin/groups`);
        assert.equal(adminDenied.status(), 403);

        await dotPermission.uncheck();
        const revoked = page.waitForResponse((response) => response.request().method() === "PUT"
            && response.url() === `${BASE}/api/v0/admin/groups`);
        await groupCard.getByRole("button", { name: "Save", exact: true }).click();
        assert.equal((await revoked).status(), 200);
        assert.equal((await memberMe()).can_manage_dots, false);
        await memberPage.goto(BASE, { waitUntil: "networkidle" });
        assert.equal(await memberPage.locator('aside a[href="/agents"]').count(), 0);
        const revokedList = await member.request.get(`${BASE}/api/v0/dots`);
        assert.equal(revokedList.status(), 403);
        const revokedCreate = await member.request.post(`${BASE}/api/v0/dots`, { data: { name: "Revoked permission probe" } });
        assert.equal(revokedCreate.status(), 403);
    } finally {
        try {
            if (admin && originalGroup) {
                try {
                    if (dotId && member) {
                        await admin.request.put(`${BASE}/api/v0/admin/groups`, {
                            data: { ...originalGroup, can_manage_dots: true }, timeout: 5000,
                        });
                        await member.request.put(`${BASE}/api/v0/dots/${dotId}/active`, { data: { active: false }, timeout: 5000 });
                    }
                } finally {
                    await admin.request.put(`${BASE}/api/v0/admin/groups`, { data: originalGroup, timeout: 5000 });
                }
            }
        } finally {
            try { await member?.close(); }
            finally {
                try { await admin?.close(); }
                finally { await browser.close(); }
            }
        }
    }
});
