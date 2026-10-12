import { test } from "node:test";
import assert from "node:assert/strict";
import { BASE, devSessionCookie, gatewayIsUp, launchBrowser } from "./helpers.mjs";

test("organization administration lists compact cards and edits in a modal", async () => {
    assert.ok(await gatewayIsUp(), `gateway is not reachable at ${BASE}`);
    const browser = await launchBrowser();
    let context;
    try {
        context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
        await context.addCookies([
            { name: "id", value: await devSessionCookie(), url: BASE },
            { name: "lang", value: "en", url: BASE },
        ]);
        const name = `Organization ${Date.now()}`;
        const created = await context.request.post(`${BASE}/api/v0/admin/organizations`, {
            data: { name, description: "First organization description", member_groups: [] },
        });
        assert.equal(created.status(), 201);
        const organization = (await created.json()).organization;

        const page = await context.newPage();
        await page.goto(`${BASE}/admin/organizations`, { waitUntil: "networkidle" });
        const card = page.locator("article.card").filter({ has: page.getByRole("heading", { name, exact: true }) });
        await card.getByText("First organization description", { exact: true }).waitFor();
        await card.getByText("No member groups are configured for this organization.", { exact: true }).waitFor();
        assert.equal(await card.getByRole("textbox").count(), 0, "organization forms stay closed in the list");

        await card.getByRole("button", { name: "Edit organization", exact: true }).click();
        const dialog = page.locator("dialog[open]");
        const updatedName = `${name} edited`;
        await dialog.getByRole("textbox", { name: "Name", exact: true }).fill(updatedName);
        const saved = page.waitForResponse((response) => response.request().method() === "PUT"
            && response.url() === `${BASE}/api/v0/admin/organizations/${encodeURIComponent(organization.id)}`);
        await dialog.getByRole("button", { name: "Save", exact: true }).click();
        assert.equal((await saved).status(), 200);
        await page.getByRole("status").getByText("Organization saved.", { exact: true }).waitFor();
        await page.getByRole("heading", { name: updatedName, exact: true }).waitFor();
        await page.waitForFunction(() => document.querySelectorAll("dialog[open]").length === 0);
        assert.equal(await dialog.count(), 0, "saving closes the edit modal");
    } finally {
        try { await context?.close(); }
        finally { await browser.close(); }
    }
});
