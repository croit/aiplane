import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import { BASE, devSessionCookie, gatewayIsUp, launchBrowser } from "./helpers.mjs";

test("Organization writer authorizes and explicitly selects its Dot account without personal Dot permission", async () => {
    assert.ok(await gatewayIsUp(), `gateway is not reachable at ${BASE}`);
    const memberCookie = process.env.AIPLANE_DOT_MEMBER_SESSION_COOKIE?.trim().replace(/^id=/, "");
    assert.ok(memberCookie, "set AIPLANE_DOT_MEMBER_SESSION_COOKIE to the seeded Engineering member session");
    const browser = await launchBrowser();
    const capture = process.env.DOCS_ORG_DOT_IDENTITY_CAPTURE;
    let admin;
    let member;
    let originalGroup;
    let organization;
    let dotId;
    let restoreEnabled = false;
    const key = "dot-workspace";
    try {
        admin = await browser.newContext({ viewport: { width: 1280, height: 900 }, locale: "de-DE" });
        await admin.addCookies([{ name: "id", value: await devSessionCookie(), url: BASE }, { name: "lang", value: "en", url: BASE }]);
        member = await browser.newContext(capture
            ? { viewport: { width: 1400, height: 950 }, deviceScaleFactor: 2, colorScheme: "dark", locale: "de-DE" }
            : { viewport: { width: 1280, height: 900 }, locale: "de-DE" });
        await member.addCookies([{ name: "id", value: memberCookie, url: BASE }, { name: "lang", value: "en", url: BASE }]);
        const groupsResponse = await admin.request.get(`${BASE}/api/v0/admin/groups`);
        assert.equal(groupsResponse.status(), 200);
        originalGroup = (await groupsResponse.json()).groups.find((group) => group.name === "engineering");
        assert.ok(originalGroup);
        assert.equal((await admin.request.put(`${BASE}/api/v0/admin/groups`, {
            data: { ...originalGroup, can_manage_agents: true, can_manage_dots: false },
        })).status(), 200);
        const currentUserResponse = await member.request.get(`${BASE}/api/v0/me`);
        assert.equal(currentUserResponse.status(), 200);
        const currentUser = await currentUserResponse.json();
        assert.equal(currentUser.id, "eng");
        assert.equal(currentUser.can_manage_dots, false);
        assert.equal(currentUser.can_manage_agents, true);
        const created = await admin.request.post(`${BASE}/api/v0/admin/organizations`, {
            data: { name: `Identity Organization ${Date.now()}`, description: "Own support identity.", member_groups: [originalGroup.name] },
        });
        assert.equal(created.status(), 201);
        organization = (await created.json()).organization;
        const createdDot = await member.request.post(`${BASE}/api/v0/organizations/${organization.id}/dots`, {
            data: { name: "Harald", display: "Harald Support", description: "Own organization account." },
        });
        assert.equal(createdDot.status(), 201);
        dotId = (await createdDot.json()).agent.id;
        const inventoryUrl = `${BASE}/api/v0/dots/${dotId}/connections`;
        const connectionUrl = `${inventoryUrl}/${key}`;
        const inventory = async () => {
            const response = await member.request.get(inventoryUrl);
            assert.equal(response.status(), 200);
            const data = await response.json();
            assert.equal(data.dot.name, "Harald");
            assert.equal(data.dot.display, "Harald Support");
            const row = data.connections.find((item) => item.key === key);
            assert.ok(row);
            assert.equal(row.user_connection, null);
            return row;
        };
        const initial = await inventory();
        assert.equal(initial.connector.title, "Workspace");
        assert.deepEqual(initial.supported_modes, ["principal"]);
        assert.equal(initial.can_select, true);
        assert.deepEqual(initial.selected, { mode: "principal", binding_generation: null, connection_id: null, connection_generation: null });
        const page = await member.newPage();
        await page.goto(`${BASE}/agents/${dotId}?tab=setup`, { waitUntil: "networkidle" });
        const panel = page.getByRole("region", { name: "Connector identity", exact: true });
        const row = panel.getByRole("region", { name: initial.connector.title, exact: true });
        await row.getByRole("combobox", { name: "Connector identity", exact: true }).waitFor();
        assert.deepEqual(await row.getByRole("combobox").locator("option").evaluateAll((options) => options.map((option) => option.value)), ["principal"]);
        assert.equal(await row.getByRole("button", { name: "Save", exact: true }).isDisabled(), true);
        assert.equal(await page.getByRole("tab", { name: "Try it", exact: true }).count(), 0);
        const provider = page.waitForRequest((request) => request.method() === "GET" && request.isNavigationRequest() && new URL(request.url()).pathname === "/authorize");
        const callback = page.waitForResponse((response) => new URL(response.url()).pathname === "/integrations/callback");
        const started = page.waitForResponse((response) => response.request().method() === "POST" && response.url() === `${connectionUrl}/connect`);
        await row.getByRole("button", { name: "Connect", exact: true }).click();
        assert.equal((await started).status(), 200);
        assert.ok(new URL((await provider).url()).searchParams.get("state"));
        assert.equal((await callback).status(), 303);
        await page.waitForURL((url) => url.pathname === `/agents/${dotId}` && url.searchParams.get("tab") === "setup");
        await row.getByRole("combobox").waitFor();
        const connected = await inventory();
        assert.equal(connected.principal_connection.connected, true);
        assert.equal(connected.principal_connection.account_label, null);
        assert.deepEqual(connected.selected, initial.selected, "authorizing must not select an organization credential");
        assert.equal(await row.getByRole("button", { name: "Save", exact: true }).isEnabled(), true);
        const saved = page.waitForResponse((response) => response.request().method() === "PUT" && response.url() === connectionUrl);
        await row.getByRole("button", { name: "Save", exact: true }).click();
        const saveResponse = await saved;
        assert.equal(saveResponse.status(), 200);
        assert.deepEqual(saveResponse.request().postDataJSON(), { mode: "principal", expected_binding_generation: null,
            connection_id: connected.principal_connection.id, connection_generation: connected.principal_connection.generation });
        await row.getByRole("status").getByText("Connector identity saved.", { exact: true }).waitFor();
        const selected = await inventory();
        assert.equal(selected.selected.connection_id, connected.principal_connection.id);
        assert.ok(selected.selected.binding_generation);
        if (capture) {
            await page.evaluate(() => document.fonts.ready);
            await row.scrollIntoViewIfNeeded();
            await row.screenshot({ path: capture });
            const size = await row.boundingBox();
            assert.ok(size);
            const manifestPath = "docs/site-support/captures.json";
            const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
            const entry = {
                file: capture, route: `/agents/${dotId}?tab=setup`,
                selector: 'section[aria-label="Connector identity"] section[aria-label="Workspace"]',
                viewport: { width: 1400, height: 950 }, scale: 2,
                width: Math.round(size.width * 2), height: Math.round(size.height * 2),
                data: "Synthetic dev-ui fixture; actual organization member writer without personal Dot management. Real OAuth round trip and explicit selection of Harald Support's own Workspace credential. No employee identity or invented account label. Captured by spa-dots-organization-identities.test.mjs with DOCS_ORG_DOT_IDENTITY_CAPTURE."
            };
            fs.writeFileSync(manifestPath, JSON.stringify([...manifest.filter((item) => item.file !== capture), entry], null, 2) + "\n");
        }
        restoreEnabled = true;
        assert.equal((await admin.request.post(`${BASE}/api/v0/admin/connectors/${key}/toggle`, { data: { enabled: false } })).status(), 200);
        await panel.getByRole("button", { name: "Reload connections", exact: true }).click();
        const restrictedRow = panel.getByRole("region", { name: key, exact: true });
        await restrictedRow.getByRole("button", { name: "Disconnect", exact: true }).waitFor();
        const restricted = await inventory();
        assert.equal(restricted.can_select, false);
        assert.equal(restricted.can_disconnect, true);
        assert.equal(await restrictedRow.getByRole("combobox").isDisabled(), true);
        assert.equal(await restrictedRow.getByRole("button", { name: "Save", exact: true }).isDisabled(), true);
        assert.equal(await restrictedRow.getByRole("button", { name: "Disconnect", exact: true }).isEnabled(), true);
        await page.setViewportSize({ width: 390, height: 844 });
        assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true);
        page.once("dialog", async (dialog) => await dialog.accept());
        const erased = page.waitForResponse((response) => response.request().method() === "DELETE" && response.url() === connectionUrl);
        await restrictedRow.getByRole("button", { name: "Disconnect", exact: true }).click();
        assert.equal((await erased).status(), 200);
        const unavailable = await inventory();
        assert.equal(unavailable.principal_connection, null);
        assert.equal(unavailable.selected.mode, "principal");
        assert.equal(unavailable.selected.connection_id, null);
        assert.equal(unavailable.selected.binding_generation, selected.selected.binding_generation);
    } finally {
        try {
            if (admin) {
                if (restoreEnabled) await admin.request.post(`${BASE}/api/v0/admin/connectors/${key}/toggle`, { data: { enabled: true }, timeout: 5000 });
                if (dotId) await admin.request.delete(`${BASE}/api/v0/agents/${dotId}`, { timeout: 5000 });
                if (organization) {
                    const current = await admin.request.get(`${BASE}/api/v0/admin/organizations`, { timeout: 5000 });
                    if (current.ok()) {
                        const actual = (await current.json()).organizations.find((row) => row.id === organization.id);
                        if (actual) await admin.request.put(`${BASE}/api/v0/admin/organizations/${actual.id}`, {
                            data: { name: actual.name, description: actual.description, member_groups: [], expected_revision: actual.revision }, timeout: 5000,
                        });
                    }
                }
                if (originalGroup) await admin.request.put(`${BASE}/api/v0/admin/groups`, { data: originalGroup, timeout: 5000 });
            }
        } finally {
            try { await member?.close(); } finally { try { await admin?.close(); } finally { await browser.close(); } }
        }
    }
});
