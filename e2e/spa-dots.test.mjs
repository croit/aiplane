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

    try {
        await page.goto(`${BASE}/dots`, { waitUntil: "networkidle" });
        await page.getByRole("heading", { name: "Personal Dots", exact: true }).waitFor();
        await page.getByRole("button", { name: "Create Dot", exact: true }).click();

        const createDialog = page.getByRole("dialog");
        await createDialog.getByRole("textbox", { name: "Name", exact: true }).fill(name);
        await createDialog.getByRole("textbox", { name: "Display name", exact: true }).fill(name);
        await createDialog.getByRole("button", { name: "Create", exact: true }).click();
        await page.getByRole("status").getByText("Dot created.").waitFor();
        await card().waitFor();

        const access = card().locator("details").filter({ has: page.getByText("Access and approvals", { exact: true }) });
        await access.locator("summary").click();
        const remember = access.locator("li").filter({ has: page.getByText("remember", { exact: true }) });
        await remember.getByRole("checkbox").waitFor();
        assert.equal(await access.evaluate((element) => element.open), true, "manual opening must survive lazy loading");
        assert.equal(await remember.getByRole("checkbox").isChecked(), true);
        await remember.getByRole("combobox", { name: "Approval rule", exact: true }).selectOption("always_ask");
        await access.locator(":scope > .collapse-content > .flex").getByRole("button", { name: "Save", exact: true }).click();
        await access.getByRole("status").getByText("Access and approval rules saved.", { exact: true }).waitFor();
        await access.getByRole("button", { name: "Reload access", exact: true }).click();
        await page.waitForFunction(() => {
            const row = Array.from(document.querySelectorAll("details li")).find((element) => element.querySelector("code")?.textContent === "remember");
            return row?.querySelector("select")?.value === "always_ask";
        });
        await remember.getByRole("checkbox").uncheck();
        await access.locator(":scope > .collapse-content > .flex").getByRole("button", { name: "Save", exact: true }).click();
        await access.getByRole("status").getByText("Access and approval rules saved.", { exact: true }).waitFor();
        await access.getByRole("button", { name: "Reload access", exact: true }).click();
        await page.waitForFunction(() => {
            const row = Array.from(document.querySelectorAll("details li")).find((element) => element.querySelector("code")?.textContent === "remember");
            return row?.querySelector("input[type=checkbox]")?.checked === false;
        });
        await remember.getByRole("checkbox").check();
        await remember.getByRole("combobox", { name: "Approval rule", exact: true }).selectOption("always_allow");
        await access.locator(":scope > .collapse-content > .flex").getByRole("button", { name: "Save", exact: true }).click();
        await access.getByRole("status").getByText("Access and approval rules saved.", { exact: true }).waitFor();

        await card().getByRole("button", { name: "Chat", exact: true }).click();
        await page.waitForURL(/\/chat\//);
        await page.goto(`${BASE}/dots`, { waitUntil: "networkidle" });
        await card().waitFor();

        const memory = card().locator("details").filter({ has: page.getByText("Dot memory", { exact: true }) });
        await memory.locator("summary").click();
        await memory.getByRole("button", { name: "Add", exact: true }).first().click();
        const memoryDialog = page.getByRole("dialog");
        await memoryDialog.getByRole("textbox", { name: "Content", exact: true }).fill("Keep project notes private to this Dot.");
        await memoryDialog.getByRole("button", { name: "Remember", exact: true }).click();
        const savedMemory = memory.getByRole("textbox", { name: "Content", exact: true });
        await savedMemory.waitFor();
        assert.equal(await savedMemory.inputValue(), "Keep project notes private to this Dot.");

        const activity = card().locator("details").filter({ has: page.getByText("Recent activity", { exact: true }) });
        await activity.locator("summary").click();
        await activity.getByText("Started a Dot chat.", { exact: true }).waitFor();

        await page.setViewportSize({ width: 390, height: 844 });
        await access.locator("summary").click();
        await remember.getByRole("checkbox").waitFor();
        assert.ok(
            await page.locator("main").evaluate((element) => element.scrollWidth <= element.clientWidth),
            "the Dot management page must not overflow a mobile viewport",
        );
    } finally {
        if (await card().count()) {
            await card().getByRole("button", { name: "Deactivate", exact: true }).click();
            await card().getByRole("button", { name: "Activate", exact: true }).waitFor();
        }
        await context.close();
        await browser.close();
    }
});
