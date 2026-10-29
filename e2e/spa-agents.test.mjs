import { test } from "node:test";
import assert from "node:assert/strict";

import { devSessionCookie, gatewayIsUp, launchBrowser, BASE } from "./helpers.mjs";

test("personal agent lifecycle, task, access rules, private chat, memory, activity, and mobile layout", async () => {
    assert.ok(await gatewayIsUp(), `gateway is not reachable at ${BASE}`);
    const browser = await launchBrowser();
    const context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
    await context.addCookies([
        { name: "id", value: await devSessionCookie(), url: BASE },
        { name: "lang", value: "en", url: BASE },
    ]);
    const page = await context.newPage();
    const name = `UX probe ${Date.now()}`;
    const editor = () => page.getByRole("region", { name: "Agent details", exact: true });
    let agentId;

    try {
        await page.goto(`${BASE}/agents`, { waitUntil: "networkidle" });
        await page.getByRole("heading", { name: "Agents", exact: true }).waitFor();
        await page.getByRole("button", { name: "New agent", exact: true }).click();

        const createDialog = page.getByRole("dialog");
        await createDialog.getByRole("radio", { name: /Only me/ }).check();
        await createDialog.getByRole("textbox", { name: "Name", exact: true }).fill(name);
        const description = "Keeps my support tickets visible and flags ones nearing escalation.";
        await createDialog.getByRole("textbox", { name: /Description/ }).fill(description);
        const created = page.waitForResponse((response) => response.request().method() === "POST" && response.url() === `${BASE}/api/v0/agents`);
        await createDialog.getByRole("button", { name: "Create agent", exact: true }).click();
        assert.equal((await created).status(), 201);
        agentId = (await (await created).json()).agent.id;
        await page.waitForURL((url) => url.pathname === `/agents/${agentId}`);
        await page.getByRole("heading", { name, exact: true }).waitFor();
        await page.getByText(description, { exact: true }).first().waitFor();

        const tabs = editor().getByRole("tablist", { name: "Agent details", exact: true });
        for (const tab of ["Task", "Tools", "Memory", "Activity", "Collaboration", "Files", "Automation"]) {
            await tabs.getByRole("tab", { name: tab, exact: true }).waitFor();
        }

        const task = editor().getByRole("tabpanel", { name: "Task", exact: true });
        await task.getByRole("textbox", { name: /^Task/ }).fill("Watch the support queue and flag tickets older than two days.");
        await task.getByRole("textbox", { name: /^Topics/ }).fill("Support, Escalations");
        await task.getByRole("button", { name: "Save", exact: true }).click();
        await task.getByRole("status").getByText("Saved.", { exact: true }).waitFor();
        const profile = await (await context.request.get(`${BASE}/api/v0/agents/${agentId}/profile`)).json();
        assert.equal(profile.task, "Watch the support queue and flag tickets older than two days.");
        assert.deepEqual(profile.topics, ["Support", "Escalations"]);

        const access = editor().getByRole("tabpanel", { name: "Tools", exact: true });
        await tabs.getByRole("tab", { name: "Tools", exact: true }).click();
        const remember = access.locator("li").filter({ has: page.getByText("remember", { exact: true }) });
        await remember.getByRole("checkbox").waitFor();
        assert.equal(await remember.getByRole("checkbox").isChecked(), true);
        const rememberPermission = remember.getByRole("combobox", { name: "Approval rule", exact: true });
        await rememberPermission.selectOption("always_ask");
        await tabs.getByRole("tab", { name: "Memory", exact: true }).click();
        await tabs.getByRole("tab", { name: "Tools", exact: true }).click();
        assert.equal(await rememberPermission.inputValue(), "always_ask", "changing tabs must keep unsaved access edits");
        await access.getByRole("button", { name: "Save", exact: true }).last().click();
        await access.getByRole("status").getByText("Access and approval rules saved.", { exact: true }).waitFor();
        await access.getByRole("button", { name: "Reload access", exact: true }).click();
        await page.waitForFunction(() => {
            const row = Array.from(document.querySelectorAll("[role=tabpanel] li")).find((element) => element.querySelector("code")?.textContent === "remember");
            return row?.querySelector("select")?.value === "always_ask";
        });
        await remember.getByRole("combobox", { name: "Approval rule", exact: true }).selectOption("always_allow");
        await access.getByRole("button", { name: "Save", exact: true }).last().click();
        await access.getByRole("status").getByText("Access and approval rules saved.", { exact: true }).waitFor();

        await page.getByRole("button", { name: "Chat", exact: true }).click();
        await page.waitForURL(/\/chat\//);
        await page.goto(`${BASE}/agents/${agentId}?tab=memory`, { waitUntil: "networkidle" });

        const memory = editor().getByRole("tabpanel", { name: "Memory", exact: true });
        await memory.getByRole("button", { name: "Add", exact: true }).first().click();
        const memoryDialog = page.getByRole("dialog");
        await memoryDialog.getByRole("textbox", { name: "Content", exact: true }).fill("Keep project notes private to this agent.");
        await memoryDialog.getByRole("button", { name: "Remember", exact: true }).click();
        const savedMemory = memory.getByRole("textbox", { name: "Content", exact: true });
        await savedMemory.waitFor();
        assert.equal(await savedMemory.inputValue(), "Keep project notes private to this agent.");

        const activity = editor().getByRole("tabpanel", { name: "Activity", exact: true });
        await tabs.getByRole("tab", { name: "Activity", exact: true }).click();
        await activity.getByText("Started an agent chat.", { exact: true }).waitFor();

        await page.goto(`${BASE}/agents`, { waitUntil: "networkidle" });
        const mine = page.locator("section").filter({ has: page.getByRole("heading", { name: "My agents", exact: true }) });
        const agentCard = mine.locator("a.card").filter({ has: page.getByRole("heading", { name, exact: true }) });
        await agentCard.getByText("Personal", { exact: true }).waitFor();
        assert.equal(await agentCard.getByText(/^personal-agent-/).count(), 0, "cards should not expose the generated principal ID as the identity label");

        await page.setViewportSize({ width: 390, height: 844 });
        await page.goto(`${BASE}/agents/${agentId}?tab=tools`, { waitUntil: "networkidle" });
        await remember.getByRole("checkbox").waitFor();
        assert.ok(
            await page.locator("main").evaluate((element) => element.scrollWidth <= element.clientWidth),
            "the agent editor must not overflow a mobile viewport",
        );
    } finally {
        if (agentId) await context.request.put(`${BASE}/api/v0/agents/${agentId}/active`, { data: { active: false } });
        await context.close();
        await browser.close();
    }
});
