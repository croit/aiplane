import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import { BASE, devSessionCookie, gatewayIsUp, launchBrowser } from "./helpers.mjs";

test("Personal agent OAuth connects without selecting, saves explicitly and disconnects only its own account", async () => {
    assert.ok(await gatewayIsUp(), `gateway is not reachable at ${BASE}`);
    const browser = await launchBrowser();
    const capture = process.env.DOCS_AGENT_IDENTITY_CAPTURE;
    let context;
    let agentId;
    const key = "agent-workspace";
    try {
        context = await browser.newContext(capture
            ? { viewport: { width: 1400, height: 950 }, deviceScaleFactor: 2, colorScheme: "dark", locale: "de-DE" }
            : { viewport: { width: 1280, height: 900 }, locale: "de-DE" });
        await context.addCookies([
            { name: "id", value: await devSessionCookie(), url: BASE },
            { name: "lang", value: "en", url: BASE },
        ]);
        const page = await context.newPage();
        const created = await context.request.post(`${BASE}/api/v0/agents`, {
            data: { kind: "personal", name: `identity-harald-${Date.now()}`, display: "Harald" },
        });
        assert.equal(created.status(), 201);
        agentId = (await created.json()).agent.id;
        const inventoryUrl = `${BASE}/api/v0/agents/${agentId}/connections`;
        const connectionUrl = `${inventoryUrl}/${key}`;
        async function inventory() {
            const response = await context.request.get(inventoryUrl);
            assert.equal(response.status(), 200);
            const data = await response.json();
            assert.equal(data.agent.id, agentId);
            const connection = data.connections.find((item) => item.key === key);
            assert.ok(connection, "the actual fixture must expose its permitted agent-workspace connector");
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

        await page.goto(`${BASE}/agents/${agentId}?tab=tools`, { waitUntil: "networkidle" });
        const card = page.getByRole("region", { name: "Agent details", exact: true });
        const access = card.getByRole("tabpanel", { name: "Tools", exact: true });
        const row = access.getByRole("region", { name: initial.connector.title, exact: true });
        await row.getByRole("combobox", { name: "Connector identity", exact: true }).waitFor();
        assert.equal(await card.getByRole("tab", { name: "Tools", exact: true }).getAttribute("aria-selected"), "true");
        assert.equal(await row.getByRole("combobox").inputValue(), "user");
        await row.getByText("The provider decides which account you authorize. Connecting does not create an account or change this agent’s saved selection.", { exact: true }).waitFor();
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
        await page.waitForURL((url) => url.pathname === `/agents/${agentId}`);
        await row.getByRole("combobox").waitFor();
        const connected = await inventory();
        assert.equal(connected.principal_connection.connected, true);
        assert.equal(connected.principal_connection.authorizing, false);
        assert.equal(connected.principal_connection.account_label, null);
        assert.ok(connected.principal_connection.id);
        assert.ok(connected.principal_connection.generation);
        assert.equal(connected.selected.mode, "user", "OAuth completion must not silently choose the agent identity");
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
        const accessResponse = await context.request.get(`${BASE}/api/v0/agents/${agentId}/access`);
        assert.equal(accessResponse.status(), 200);
        const actualTools = (await accessResponse.json()).tools.filter((tool) => tool.connector_key === key);
        assert.ok(actualTools.some((tool) => tool.id.endsWith("read_workspace")), "the selected credential must expose the real MCP read tool");
        assert.ok(actualTools.some((tool) => tool.id.endsWith("write_workspace")), "the selected credential must expose the real MCP write tool");
        assert.ok(actualTools.every((tool) => tool.available && tool.identity === "principal"),
            JSON.stringify(actualTools.map(({ id, available, identity }) => ({ id, available, identity }))));

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
                route: `/agents/${agentId}?tab=tools`,
                selector: `section[aria-label="Agent details"] section[aria-label="Workspace"]`,
                viewport: { width: 1400, height: 950 },
                scale: 2, width: Math.round(size.width * 2), height: Math.round(size.height * 2),
                data: "Synthetic dev-ui fixture; actual Harald Workspace principal connection explicitly selected after the real OAuth provider redirect and callback. The provider account label is absent; no account or email is invented. Capture generated by spa-agents-identities.test.mjs with DOCS_AGENT_IDENTITY_CAPTURE."
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
        await row.getByRole("status").getByText("Agent connection disconnected.", { exact: true }).waitFor();
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
            if (context && agentId) {
                try {
                    const response = await context.request.get(`${BASE}/api/v0/agents/${agentId}/connections`, { timeout: 5000 });
                    if (response.status() === 200) {
                        const row = (await response.json()).connections.find((item) => item.key === key);
                        if (row?.principal_connection?.id) await context.request.delete(`${BASE}/api/v0/agents/${agentId}/connections/${key}`, {
                            data: { connection_id: row.principal_connection.id, connection_generation: row.principal_connection.generation }, timeout: 5000,
                        });
                    }
                } finally {
                    await context.request.put(`${BASE}/api/v0/agents/${agentId}/active`, { data: { active: false }, timeout: 5000 });
                }
            }
        } finally {
            try { await context?.close(); }
            finally { await browser.close(); }
        }
    }
});

test("An owner reloads and erases selected or never-selected local agent credentials after connector disable", async () => {
    assert.ok(await gatewayIsUp(), `gateway is not reachable at ${BASE}`);
    const browser = await launchBrowser();
    let context;
    let agentId;
    let restoreEnabled = false;
    const key = "agent-workspace";
    try {
        context = await browser.newContext({ viewport: { width: 1280, height: 900 }, locale: "de-DE" });
        await context.addCookies([
            { name: "id", value: await devSessionCookie(), url: BASE },
            { name: "lang", value: "en", url: BASE },
        ]);
        const page = await context.newPage();
        const toggleUrl = `${BASE}/api/v0/admin/connectors/${key}/toggle`;
        for (const selected of [true, false]) {
            const created = await context.request.post(`${BASE}/api/v0/agents`, {
                data: { kind: "personal", name: `cleanup-harald-${selected}-${Date.now()}`, display: "Harald" },
            });
            assert.equal(created.status(), 201);
            agentId = (await created.json()).agent.id;
            const inventoryUrl = `${BASE}/api/v0/agents/${agentId}/connections`;
            const connectionUrl = `${inventoryUrl}/${key}`;
            const inventory = async () => {
                const response = await context.request.get(inventoryUrl);
                assert.equal(response.status(), 200);
                return (await response.json()).connections;
            };
            const initial = (await inventory()).find((item) => item.key === key);
            assert.equal(initial.connector.title, "Workspace");
            assert.equal(initial.user_connection.connected, true);
            const userBefore = initial.user_connection;
            await page.goto(`${BASE}/agents/${agentId}?tab=tools`, { waitUntil: "networkidle" });
            const access = page.getByRole("region", { name: "Agent details", exact: true }).getByRole("tabpanel", { name: "Tools", exact: true });
            const permittedRow = access.getByRole("region", { name: "Workspace", exact: true });
            const provider = page.waitForRequest((request) => request.isNavigationRequest()
                && request.method() === "GET" && new URL(request.url()).pathname === "/authorize");
            const callback = page.waitForResponse((response) => new URL(response.url()).pathname === "/integrations/callback");
            await permittedRow.getByRole("button", { name: "Connect", exact: true }).click();
            assert.ok(new URL((await provider).url()).searchParams.get("state"));
            assert.equal((await callback).status(), 303);
            await page.waitForURL((url) => url.pathname === `/agents/${agentId}`);
            await permittedRow.getByRole("combobox").waitFor();
            const connected = (await inventory()).find((item) => item.key === key);
            assert.equal(connected.principal_connection.connected, true);
            assert.equal(connected.can_disconnect, true);
            if (selected) {
                await permittedRow.getByRole("combobox").selectOption("principal");
                const saved = page.waitForResponse((response) => response.request().method() === "PUT" && response.url() === connectionUrl);
                await permittedRow.getByRole("button", { name: "Save", exact: true }).click();
                assert.equal((await saved).status(), 200);
                await permittedRow.getByRole("status").getByText("Connector identity saved.", { exact: true }).waitFor();
            }
            const beforeDisable = (await inventory()).find((item) => item.key === key);
            restoreEnabled = true;
            assert.equal((await context.request.post(toggleUrl, { data: { enabled: false } })).status(), 200);
            await access.getByRole("button", { name: "Reload connections", exact: true }).click();
            const unavailableRow = access.getByRole("region", { name: key, exact: true });
            await unavailableRow.getByText("This agent’s local credentials are unavailable for use.", { exact: true }).waitFor();
            const unavailable = (await inventory()).find((item) => item.key === key);
            assert.ok(unavailable, "never-selected own credentials must remain in the inventory after reload");
            assert.equal(unavailable.connector, null);
            assert.deepEqual(unavailable.supported_modes, []);
            assert.deepEqual(unavailable.principal_connection, beforeDisable.principal_connection);
            assert.deepEqual(unavailable.user_connection, {
                id: null, generation: null, connected: false, errored: false,
                needs_reauth: false, authorizing: false, account_label: null,
            });
            assert.equal(unavailable.can_connect, false);
            assert.equal(unavailable.can_disconnect, true);
            assert.equal(unavailable.selected.connection_id, null);
            assert.equal(unavailable.selected.binding_generation, beforeDisable.selected.binding_generation);
            assert.equal(await unavailableRow.getByRole("combobox").isDisabled(), true);
            assert.equal(await unavailableRow.getByRole("button", { name: "Save", exact: true }).isDisabled(), true);
            assert.equal(await unavailableRow.getByRole("button", { name: /^(Connect|Reconnect)$/ }).count(), 0);
            assert.equal(await unavailableRow.locator("form").count(), 0);
            assert.equal(await unavailableRow.getByText("Workspace", { exact: true }).count(), 0);
            await page.setViewportSize({ width: 390, height: 844 });
            assert.ok(await page.locator("main").evaluate((element) => element.scrollWidth <= element.clientWidth));
            let actualDialog;
            page.once("dialog", async (dialog) => { actualDialog = dialog.message(); await dialog.accept(); });
            const erased = page.waitForResponse((response) => response.request().method() === "DELETE" && response.url() === connectionUrl);
            await unavailableRow.getByRole("button", { name: "Disconnect", exact: true }).click();
            const response = await erased;
            assert.equal(response.status(), 200);
            assert.deepEqual(response.request().postDataJSON(), {
                connection_id: connected.principal_connection.id,
                connection_generation: connected.principal_connection.generation,
            });
            assert.equal(actualDialog, `Disconnect Harald’s connection to ${key}? This removes its local credentials and pending authorization; your own connection remains.`);
            const remaining = (await inventory()).find((item) => item.key === key);
            if (selected) {
                assert.equal(remaining.principal_connection, null);
                assert.equal(remaining.can_disconnect, false);
                assert.equal(remaining.selected.mode, "principal");
                assert.equal(remaining.selected.binding_generation, beforeDisable.selected.binding_generation);
                assert.equal(remaining.selected.connection_generation, beforeDisable.selected.connection_generation);
            } else assert.equal(remaining, undefined);
            assert.equal((await context.request.post(toggleUrl, { data: { enabled: true } })).status(), 200);
            restoreEnabled = false;
            assert.deepEqual((await inventory()).find((item) => item.key === key).user_connection, userBefore);
            assert.equal((await context.request.put(`${BASE}/api/v0/agents/${agentId}/active`, { data: { active: false } })).status(), 200);
            agentId = undefined;
            await page.setViewportSize({ width: 1280, height: 900 });
        }
    } finally {
        try {
            if (context) {
                try {
                    if (restoreEnabled) await context.request.post(`${BASE}/api/v0/admin/connectors/${key}/toggle`, { data: { enabled: true }, timeout: 5000 });
                } finally {
                    if (agentId) {
                        try {
                            const response = await context.request.get(`${BASE}/api/v0/agents/${agentId}/connections`, { timeout: 5000 });
                            if (response.ok()) {
                                const row = (await response.json()).connections.find((item) => item.key === key);
                                if (row?.principal_connection?.id) await context.request.delete(`${BASE}/api/v0/agents/${agentId}/connections/${key}`, {
                                    data: { connection_id: row.principal_connection.id, connection_generation: row.principal_connection.generation }, timeout: 5000,
                                });
                            }
                        } finally {
                            await context.request.put(`${BASE}/api/v0/agents/${agentId}/active`, { data: { active: false }, timeout: 5000 });
                        }
                    }
                }
            }
        } finally {
            try { await context?.close(); } finally { await browser.close(); }
        }
    }
});

test("Catalog resource rotation requires own reauthorization and explicit selection without repairing the User account", async () => {
    assert.ok(await gatewayIsUp(), `gateway is not reachable at ${BASE}`);
    const browser = await launchBrowser();
    let context;
    let agentId;
    let originalCatalog;
    let restoreCatalog = false;
    const key = "agent-workspace";
    const catalogUrl = `${BASE}/api/v0/admin/connectors`;
    const catalogBody = (row, baseUrl) => ({
        key: row.key, title: row.title, description: row.description ?? "", icon: row.icon ?? "",
        category: row.category ?? "", base_url: baseUrl, scope: row.scope, auth_type: row.auth_type,
        scopes: row.scopes, client_id: row.client_id ?? "", client_secret: "", client_json: "",
        use_dcr: row.use_dcr, authorize_url: row.authorize_url ?? "", token_url: row.token_url ?? "",
        registration_url: row.registration_url ?? "", groups: row.groups, audit: row.audit, overwrite: true,
    });
    try {
        context = await browser.newContext({ viewport: { width: 1280, height: 900 }, locale: "de-DE" });
        await context.addCookies([
            { name: "id", value: await devSessionCookie(), url: BASE },
            { name: "lang", value: "en", url: BASE },
        ]);
        const catalogResponse = await context.request.get(catalogUrl);
        assert.equal(catalogResponse.status(), 200);
        originalCatalog = (await catalogResponse.json()).connectors.find((row) => row.key === key);
        assert.ok(originalCatalog);
        assert.equal(originalCatalog.enabled, true);
        const created = await context.request.post(`${BASE}/api/v0/agents`, {
            data: { kind: "personal", name: `resource-harald-${Date.now()}`, display: "Harald" },
        });
        assert.equal(created.status(), 201);
        agentId = (await created.json()).agent.id;
        const inventoryUrl = `${BASE}/api/v0/agents/${agentId}/connections`;
        const connectionUrl = `${inventoryUrl}/${key}`;
        const inventory = async () => {
            const response = await context.request.get(inventoryUrl);
            assert.equal(response.status(), 200);
            const row = (await response.json()).connections.find((item) => item.key === key);
            assert.ok(row);
            return row;
        };
        const initial = await inventory();
        assert.equal(initial.user_connection.connected, true);
        assert.equal(initial.user_connection.needs_reauth, false);
        const page = await context.newPage();
        await page.goto(`${BASE}/agents/${agentId}?tab=tools`, { waitUntil: "networkidle" });
        const access = page.getByRole("region", { name: "Agent details", exact: true }).getByRole("tabpanel", { name: "Tools", exact: true });
        const row = access.getByRole("region", { name: initial.connector.title, exact: true });
        async function authorize(button) {
            const provider = page.waitForRequest((request) => request.method() === "GET"
                && request.isNavigationRequest() && new URL(request.url()).pathname === "/authorize");
            const callback = page.waitForResponse((response) => new URL(response.url()).pathname === "/integrations/callback");
            const started = page.waitForResponse((response) => response.request().method() === "POST" && response.url() === `${connectionUrl}/connect`);
            await row.getByRole("button", { name: button, exact: true }).click();
            assert.equal((await started).status(), 200);
            assert.ok(new URL((await provider).url()).searchParams.get("state"));
            assert.equal((await callback).status(), 303);
            await page.waitForURL((url) => url.pathname === `/agents/${agentId}`);
            await row.getByRole("combobox").waitFor();
        }
        await authorize("Connect");
        const connected = await inventory();
        await row.getByRole("combobox").selectOption("principal");
        const saved = page.waitForResponse((response) => response.request().method() === "PUT" && response.url() === connectionUrl);
        await row.getByRole("button", { name: "Save", exact: true }).click();
        const initialSave = await saved;
        let initialSaveError;
        if (initialSave.status() !== 200) {
            const body = await initialSave.json();
            initialSaveError = { type: body.error?.type, message: body.error?.message };
        }
        assert.equal(initialSave.status(), 200, JSON.stringify(initialSaveError));
        await row.getByRole("status").getByText("Connector identity saved.", { exact: true }).waitFor();
        const selected = await inventory();
        assert.equal(selected.selected.connection_id, connected.principal_connection.id);
        const changedUrl = new URL(originalCatalog.base_url);
        changedUrl.searchParams.set("resource-version", "2");
        restoreCatalog = true;
        const updated = await context.request.put(catalogUrl, { data: catalogBody(originalCatalog, changedUrl.href) });
        let updateError;
        if (updated.status() !== 200) {
            const body = await updated.json();
            updateError = { type: body.error?.type, message: body.error?.message };
        }
        assert.equal(updated.status(), 200, JSON.stringify(updateError));
        await access.getByRole("button", { name: "Reload connections", exact: true }).click();
        await row.getByText("The saved connection is unavailable. Connect that identity or explicitly select another available connection.", { exact: true }).waitFor();
        const stale = await inventory();
        for (const owner of ["principal_connection", "user_connection"]) {
            assert.equal(stale[owner].connected, true, "local credentials remain stored after resource rotation");
            assert.equal(stale[owner].needs_reauth, true);
            assert.equal(stale[owner].id, selected[owner].id);
            assert.equal(stale[owner].generation, selected[owner].generation);
            assert.equal(stale[owner].account_label, null);
        }
        assert.equal(stale.selected.connection_id, null);
        assert.equal(stale.selected.binding_generation, selected.selected.binding_generation);
        assert.equal(stale.selected.connection_generation, selected.selected.connection_generation);
        assert.equal(await row.getByText("Connected", { exact: true }).count(), 0);
        assert.equal(await row.getByRole("button", { name: "Save", exact: true }).isDisabled(), true);
        assert.equal(await row.getByRole("button", { name: "Disconnect", exact: true }).isEnabled(), true);
        await authorize("Reconnect");
        const authorized = await inventory();
        assert.notEqual(authorized.principal_connection.generation, connected.principal_connection.generation);
        assert.equal(authorized.principal_connection.needs_reauth, false);
        assert.equal(authorized.selected.connection_id, null, "reauthorizing must not silently repair the saved selection");
        assert.equal(authorized.selected.binding_generation, selected.selected.binding_generation);
        assert.equal(authorized.selected.connection_generation, selected.selected.connection_generation);
        assert.deepEqual(authorized.user_connection, stale.user_connection, "own reauthorization must not repair the User account");
        const rebound = page.waitForResponse((response) => response.request().method() === "PUT" && response.url() === connectionUrl);
        await row.getByRole("button", { name: "Save", exact: true }).click();
        const response = await rebound;
        assert.equal(response.status(), 200);
        assert.deepEqual(response.request().postDataJSON(), {
            mode: "principal", expected_binding_generation: selected.selected.binding_generation,
            connection_id: authorized.principal_connection.id, connection_generation: authorized.principal_connection.generation,
        });
        await row.getByRole("status").getByText("Connector identity saved.", { exact: true }).waitFor();
        const fresh = await inventory();
        assert.equal(fresh.selected.connection_id, authorized.principal_connection.id);
        assert.equal(fresh.selected.connection_generation, authorized.principal_connection.generation);
        assert.deepEqual(fresh.user_connection, stale.user_connection);
        await page.setViewportSize({ width: 390, height: 844 });
        assert.ok(await page.locator("main").evaluate((element) => element.scrollWidth <= element.clientWidth));
        assert.equal((await context.request.put(catalogUrl, { data: catalogBody(originalCatalog, originalCatalog.base_url) })).status(), 200);
        restoreCatalog = false;
        const restored = await inventory();
        assert.equal(restored.user_connection.needs_reauth, true, "restoring the old URL must not revive the original User credential");
        assert.equal(restored.user_connection.generation, initial.user_connection.generation);
        assert.equal(restored.principal_connection.needs_reauth, true, "the new authorization is pinned to the second resource version");
        assert.equal(restored.selected.connection_id, null);
    } finally {
        try {
            if (context) {
                try {
                    if (restoreCatalog && originalCatalog) await context.request.put(catalogUrl, { data: catalogBody(originalCatalog, originalCatalog.base_url), timeout: 5000 });
                } finally {
                    if (agentId) {
                        try {
                            const response = await context.request.get(`${BASE}/api/v0/agents/${agentId}/connections`, { timeout: 5000 });
                            if (response.ok()) {
                                const own = (await response.json()).connections.find((item) => item.key === key)?.principal_connection;
                                if (own?.id) await context.request.delete(`${BASE}/api/v0/agents/${agentId}/connections/${key}`, {
                                    data: { connection_id: own.id, connection_generation: own.generation }, timeout: 5000,
                                });
                            }
                        } finally {
                            await context.request.put(`${BASE}/api/v0/agents/${agentId}/active`, { data: { active: false }, timeout: 5000 });
                        }
                    }
                }
            }
        } finally {
            try { await context?.close(); } finally { await browser.close(); }
        }
    }
});
