import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import { BASE, devSessionCookie, gatewayIsUp, launchBrowser } from "./helpers.mjs";

test("delegated approval is settled at the root and child history stays read-only", async () => {
    assert.ok(await gatewayIsUp(), `gateway is not reachable at ${BASE}`);
    const browser = await launchBrowser();
    const capture = process.env.DOCS_AGENT_DELEGATION_CAPTURE;
    const context = await browser.newContext(capture
        ? { viewport: { width: 1400, height: 950 }, deviceScaleFactor: 2, colorScheme: "dark" }
        : { viewport: { width: 1280, height: 900 } });
    const ids = [];
    let root;
    try {
        await context.addCookies([
            { name: "id", value: await devSessionCookie(), url: BASE },
            { name: "lang", value: "en", url: BASE },
        ]);
        const page = await context.newPage();
        async function get(path) {
            const response = await context.request.get(`${BASE}${path}`);
            assert.equal(response.status(), 200);
            return response.json();
        }
        async function poll(path, predicate) {
            for (let attempt = 0; attempt < 80; attempt++) {
                const body = await get(path);
                if (predicate(body)) return body;
                await page.waitForTimeout(200);
            }
            assert.fail(`delegation state was not observed at ${path}`);
        }
        const userMemories = (await get("/api/v0/memories")).memories;
        const suffix = Date.now();
        for (const name of ["Caller", "Receiver"]) {
            const response = await context.request.post(`${BASE}/api/v0/agents`, {
                data: { kind: "personal", name: capture ? (name === "Caller" ? "Harald" : "Clara") : `delegation-${name.toLowerCase()}-${suffix}`, display: name },
            });
            assert.equal(response.status(), 201);
            ids.push((await response.json()).agent.id);
        }
        for (const [index, tool, permission] of [[0, "forward_request", "always_allow"], [1, "remember", "always_ask"]]) {
            const { revision } = await (await context.request.get(`${BASE}/api/v0/agents/${ids[index]}/access`)).json();
            const response = await context.request.put(`${BASE}/api/v0/agents/${ids[index]}/access`, {
                data: { revision, tools: [{ id: tool, permission }] },
            });
            assert.equal(response.status(), 200, await response.text());
        }
        let inventory = await get(`/api/v0/agents/${ids[0]}/communication`);
        const initial = inventory.peers.find((peer) => peer.agent.id === ids[1]).outgoing;
        const outgoing = await context.request.put(`${BASE}/api/v0/agents/${ids[0]}/communication`, {
            data: { peer_id: ids[1], direction: "outbound", enabled: true, expected_generation: initial.generation },
        });
        assert.equal(outgoing.status(), 200);
        inventory = await outgoing.json();
        const edge = inventory.peers.find((peer) => peer.agent.id === ids[1]).outgoing;
        const incoming = await context.request.put(`${BASE}/api/v0/agents/${ids[1]}/communication`, {
            data: { peer_id: ids[0], direction: "inbound", enabled: true, expected_generation: edge.generation },
        });
        assert.equal(incoming.status(), 200);
        const session = await context.request.post(`${BASE}/api/v0/agents/${ids[0]}/sessions`, { data: {} });
        assert.equal(session.status(), 201);
        root = (await session.json()).session.id;
        const submitted = await context.request.post(`${BASE}/api/v0/chat/sessions/${root}/messages`, {
            data: { model: "demo-model", message: `Agent delegation approval probe ${ids[1]}` },
        });
        assert.equal(submitted.status(), 202);
        const rootTurn = (await submitted.json()).assistant_turn_id;
        const inbox = await poll("/api/v0/agents/inbox", (body) => body.items.some((item) => item.turn_id === rootTurn));
        const pending = inbox.items.find((item) => item.turn_id === rootTurn);
        assert.equal(pending.call.name, "remember");
        const children = await get(`/api/v0/chat/sessions?agent_id=${ids[1]}`);
        assert.equal(children.sessions.length, 1);
        const child = children.sessions[0].id;
        const detail = await get(`/api/v0/chat/sessions/${child}`);
        assert.equal(detail.can_write, false);
        assert.equal(detail.can_delete, false);
        assert.equal(detail.delegation.root_session_id, root);
        assert.deepEqual((await get(`/api/v0/agents/${ids[1]}/memories`)).memories, []);
        const leaf = detail.turns.find((item) => item.turn.role === "assistant");
        const independent = await context.request.post(`${BASE}/api/v0/chat/sessions/${child}/turns/${leaf.turn.id}/resume`, {
            data: { decision: "allow_once" },
        });
        assert.equal(independent.status(), 404);
        const writes = [];
        page.on("request", (request) => {
            if (request.method() === "POST" && request.url().includes(`/api/v0/chat/sessions/${child}`)) writes.push(request.url());
        });
        await page.goto(`${BASE}/chat/${child}`, { waitUntil: "networkidle" });
        await page.getByText("Delegated conversation — read-only history. Continue, approve or stop this work through the root conversation or Inbox.", { exact: true }).waitFor();
        await page.getByText("Waiting for a human decision", { exact: true }).waitFor({ timeout: 5000 });
        assert.equal(await page.getByPlaceholder("Message the model…").count(), 0);
        assert.equal(await page.getByRole("button", { name: "Approve once", exact: true }).count(), 0);
        const rootLink = page.getByRole("link", { name: "Open root conversation", exact: true });
        assert.equal(await rootLink.getAttribute("href"), `/chat/${root}`);
        if (capture) {
            await page.evaluate(() => document.fonts.ready);
            await page.screenshot({ path: capture });
            const manifestPath = "docs/site-support/captures.json";
            const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
            const entry = {
                file: capture,
                route: `/chat/${child}`,
                selector: "body",
                viewport: { width: 1400, height: 950 },
                scale: 2, width: 2800, height: 1900,
                data: "Synthetic dev-ui fixture; actual Harald to Clara delegation awaiting the owner's memory approval. Private receiver history has no composer or independent approval; the root link comes from verified stored ancestry. Capture generated by spa-agents-delegation.test.mjs with DOCS_AGENT_DELEGATION_CAPTURE."
            };
            fs.writeFileSync(manifestPath, JSON.stringify([...manifest.filter((item) => item.file !== capture), entry], null, 2) + "\n");
        }
        await page.setViewportSize({ width: 390, height: 844 });
        assert.ok(await page.locator("[data-chat-transcript]").evaluate((element) => element.scrollWidth <= element.clientWidth));
        assert.deepEqual(writes, []);
        await rootLink.click();
        await page.waitForURL(`${BASE}/chat/${root}`);
        await page.goto(`${BASE}/inbox?item=${encodeURIComponent(pending.id)}`, { waitUntil: "networkidle" });
        const card = page.locator(`[id="inbox-item-${pending.id}"]`);
        const response = page.waitForResponse((response) => response.request().method() === "POST"
            && response.url().endsWith(`/api/v0/agents/inbox/${encodeURIComponent(pending.id)}/answer`));
        await card.getByRole("button", { name: "Approve once", exact: true }).click();
        assert.equal((await response).status(), 202);
        await poll(`/api/v0/chat/sessions/${root}`, (body) => body.turns.some((item) => item.turn.id === rootTurn && item.turn.status === "completed"));
        const memories = (await get(`/api/v0/agents/${ids[1]}/memories`)).memories;
        assert.equal(memories.length, 1);
        assert.equal(memories[0].content, "Approved delegated lesson.");
        assert.deepEqual((await get(`/api/v0/agents/${ids[0]}/memories`)).memories, []);
        assert.deepEqual((await get("/api/v0/memories")).memories, userMemories);
    } finally {
        if (root) await context.request.post(`${BASE}/api/v0/chat/sessions/${root}/cancel`);
        for (const id of ids) await context.request.put(`${BASE}/api/v0/agents/${id}/active`, { data: { active: false } });
        await context.close();
        await browser.close();
    }
});
