import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import { BASE, devSessionCookie, gatewayIsUp, launchBrowser } from "./helpers.mjs";

test("Personal Dot OAuth connects without selecting, saves explicitly and disconnects only its own account", async () => {
    assert.ok(await gatewayIsUp(), `gateway is not reachable at ${BASE}`);
    const browser = await launchBrowser();
    const capture = process.env.DOCS_DOT_IDENTITY_CAPTURE;
    let context;
    let dotId;
    const key = "dot-workspace";
    try {
        context = await browser.newContext(capture
            ? { viewport: { width: 1400, height: 950 }, deviceScaleFactor: 2, colorScheme: "dark", locale: "de-DE" }
            : { viewport: { width: 1280, height: 900 }, locale: "de-DE" });
        await context.addCookies([
            { name: "id", value: await devSessionCookie(), url: BASE },
            { name: "lang", value: "en", url: BASE },
        ]);
        const page = await context.newPage();
        const created = await context.request.post(`${BASE}/api/v0/dots`, {
            data: { name: `identity-harald-${Date.now()}`, display: "Harald" },
        });
        assert.equal(created.status(), 201);
        dotId = (await created.json()).dot.id;
        const inventoryUrl = `${BASE}/api/v0/dots/${dotId}/connections`;
        const connectionUrl = `${inventoryUrl}/${key}`;
        async function inventory() {
            const response = await context.request.get(inventoryUrl);
            assert.equal(response.status(), 200);
            const data = await response.json();
            assert.equal(data.dot.id, dotId);
            const connection = data.connections.find((item) => item.key === key);
            assert.ok(connection, "the actual fixture must expose its permitted dot-workspace connector");
            return connection;
        }
        const initial = await inventory();
        assert.equal(initial.connector.title, "Workspace");
        assert.equal(initial.connector.scope, "per_user");
        assert.equal(initial.connector.auth_type, "oauth2");
        assert.deepEqual(initial.supported_modes, ["user", "principal"]);
        assert.equal(initial.selected.mode, "user");
        assert.equal(initial.user_connection.connected, true);
        assert.ok(initial.user_connection.id);
        assert.ok(initial.user_connection.generation);
        assert.equal(initial.user_connection.account_label, null);
        assert.equal(initial.principal_connection, null);
        const userBefore = initial.user_connection;

        await page.goto(`${BASE}/dots?dot_id=${encodeURIComponent(dotId)}`, { waitUntil: "networkidle" });
        const card = page.locator(`[id="dot-${dotId}"]`);
        const access = card.locator("details").filter({ has: page.getByText("Access and approvals", { exact: true }) });
        const row = access.getByRole("region", { name: initial.connector.title, exact: true });
        await row.getByRole("combobox", { name: "Connector identity", exact: true }).waitFor();
        assert.equal(await access.evaluate((element) => element.open), true);
        assert.equal(await row.getByRole("combobox").inputValue(), "user");
        await row.getByText("The provider decides which account you authorize. Connecting does not create an account or change this Dot’s saved selection.", { exact: true }).waitFor();
        assert.equal(await row.getByRole("button", { name: "Save", exact: true }).isDisabled(), true);

        const connectResponse = page.waitForResponse((response) => response.request().method() === "POST"
            && response.url() === `${connectionUrl}/connect`);
        const providerNavigation = page.waitForRequest((request) => request.method() === "GET"
            && request.isNavigationRequest() && new URL(request.url()).pathname === "/authorize");
        const callbackResponse = page.waitForResponse((response) => new URL(response.url()).pathname === "/integrations/callback");
        await row.getByRole("button", { name: "Connect", exact: true }).click();
        const started = await connectResponse;
        assert.equal(started.status(), 200);
        assert.equal(started.request().headers()["accept-language"], "en");
        assert.deepEqual(started.request().postDataJSON(), { connection_id: null, connection_generation: null });
        const authorizationUrl = new URL((await providerNavigation).url());
        assert.ok(authorizationUrl.searchParams.get("state"));
        assert.equal((await callbackResponse).status(), 303);
        await page.waitForURL((url) => url.pathname === "/dots" && url.searchParams.get("dot_id") === dotId);
        await row.getByRole("combobox").waitFor();
        const connected = await inventory();
        assert.equal(connected.principal_connection.connected, true);
        assert.equal(connected.principal_connection.authorizing, false);
        assert.equal(connected.principal_connection.account_label, null);
        assert.ok(connected.principal_connection.id);
        assert.ok(connected.principal_connection.generation);
        assert.equal(connected.selected.mode, "user", "OAuth completion must not silently choose the Dot identity");
        assert.deepEqual(connected.user_connection, userBefore);
        assert.equal(await row.getByRole("combobox").inputValue(), "user");
        assert.equal(await row.getByRole("button", { name: "Save", exact: true }).isDisabled(), true);

        await row.getByRole("combobox").selectOption("principal");
        const savedResponse = page.waitForResponse((response) => response.request().method() === "PUT" && response.url() === connectionUrl);
        await row.getByRole("button", { name: "Save", exact: true }).click();
        const saved = await savedResponse;
        assert.equal(saved.status(), 200);
        assert.deepEqual(saved.request().postDataJSON(), {
            mode: "principal",
            expected_binding_generation: connected.selected.binding_generation,
            connection_id: connected.principal_connection.id,
            connection_generation: connected.principal_connection.generation,
        });
        await row.getByRole("status").getByText("Connector identity saved.", { exact: true }).waitFor();
        const selected = await inventory();
        assert.equal(selected.selected.mode, "principal");
        assert.equal(selected.selected.connection_id, connected.principal_connection.id);
        assert.equal(selected.selected.connection_generation, connected.principal_connection.generation);
        assert.ok(selected.selected.binding_generation);
        const accessResponse = await context.request.get(`${BASE}/api/v0/dots/${dotId}/access`);
        assert.equal(accessResponse.status(), 200);
        const actualTools = (await accessResponse.json()).tools.filter((tool) => tool.connector_key === key);
        assert.ok(actualTools.some((tool) => tool.id.endsWith("read_workspace")), "the selected credential must expose the real MCP read tool");
        assert.ok(actualTools.some((tool) => tool.id.endsWith("write_workspace")), "the selected credential must expose the real MCP write tool");
        assert.ok(actualTools.every((tool) => tool.available && tool.identity === "principal"));

        if (capture) {
            await page.evaluate(() => document.fonts.ready);
            await row.scrollIntoViewIfNeeded();
            await row.screenshot({ path: capture });
            const size = await row.boundingBox();
            assert.ok(size);
            const manifestPath = "docs/site-support/captures.json";
            const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
            const entry = {
                file: capture,
                route: `/dots?dot_id=${dotId}`,
                selector: `[id="dot-${dotId}"] section[aria-label="Workspace"]`,
                viewport: { width: 1400, height: 950 },
                scale: 2, width: Math.round(size.width * 2), height: Math.round(size.height * 2),
                data: "Synthetic dev-ui fixture; actual Harald Workspace principal connection explicitly selected after the real OAuth provider redirect and callback. The provider account label is absent; no account or email is invented. Capture generated by spa-dots-identities.test.mjs with DOCS_DOT_IDENTITY_CAPTURE."
            };
            fs.writeFileSync(manifestPath, JSON.stringify([...manifest.filter((item) => item.file !== capture), entry], null, 2) + "\n");
        }

        let disconnectDialog;
        page.once("dialog", async (dialog) => {
            disconnectDialog = { type: dialog.type(), message: dialog.message() };
            await dialog.accept();
        });
        const disconnectResponse = page.waitForResponse((response) => response.request().method() === "DELETE" && response.url() === connectionUrl);
        await row.getByRole("button", { name: "Disconnect", exact: true }).click();
        const disconnected = await disconnectResponse;
        assert.equal(disconnected.status(), 200);
        assert.deepEqual(disconnectDialog, { type: "confirm", message: "Disconnect Harald’s connection to Workspace? This removes its local credentials and pending authorization; your own connection remains." });
        assert.deepEqual(disconnected.request().postDataJSON(), {
            connection_id: connected.principal_connection.id,
            connection_generation: connected.principal_connection.generation,
        });
        await row.getByRole("status").getByText("Dot connection disconnected.", { exact: true }).waitFor();
        const unavailable = await inventory();
        assert.equal(unavailable.selected.mode, "principal", "disconnect must not select a fallback identity");
        assert.deepEqual(unavailable.selected, { ...selected.selected, connection_id: null });
        assert.equal(unavailable.principal_connection?.connected ?? false, false);
        assert.deepEqual(unavailable.user_connection, userBefore);
        assert.equal(await row.getByRole("combobox").inputValue(), "principal");
        await row.getByText("The saved connection is unavailable. Connect that identity or explicitly select another available connection.", { exact: true }).waitFor();
        assert.equal(await row.getByRole("button", { name: "Save", exact: true }).isDisabled(), true);

        await row.getByRole("combobox").selectOption("user");
        const userSelectedResponse = page.waitForResponse((response) => response.request().method() === "PUT" && response.url() === connectionUrl);
        await row.getByRole("button", { name: "Save", exact: true }).click();
        const userSelected = await userSelectedResponse;
        assert.equal(userSelected.status(), 200);
        assert.deepEqual(userSelected.request().postDataJSON(), {
            mode: "user", expected_binding_generation: selected.selected.binding_generation,
            connection_id: userBefore.id, connection_generation: userBefore.generation,
        });
        await row.getByRole("status").getByText("Connector identity saved.", { exact: true }).waitFor();
        const final = await inventory();
        assert.equal(final.selected.mode, "user");
        assert.deepEqual(final.user_connection, userBefore);
        await page.setViewportSize({ width: 390, height: 844 });
        assert.ok(await page.locator("main").evaluate((element) => element.scrollWidth <= element.clientWidth), "identity controls must fit a mobile viewport");
        await row.getByRole("combobox").focus();
        assert.equal(await row.getByRole("combobox").evaluate((element) => document.activeElement === element), true);
    } finally {
        try {
            if (context && dotId) {
                try {
                    const response = await context.request.get(`${BASE}/api/v0/dots/${dotId}/connections`, { timeout: 5000 });
                    if (response.status() === 200) {
                        const row = (await response.json()).connections.find((item) => item.key === key);
                        if (row?.principal_connection?.id) await context.request.delete(`${BASE}/api/v0/dots/${dotId}/connections/${key}`, {
                            data: { connection_id: row.principal_connection.id, connection_generation: row.principal_connection.generation }, timeout: 5000,
                        });
                    }
                } finally {
                    await context.request.put(`${BASE}/api/v0/dots/${dotId}/active`, { data: { active: false }, timeout: 5000 });
                }
            }
        } finally {
            try { await context?.close(); }
            finally { await browser.close(); }
        }
    }
});
