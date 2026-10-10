import { test } from "node:test";
import assert from "node:assert/strict";

import { devSessionCookie, gatewayIsUp, launchBrowser, BASE } from "./helpers.mjs";

test("personal Dot lifecycle, private chat, memory, activity, and mobile layout", async () => {
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
