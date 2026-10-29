import { test } from "node:test";
import assert from "node:assert/strict";
import { BASE, devSessionCookie, gatewayIsUp, launchBrowser } from "./helpers.mjs";

test("Personal agent communication saves directed consent, preserves stale drafts and allows inactive revocation", async () => {
    assert.ok(await gatewayIsUp(), `gateway is not reachable at ${BASE}`);
    const browser = await launchBrowser();
    let context;
    const agentIds = [];
    try {
        context = await browser.newContext({ viewport: { width: 1280, height: 900 }, locale: "de-DE" });
        await context.addCookies([
            { name: "id", value: await devSessionCookie(), url: BASE },
            { name: "lang", value: "en", url: BASE },
        ]);
        const page = await context.newPage();
        const suffix = Date.now();
        async function createAgent(name, display) {
            const response = await context.request.post(`${BASE}/api/v0/agents`, { data: { kind: "personal", name, display } });
            assert.equal(response.status(), 201);
            const agent = (await response.json()).agent;
            agentIds.push(agent.id);
            return agent;
        }
        async function inventory(agent) {
            const response = await context.request.get(`${BASE}/api/v0/agents/${agent.id}/communication`);
            assert.equal(response.status(), 200);
            return response.json();
        }
        const harald = await createAgent(`harald-${suffix}`, "Harald");
        const clara = await createAgent(`clara-${suffix}`, "Clara");
        const initial = await inventory(harald);
        assert.ok(initial.peers.every((peer) => peer.agent.id !== harald.id));
        assert.deepEqual(initial.peers.find((peer) => peer.agent.id === clara.id).outgoing,
            { outbound_enabled: false, inbound_enabled: false, generation: null });
        function panel() {
            return page.getByRole("region", { name: "Agent details", exact: true }).getByRole("tabpanel", { name: "Collaboration", exact: true });
        }
        function section(agent, title) {
            return panel(agent).getByRole("region", { name: title, exact: true });
        }
        function row(agent, title, peer) {
            return section(agent, title).locator("li").filter({ has: page.locator(`[title="${peer.id}"]`) });
        }
        async function open(agent) {
            if (new URL(page.url()).pathname !== `/agents/${agent.id}`) {
                await page.goto(`${BASE}/agents/${agent.id}?tab=communication`, { waitUntil: "networkidle" });
            }
            await section(agent, "Can ask").getByRole("checkbox").first().waitFor();
        }
        async function save(agent, title, peer, expectedStatus = 200) {
            const saved = page.waitForResponse((response) => response.request().method() === "PUT"
                && response.url() === `${BASE}/api/v0/agents/${agent.id}/communication`);
            await panel(agent).getByRole("button", { name: "Save changes", exact: true }).click();
            const response = await saved;
            assert.equal(response.status(), expectedStatus);
            if (expectedStatus === 200) await panel(agent).getByRole("status").waitFor();
            return response;
        }
        async function reload(agent) {
            await open(agent);
            const loaded = page.waitForResponse((response) => response.request().method() === "GET"
                && response.url() === `${BASE}/api/v0/agents/${agent.id}/communication`);
            await panel(agent).getByRole("button", { name: "Reload communication", exact: true }).click();
            assert.equal((await loaded).status(), 200);
            await section(agent, "Can ask").getByRole("checkbox").first().waitFor();
        }

        await open(harald);
        const outgoing = row(harald, "Can ask", clara);
        const outgoingCheckbox = outgoing.getByRole("checkbox", { name: `Can ask Clara (${clara.id})`, exact: true });
        assert.equal(await outgoingCheckbox.isChecked(), false);
        assert.equal(await panel(harald).getByRole("button", { name: "Save changes", exact: true }).count(), 0);
        await outgoing.getByText(/^This agent has not accepted your requests\./).waitFor();
        assert.equal(await outgoing.getByRole("link", { name: "Set it on Clara", exact: true }).getAttribute("href"),
            `/agents/${clara.id}?tab=communication`, "the counterpart choice is one click away");
        await outgoingCheckbox.check();
        const firstSave = await save(harald, "Can ask", clara);
        assert.deepEqual(firstSave.request().postDataJSON(), { peer_id: clara.id, direction: "outbound", enabled: true, expected_generation: null });
        assert.equal(await outgoing.getByText("Both consents enabled", { exact: true }).count(), 0);
        const haraldStored = (await inventory(harald)).peers.find((peer) => peer.agent.id === clara.id);
        assert.equal(haraldStored.outgoing.outbound_enabled, true);
        assert.equal(haraldStored.outgoing.inbound_enabled, false);
        assert.equal(haraldStored.incoming.inbound_enabled, false);

        await open(clara);
        const accepts = row(clara, "Accepts requests from", harald);
        const acceptsCheckbox = accepts.getByRole("checkbox", { name: `Accepts requests from Harald (${harald.id})`, exact: true });
        await accepts.getByText(/^This agent is allowed to ask you\./).waitFor();
        await acceptsCheckbox.check();
        const acceptance = await save(clara, "Accepts requests from", harald);
        assert.deepEqual(acceptance.request().postDataJSON(), { peer_id: harald.id, direction: "inbound", enabled: true, expected_generation: haraldStored.outgoing.generation });
        await reload(harald);
        await outgoing.getByText("Both consents enabled", { exact: true }).waitFor();
        assert.equal(await row(harald, "Accepts requests from", clara).getByRole("checkbox").isChecked(), false);
        await open(clara);
        assert.equal(await row(clara, "Can ask", harald).getByRole("checkbox").isChecked(), false);
        await open(harald);
        assert.equal(await panel(harald).getByRole("button", { name: "Save changes", exact: true }).count(), 0);

        const beforeNoop = (await inventory(harald)).peers.find((peer) => peer.agent.id === clara.id).outgoing;
        const noop = await context.request.put(`${BASE}/api/v0/agents/${harald.id}/communication`, {
            data: { peer_id: clara.id, direction: "outbound", enabled: true, expected_generation: beforeNoop.generation },
        });
        assert.equal(noop.status(), 200);
        assert.deepEqual((await noop.json()).peers.find((peer) => peer.agent.id === clara.id).outgoing, beforeNoop);
        await outgoingCheckbox.uncheck();
        const competing = await context.request.put(`${BASE}/api/v0/agents/${harald.id}/communication`, {
            data: { peer_id: clara.id, direction: "outbound", enabled: false, expected_generation: beforeNoop.generation },
        });
        assert.equal(competing.status(), 200);
        await row(harald, "Accepts requests from", clara).getByRole("checkbox").check();
        const stale = await save(harald, "Can ask", clara, 409);
        assert.equal(stale.request().headers()["accept-language"], "en",
            "API errors must follow the explicitly selected UI language rather than the browser preference");
        assert.equal(stale.request().postDataJSON().expected_generation, beforeNoop.generation,
            "saving another row must not replace this draft's generation");
        try {
            await outgoing.getByRole("alert").getByText("The communication permissions changed while you were editing. Reload them and try again.", { exact: true }).waitFor();
        } catch (error) {
            console.error("communication stale evidence", JSON.stringify({
                response: await stale.json(),
                alert: await outgoing.getByRole("alert").allTextContents(),
                language: await page.locator("html").getAttribute("lang"),
                assets: await page.evaluate(() => performance.getEntriesByType("resource")
                    .map((entry) => entry.name).filter((url) => url.includes("/_app/immutable/"))),
            }));
            throw error;
        }
        assert.equal(await outgoingCheckbox.isChecked(), false, "the failed save must preserve the outgoing draft");
        assert.equal(await row(harald, "Accepts requests from", clara).getByRole("checkbox").isChecked(), true,
            "the failed save must preserve the other pending draft too");
        assert.equal(await panel(harald).getByRole("button", { name: "Save changes", exact: true }).isVisible(), true);
        await reload(harald);
        assert.equal(await outgoingCheckbox.isChecked(), false);
        assert.equal(await row(harald, "Accepts requests from", clara).getByRole("checkbox").isChecked(), false,
            "reloading must discard the conflicting inbound draft and show the stored consent");
        assert.equal(await panel(harald).getByRole("button", { name: "Save changes", exact: true }).count(), 0);

        const inactive = await context.request.put(`${BASE}/api/v0/agents/${clara.id}/active`, { data: { active: false } });
        assert.equal(inactive.status(), 200);
        await reload(harald);
        await outgoing.getByText("Inactive", { exact: true }).waitFor();
        assert.equal(await outgoingCheckbox.isDisabled(), true);
        await panel(harald).getByRole("textbox", { name: "Search agents by name or ID", exact: true }).fill(clara.id.toUpperCase());
        await outgoingCheckbox.waitFor();
        assert.equal(await outgoing.locator(`[title="${clara.id}"]`).getAttribute("title"), clara.id);
        await reload(clara);
        assert.equal(await acceptsCheckbox.isChecked(), true);
        assert.equal(await acceptsCheckbox.isDisabled(), false);
        await acceptsCheckbox.uncheck();
        await save(clara, "Accepts requests from", harald);
        assert.equal((await inventory(clara)).peers.find((peer) => peer.agent.id === harald.id).incoming.inbound_enabled, false);
        await page.setViewportSize({ width: 390, height: 844 });
        assert.ok(await page.locator("main").evaluate((element) => element.scrollWidth <= element.clientWidth));
        await open(harald);
        await panel(harald).getByRole("button", { name: "Reload communication", exact: true }).focus();
        assert.equal(await panel(harald).getByRole("button", { name: "Reload communication", exact: true }).evaluate((element) => document.activeElement === element), true);
    } finally {
        try {
            if (context) {
                for (const id of agentIds) {
                    await context.request.put(`${BASE}/api/v0/agents/${id}/active`, { data: { active: false }, timeout: 5000 });
                }
            }
        } finally {
            try { await context?.close(); }
            finally { await browser.close(); }
        }
    }
});
