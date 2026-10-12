import { test } from "node:test";
import assert from "node:assert/strict";

import { devSessionCookie, gatewayIsUp, launchBrowser, BASE } from "./helpers.mjs";

test("personal Dot lifecycle, access rules, private chat, memory, activity, and mobile layout", async () => {
    assert.ok(await gatewayIsUp(), `gateway is not reachable at ${BASE}`);
    const browser = await launchBrowser();
    const context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
    await context.addCookies([
        { name: "id", value: await devSessionCookie(), url: BASE },
        { name: "lang", value: "en", url: BASE },
    ]);
    const page = await context.newPage();
    const name = `UX probe ${Date.now()}`;
    const card = () => page.locator("article").filter({ has: page.getByRole("heading", { name, exact: true }) });
    let dotId;

    try {
        const organizationsLoaded = page.waitForResponse((response) => response.request().method() === "GET"
            && response.url() === `${BASE}/api/v0/organizations`);
        await page.goto(`${BASE}/dots`, { waitUntil: "networkidle" });
        const organizationsResponse = await organizationsLoaded;
        assert.equal(organizationsResponse.status(), 200);
        assert.deepEqual((await organizationsResponse.json()).organizations, []);
        assert.equal(await page.getByRole("link", { name: "Organizations", exact: true }).count(), 0);
        await page.getByRole("heading", { name: "Personal Dots", exact: true }).waitFor();
        await page.getByRole("button", { name: "Create Dot", exact: true }).click();

        const createDialog = page.getByRole("dialog");
        await createDialog.getByRole("textbox", { name: "Name", exact: true }).fill(name);
        await createDialog.getByRole("textbox", { name: "Display name", exact: true }).fill(name);
        const description = "Keeps my support tickets visible and flags ones nearing escalation.";
        await createDialog.getByPlaceholder("e.g. Keeps support tickets visible and flags those nearing escalation.", { exact: true }).fill(description);
        await createDialog.getByText("A sentence about the Dot’s purpose. It appears on Dot cards and to other Dots. It does not guide the model; give work instructions in chat.", { exact: true }).waitFor();
        const created = page.waitForResponse((response) => response.request().method() === "POST" && response.url() === `${BASE}/api/v0/dots`);
        await createDialog.getByRole("button", { name: "Create", exact: true }).click();
        assert.equal((await created).status(), 201);
        dotId = (await (await created).json()).dot.id;
        await page.getByRole("status").getByText("Dot created.").waitFor();
        await card().waitFor();
        await card().getByText(description, { exact: true }).waitFor();

        if (await card().getByRole("button", { name: "Details", exact: true }).count()) {
            await card().getByRole("button", { name: "Details", exact: true }).click();
        }
        const tabs = card().getByRole("tablist", { name: "Dot details", exact: true });
        await tabs.getByRole("tab", { name: "Memory", exact: true }).waitFor();
        await tabs.getByRole("tab", { name: "Workspace", exact: true }).waitFor();
        await tabs.getByRole("tab", { name: "Automation", exact: true }).waitFor();

        const access = card().getByRole("tabpanel", { name: "Access", exact: true });
        await tabs.getByRole("tab", { name: "Access", exact: true }).click();
        const remember = access.locator("li").filter({ has: page.getByText("remember", { exact: true }) });
        await remember.getByRole("checkbox").waitFor();
        assert.equal(await remember.getByRole("checkbox").isChecked(), true);
        const rememberPermission = remember.getByRole("combobox", { name: "Approval rule", exact: true });
        await rememberPermission.selectOption("always_ask");
        await tabs.getByRole("tab", { name: "Memory", exact: true }).click();
        await tabs.getByRole("tab", { name: "Access", exact: true }).click();
        assert.equal(await rememberPermission.inputValue(), "always_ask", "changing tabs must keep unsaved access edits");
        await access.getByRole("button", { name: "Save", exact: true }).last().click();
        await access.getByRole("status").getByText("Access and approval rules saved.", { exact: true }).waitFor();
        await access.getByRole("button", { name: "Reload access", exact: true }).click();
        await page.waitForFunction(() => {
            const row = Array.from(document.querySelectorAll("[role=tabpanel] li")).find((element) => element.querySelector("code")?.textContent === "remember");
            return row?.querySelector("select")?.value === "always_ask";
        });
        await remember.getByRole("checkbox").uncheck();
        await access.getByRole("button", { name: "Save", exact: true }).last().click();
        await access.getByRole("status").getByText("Access and approval rules saved.", { exact: true }).waitFor();
        await access.getByRole("button", { name: "Reload access", exact: true }).click();
        await page.waitForFunction(() => {
            const row = Array.from(document.querySelectorAll("[role=tabpanel] li")).find((element) => element.querySelector("code")?.textContent === "remember");
            return row?.querySelector("input[type=checkbox]")?.checked === false;
        });
        await remember.getByRole("checkbox").check();
        await remember.getByRole("combobox", { name: "Approval rule", exact: true }).selectOption("always_allow");
        await access.getByRole("button", { name: "Save", exact: true }).last().click();
        await access.getByRole("status").getByText("Access and approval rules saved.", { exact: true }).waitFor();

        await card().getByRole("button", { name: "Chat", exact: true }).click();
        await page.waitForURL(/\/chat\//);
        await page.goto(`${BASE}/dots`, { waitUntil: "networkidle" });
        await card().waitFor();
        if (await card().getByRole("button", { name: "Details", exact: true }).count()) {
            await card().getByRole("button", { name: "Details", exact: true }).click();
        }

        const memory = card().getByRole("tabpanel", { name: "Memory", exact: true });
        await tabs.getByRole("tab", { name: "Memory", exact: true }).click();
        await memory.getByRole("button", { name: "Add", exact: true }).first().click();
        const memoryDialog = page.getByRole("dialog");
        await memoryDialog.getByRole("textbox", { name: "Content", exact: true }).fill("Keep project notes private to this Dot.");
        await memoryDialog.getByRole("button", { name: "Remember", exact: true }).click();
        const savedMemory = memory.getByRole("textbox", { name: "Content", exact: true });
        await savedMemory.waitFor();
        assert.equal(await savedMemory.inputValue(), "Keep project notes private to this Dot.");

        const activity = card().getByRole("tabpanel", { name: "Activity", exact: true });
        await tabs.getByRole("tab", { name: "Activity", exact: true }).click();
        await activity.getByText("Started a Dot chat.", { exact: true }).waitFor();

        await page.goto(`${BASE}/agents`, { waitUntil: "networkidle" });
        const agentCard = page.locator("a.card").filter({ has: page.getByRole("heading", { name, exact: true }) });
        await agentCard.getByText("Personal Dot", { exact: true }).waitFor();
        assert.equal(await agentCard.getByText(/^personal-dot-/).count(), 0, "Dot cards should not expose the generated principal ID as the identity label");

        await page.setViewportSize({ width: 390, height: 844 });
        await page.goto(`${BASE}/dots`, { waitUntil: "networkidle" });
        await card().getByRole("button", { name: "Details", exact: true }).click();
        const mobileTabs = card().getByRole("tablist", { name: "Dot details", exact: true });
        await mobileTabs.getByRole("tab", { name: "Access", exact: true }).click();
        await remember.getByRole("checkbox").waitFor();
        assert.ok(
            await page.locator("main").evaluate((element) => element.scrollWidth <= element.clientWidth),
            "the Dot management page must not overflow a mobile viewport",
        );
    } finally {
        if (dotId) await context.request.put(`${BASE}/api/v0/dots/${dotId}/active`, { data: { active: false } });
        await context.close();
        await browser.close();
    }
});
