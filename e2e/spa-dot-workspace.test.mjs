import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs/promises";
import { BASE, devSessionCookie, gatewayIsUp, launchBrowser } from "./helpers.mjs";

const RUNNER = process.env.AIPLANE_WORKSPACE_FIXTURE_URL ?? "http://127.0.0.1:9099";

test("Personal Dot workspace persists files, respects availability and supports inactive inspection", async () => {
    assert.ok(await gatewayIsUp(), `gateway is not reachable at ${BASE}`);
    const browser = await launchBrowser();
    let context;
    let dotId;
    try {
        context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
        await context.addCookies([
            { name: "id", value: await devSessionCookie(), url: BASE },
            { name: "lang", value: "en", url: BASE },
        ]);
        async function control(body) {
            const response = await context.request.post(`${RUNNER}/__control`, { data: body });
            assert.equal(response.status(), 200);
        }
        await control({ available: true });
        const created = await context.request.post(`${BASE}/api/v0/dots`, {
            data: { name: `workspace-harald-${Date.now()}`, display: "Harald" },
        });
        assert.equal(created.status(), 201);
        const dot = (await created.json()).dot;
        dotId = dot.id;
        const workspace = `${BASE}/api/v0/dots/${dotId}/workspace`;
        const route = `/dots/${dotId}/workspace`;
        const page = await context.newPage();
        await page.goto(`${BASE}/agents/${dotId}`, { waitUntil: "networkidle" });
        const card = page.getByRole("region", { name: "Agent details", exact: true });
        await card.getByRole("tab", { name: "Files", exact: true }).click();
        await card.getByRole("link", { name: "Open workspace", exact: true }).click();
        await page.waitForURL((url) => url.pathname === route);
        await page.getByText(dot.display || dot.name, { exact: true }).waitFor();
        await page.getByText("This folder is empty.", { exact: true }).waitFor();
        await page.getByRole("button", { name: "Create folder", exact: true }).click();
        const dialog = page.getByRole("dialog");
        await dialog.getByRole("textbox", { name: "Folder name", exact: true }).fill("project");
        const folderResponse = page.waitForResponse((response) => response.request().method() === "POST" && response.url() === `${workspace}/folder`);
        await dialog.getByRole("button", { name: "Create folder", exact: true }).click();
        assert.equal((await folderResponse).status(), 204);
        await page.getByRole("link", { name: "project", exact: true }).click();
        await page.waitForURL((url) => url.searchParams.get("path") === "project");
        await page.getByText("This folder is empty.", { exact: true }).waitFor();

        const name = `reviewed-${"long-name-".repeat(10)}ß.txt`;
        const content = Buffer.from("A reviewed workspace lesson. UTF-8: ß\n", "utf8");
        await page.getByLabel("File", { exact: true }).setInputFiles({ name, mimeType: "text/plain", buffer: content });
        const uploadResponse = page.waitForResponse((response) => response.request().method() === "PUT" && new URL(response.url()).pathname === `${new URL(workspace).pathname}/file`);
        await page.getByRole("button", { name: "Upload file", exact: true }).click();
        assert.equal((await uploadResponse).status(), 204);
        await page.getByText(name, { exact: true }).waitFor();
        const listing = await context.request.get(`${workspace}?path=project`);
        assert.equal(listing.status(), 200);
        const metadata = await listing.json();
        assert.equal(metadata.dot_id, dotId);
        assert.equal(metadata.dot_name, dot.display || dot.name);
        assert.equal(metadata.available, true);
        assert.equal(metadata.busy, false);
        assert.deepEqual(metadata.entries.map(({ name, kind, size_bytes }) => ({ name, kind, size_bytes })), [{ name, kind: "file", size_bytes: content.length }]);
        const downloadPromise = page.waitForEvent("download");
        await page.getByRole("link", { name: "Download file", exact: true }).click();
        const downloaded = await downloadPromise;
        assert.equal(downloaded.suggestedFilename(), name);
        assert.deepEqual(await fs.readFile(await downloaded.path()), content);
        const response = await context.request.get(`${workspace}/file?${new URLSearchParams({ path: `project/${name}` })}`);
        assert.equal(response.status(), 200);
        assert.equal(response.headers()["x-content-type-options"], "nosniff");
        assert.match(response.headers()["content-disposition"], /^attachment;/);
        assert.deepEqual(await response.body(), content);

        await page.setViewportSize({ width: 390, height: 844 });
        assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true);
        await page.getByRole("button", { name: "Reload workspace", exact: true }).focus();
        await page.keyboard.press("Enter");
        await page.getByText(name, { exact: true }).waitFor();
        await control({ dot_id: dotId, busy: true });
        await page.getByRole("button", { name: "Reload workspace", exact: true }).click();
        await page.getByText("This agent is using its workspace. Files may change; upload, folder creation and deletion are available when it is idle.", { exact: true }).waitFor();
        assert.equal(await page.getByRole("button", { name: "Create folder", exact: true }).isDisabled(), true);
        assert.equal(await page.getByRole("button", { name: "Delete", exact: true }).isDisabled(), true);
        await control({ dot_id: dotId, busy: false });
        await page.getByRole("button", { name: "Reload workspace", exact: true }).click();
        await page.getByText("This agent is using its workspace. Files may change; upload, folder creation and deletion are available when it is idle.", { exact: true }).waitFor({ state: "hidden" });
        await page.getByRole("button", { name: "Delete", exact: true }).waitFor();
        await page.getByRole("button", { name: "Delete", exact: true }).isEnabled().then((enabled) => assert.equal(enabled, true));
        await page.getByRole("link", { name: "Workspace root", exact: true }).click();
        await page.waitForURL((url) => url.pathname === route && !url.searchParams.has("path"));
        await page.goBack();
        await page.waitForURL((url) => url.searchParams.get("path") === "project");
        await page.getByText(name, { exact: true }).waitFor();
        await page.goForward();
        await page.getByRole("link", { name: "project", exact: true }).waitFor();
        await page.getByRole("link", { name: "project", exact: true }).click();
        page.once("dialog", (confirmation) => {
            assert.ok(confirmation.message().includes(name));
            void confirmation.accept();
        });
        await page.getByRole("button", { name: "Delete", exact: true }).click();
        await page.getByText("This folder is empty.", { exact: true }).waitFor();
        await page.getByRole("link", { name: "Workspace root", exact: true }).click();
        page.once("dialog", (confirmation) => void confirmation.accept());
        await page.getByRole("button", { name: "Delete", exact: true }).click();
        await page.getByText("This folder is empty.", { exact: true }).waitFor();

        assert.equal((await context.request.put(`${BASE}/api/v0/dots/${dotId}/active`, { data: { active: false } })).status(), 200);
        await page.goto(`${BASE}/agents/${dotId}?tab=workspace`, { waitUntil: "networkidle" });
        await page.getByRole("region", { name: "Agent details", exact: true }).getByRole("link", { name: "Open workspace", exact: true }).click();
        await page.getByText("This folder is empty.", { exact: true }).waitFor();
        await control({ available: false });
        await page.getByRole("button", { name: "Reload workspace", exact: true }).click();
        await page.getByText("Persistent storage is unavailable. Ask your administrator to check the sandbox runner.", { exact: true }).waitFor();
        assert.equal(await page.getByRole("button", { name: "Upload file", exact: true }).count(), 0);
        await control({ available: true });
        await page.getByRole("button", { name: "Reload workspace", exact: true }).click();
        await page.getByText("This folder is empty.", { exact: true }).waitFor();
        await page.goto(`${BASE}/dots/00000000-0000-4000-8000-000000000099/workspace`, { waitUntil: "networkidle" });
        await page.getByRole("alert").waitFor();
        assert.equal(await page.getByText(dot.display || dot.name, { exact: true }).count(), 0);
        assert.equal(await page.getByRole("link", { name: "Workspace root", exact: true }).count(), 0);
    } finally {
        try {
            if (context) await context.request.post(`${RUNNER}/__control`, { data: { available: true, dot_id: dotId, busy: false } });
            if (context && dotId) await context.request.delete(`${BASE}/api/v0/agents/${dotId}`);
        } finally {
            await context?.close();
            await browser.close();
        }
    }
});
