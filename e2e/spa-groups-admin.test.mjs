import { test } from "node:test";
import assert from "node:assert/strict";
import { BASE, devSessionCookie, gatewayIsUp, launchBrowser } from "./helpers.mjs";

test("group administration creates and edits groups in modals", async () => {
    assert.ok(await gatewayIsUp(), `gateway is not reachable at ${BASE}`);
    const browser = await launchBrowser();
    let context;
    let name;
    try {
        context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
        await context.addCookies([
            { name: "id", value: await devSessionCookie(), url: BASE },
            { name: "lang", value: "en", url: BASE },
        ]);
        const page = await context.newPage();
        name = `ui-group-${Date.now()}`;
        await page.goto(`${BASE}/admin/groups?tab=groups`, { waitUntil: "networkidle" });
        await page.getByRole("heading", { name: "Groups", exact: true }).waitFor();
        assert.equal(await page.getByRole("textbox", { name: "Name", exact: true }).count(), 0,
            "the create form is not shown inline");

        await page.getByRole("button", { name: "Add group", exact: true }).click();
        const createDialog = page.locator("dialog[open]");
        await createDialog.getByRole("textbox", { name: "Name", exact: true }).fill(name);
        await createDialog.getByRole("textbox", { name: "Description", exact: true }).fill("Initial description");
        const created = page.waitForResponse((response) => response.request().method() === "PUT"
            && response.url() === `${BASE}/api/v0/admin/groups`);
        await createDialog.getByRole("button", { name: "Save", exact: true }).click();
        assert.equal((await created).status(), 200);
        await page.waitForFunction(() => document.querySelectorAll("dialog[open]").length === 0);

        const card = page.locator("article.card").filter({ has: page.getByRole("heading", { name, exact: true }) });
        await card.getByText("Initial description", { exact: true }).waitFor();
        assert.equal(await card.getByRole("textbox").count(), 0, "group forms stay closed in the list");
        await card.getByRole("button", { name: "Edit", exact: true }).click();
        const editDialog = page.locator("dialog[open]");
        await editDialog.getByRole("textbox", { name: "Description", exact: true }).fill("Updated description");
        const updated = page.waitForResponse((response) => response.request().method() === "PUT"
            && response.url() === `${BASE}/api/v0/admin/groups`);
        await editDialog.getByRole("button", { name: "Save", exact: true }).click();
        assert.equal((await updated).status(), 200);
        await page.waitForFunction(() => document.querySelectorAll("dialog[open]").length === 0);
        await card.getByText("Updated description", { exact: true }).waitFor();
    } finally {
        try {
            if (context && name) {
                await context.request.delete(`${BASE}/api/v0/admin/groups/${encodeURIComponent(name)}`).catch(() => {});
            }
        } finally {
            await context?.close();
            await browser.close();
        }
    }
});
