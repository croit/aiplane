// The SPA chat round trip (issue #22, phase 2): the SvelteKit surface at
// The SPA talking to the /api/v0/chat JSON API end to end — create a
// conversation, submit a turn, and watch the reply arrive live over the
// JSON-SSE event stream.
//
// Needs a gateway that (a) serves the SPA (`GATEWAY_STATIC_DIR=…`) and
// (b) has a chat upstream. `mise run dev` satisfies (a) but has no pools,
// so a submit there finalises as `errored` — this file detects that and
// skips with a pointer at the right server instead of failing. The full
// flow runs against the dev-ui stub:
//
//     GATEWAY_STATIC_DIR=target/frontend/build mise run dev-ui
//     mise run e2e

import { test, before, after } from "node:test";
import assert from "node:assert";

import { BASE, devSessionCookie, gatewayIsUp, launchBrowser } from "./helpers.mjs";

let browser;

async function conversationUrl(cookie, title) {
    const response = await fetch(`${BASE}/api/v0/chat/sessions`, { headers: { cookie } });
    assert.equal(response.status, 200);
    const { sessions } = await response.json();
    const session = sessions.find((candidate) => candidate.title === title);
    assert.ok(session, `missing seeded conversation: ${title}`);
    return `/chat/${session.id}`;
}

async function chooseSearchable(page, label, query, optionName = query) {
    const picker = page.getByRole("combobox", { name: label, exact: true });
    await picker.click();
    const dropdown = picker.locator("..");
    await dropdown.getByPlaceholder("Search options…", { exact: true }).fill(query);
    // Prefer the option whose visible label is exactly this text — the
    // accessible name concatenates description and badges, so "demo-model"
    // would otherwise also match "demo-model-pro". Fall back to the
    // accessible name for labels that span several text nodes ("v1 Complete
    // feature parity"), which no single text node carries.
    const byLabel = dropdown.getByRole("option").filter({ has: page.getByText(optionName, { exact: true }) });
    const target = (await byLabel.count()) ? byLabel.first() : dropdown.getByRole("option", { name: optionName }).first();
    await target.click();
}

before(async () => {
    assert.ok(
        await gatewayIsUp(),
        `gateway is not reachable at ${BASE}; run \`GATEWAY dev server\` in another terminal`,
    );
    const probe = await fetch(`${BASE}/`);
    assert.equal(
        probe.status,
        200,
        `the SPA is not served at ${BASE} (got ${probe.status}) — run the server with GATEWAY_STATIC_DIR=target/frontend/build.`,
    );
    browser = await launchBrowser();
});

after(async () => {
    if (browser) await browser.close();
});

test("chat entry resolves directly and keeps sidebar pin and delete actions", async () => {
    const cookieValue = await devSessionCookie();
    const ctx = await browser.newContext();
    await ctx.addCookies([{ name: "id", value: cookieValue, url: BASE }]);
    const page = await ctx.newPage();

    await page.goto(`${BASE}/`, { waitUntil: "domcontentloaded" });
    await page.waitForURL((url) => /\/chat\/[^/]+$/.test(url.pathname), { timeout: 5000 });
    const landing = page.url();
    await page.getByRole("button", { name: "Start a new conversation", exact: true }).click();
    await page.waitForURL((url) => /\/chat\/[^/]+$/.test(url.pathname) && url.href !== landing, { timeout: 5000 });
    const id = page.url().split("/").at(-1);
    const row = page.locator(`a[href$="/chat/${id}"]`).last().locator("..");
    await row.waitFor();

    const pinned = page.waitForResponse((response) => response.url().endsWith(`/api/v0/chat/sessions/${id}/pin`));
    await row.getByRole("button", { name: "Pin conversation", exact: true }).click();
    assert.equal((await pinned).status(), 200);
    await row.getByRole("button", { name: "Unpin conversation", exact: true }).waitFor();

    const removed = page.waitForResponse((response) => response.url().endsWith(`/api/v0/chat/sessions/${id}`) && response.request().method() === "DELETE");
    await row.getByRole("button", { name: "Delete conversation", exact: true }).click();
    assert.equal((await removed).status(), 204);
    await page.waitForURL((url) => /\/chat\/[^/]+$/.test(url.pathname) && !url.pathname.endsWith(`/${id}`), { timeout: 5000 });
    assert.equal(await page.locator(`a[href$="/chat/${id}"]`).count(), 0);
    await ctx.close();
});

/** Whether the gateway can actually route a chat turn to an upstream. */
async function chatUpstreamAvailable(cookie) {
    const created = await fetch(`${BASE}/api/v0/chat/sessions`, {
        method: "POST",
        headers: { cookie },
    });
    if (!created.ok) throw new Error(`creating a probe session failed: ${created.status}`);
    const { session } = await created.json();
    await fetch(`${BASE}/api/v0/chat/sessions/${session.id}/messages`, {
        method: "POST",
        headers: { cookie, "content-type": "application/json" },
        body: JSON.stringify({ model: "demo-model", message: "probe" }),
    });
    for (let i = 0; i < 40; i++) {
        const snap = await fetch(`${BASE}/api/v0/chat/sessions/${session.id}`, {
            headers: { cookie },
        });
        const { turns } = await snap.json();
        const assistant = turns.find((t) => t.turn.role === "assistant");
        if (assistant && assistant.turn.status !== "in_progress") {
            return { ok: assistant.turn.status === "completed", session, assistant };
        }
        await new Promise((r) => setTimeout(r, 100));
    }
    return { ok: false, session, assistant: null };
}

test("a turn streams into the SPA over the JSON event protocol", async (t) => {
    const cookieValue = await devSessionCookie();
    const cookie = `id=${cookieValue}`;

    const upstream = await chatUpstreamAvailable(cookie);
    if (!upstream.ok) {
        t.skip(
            "no chat upstream configured — run the gateway as " +
                "`GATEWAY_STATIC_DIR=target/frontend/build mise run dev-ui` for the full flow",
        );
        return;
    }

    const ctx = await browser.newContext();
    await ctx.addCookies([{ name: "id", value: cookieValue, url: BASE }]);
    const page = await ctx.newPage();

    await page.goto(`${BASE}/chat`, { waitUntil: "domcontentloaded" });
    await page.waitForURL((u) => /\/chat\/.+/.test(u.pathname), { timeout: 5000 });
    assert.equal(await page.title(), "Chat — LLM Gateway");
    const landing = page.url();
    await page.getByRole("button", { name: "Start a new conversation", exact: true }).click();
    await page.waitForURL((url) => /\/chat\/.+/.test(url.pathname) && url.href !== landing, { timeout: 5000 });

    // With SSR off, the composer only exists once Svelte has booted —
    // waiting for it also means its listeners are attached, so the Send
    // click can't race hydration under a loaded test run.
    await page.locator("textarea").waitFor({ state: "visible", timeout: 5000 });

    // Compose: model + message, send. Offered models use the shared searchable
    // picker; a gateway without pools falls back to free text.
    const picker = page.getByLabel("Chat model");
    if (await page.getByRole("combobox", { name: "Chat model", exact: true }).count()) {
        await chooseSearchable(page, "Chat model", "demo-model");
    } else {
        await picker.fill("demo-model");
    }
    await page.locator('textarea').fill("hello from the e2e suite");
    await page.getByRole("button", { name: "Send", exact: true }).click();

    // The user bubble appears immediately…
    await page.waitForSelector("text=hello from the e2e suite", { timeout: 5000 });
    // …then the reply streams in over the event protocol and the composer
    // unlocks again once the turn finalises.
    await page.waitForSelector("text=How can I help?", { timeout: 10_000 });
    await page.getByRole("button", { name: "Send", exact: true }).waitFor({ state: "visible", timeout: 10_000 });
    await page.waitForFunction(() => document.title !== "Chat — LLM Gateway", undefined, { timeout: 5000 });
    assert.equal(await page.title(), "Hi! How can I help — LLM Gateway");
    await ctx.close();
});

test("the voice-mode modal opens with its tap-to-talk control", async () => {
    const cookieValue = await devSessionCookie();
    const ctx = await browser.newContext();
    await ctx.addCookies([{ name: "id", value: cookieValue, url: BASE }]);
    const page = await ctx.newPage();
    await page.goto(`${BASE}/chat`, { waitUntil: "domcontentloaded" });
    await page.waitForURL((u) => /\/chat\/.+/.test(u.pathname), { timeout: 5000 });
    const landing = page.url();
    await page.getByRole("button", { name: "Start a new conversation", exact: true }).click();
    await page.waitForURL((url) => /\/chat\/.+/.test(url.pathname) && url.href !== landing, { timeout: 5000 });
    await page.locator("textarea").waitFor();

    assert.equal(await page.getByRole("link", { name: "All chats", exact: true }).count(), 0);
    await page.getByRole("heading", { name: "New conversation", exact: true }).waitFor();
    const chatModel = page.getByLabel("Chat model", { exact: true });
    const voiceModel = page.getByLabel("Voice model", { exact: true });
    await chatModel.waitFor();
    await voiceModel.waitFor();
    assert.ok(
        (await chatModel.boundingBox()).y < (await page.locator("textarea").boundingBox()).y,
        "the chat model belongs in the page header, above the composer",
    );
    await page.getByRole("button", { name: "Record voice message", exact: true }).waitFor();

    // `voice-toggle-title` in the Fluent corpus — the composer's controls are
    // translated, so the label is the catalog's wording, not a literal we get
    // to choose here. The browser is pinned to en-US in `launchBrowser`.
    await page.locator('button[aria-label="Start voice conversation"]').click();
    // The modal renders with its state control and the idle caption. (The
    // actual mic capture can't run headless — capability errors surface as
    // the modal's note, which is itself the wiring under test.)
    await page.locator("dialog.modal-open").waitFor({ timeout: 5000 });
    await page.waitForSelector("text=Tap to talk", { timeout: 5000 });
    await page.locator('dialog.modal-open .modal-action button:has-text("Close")').click();
    await page.locator("dialog.modal-open").waitFor({ state: "detached", timeout: 5000 });
    await ctx.close();
});

test("conversation tools use a responsive full-screen selector", async () => {
    const cookieValue = await devSessionCookie();
    const ctx = await browser.newContext({ viewport: { width: 1400, height: 900 } });
    await ctx.addCookies([{ name: "id", value: cookieValue, url: BASE }]);
    const page = await ctx.newPage();
    await page.goto(`${BASE}/chat`, { waitUntil: "domcontentloaded" });
    await page.waitForURL((url) => /\/chat\/[^/]+$/.test(url.pathname), { timeout: 5000 });
    await page.getByRole("button", { name: "Tools", exact: true }).click();

    const dialog = page.getByRole("dialog");
    const desktopBox = await dialog.boundingBox();
    assert.deepEqual(desktopBox, { x: 0, y: 0, width: 1400, height: 900 });
    await dialog.getByRole("button", { name: /^All tools/ }).waitFor();
    assert.equal(await dialog.getByRole("combobox", { name: "Tool category" }).isVisible(), false);

    await page.setViewportSize({ width: 390, height: 844 });
    const mobileBox = await dialog.boundingBox();
    assert.deepEqual(mobileBox, { x: 0, y: 0, width: 390, height: 844 });
    await dialog.getByRole("combobox", { name: "Tool category" }).waitFor();
    assert.equal(await dialog.getByRole("button", { name: /^All tools/ }).isVisible(), false);

    await page.getByPlaceholder("Search tools…").fill("web search");

    const row = dialog.locator("li").filter({ has: page.getByText("Web search", { exact: true }) });
    const saved = page.waitForResponse(
        (response) => response.url().endsWith("/capabilities") && response.request().method() === "POST",
    );
    await row.getByRole("button", { name: "On — always available to the assistant", exact: true }).click();
    assert.equal((await saved).status(), 200);
    await page.setViewportSize({ width: 1400, height: 900 });
    await page.getByRole("button", { name: "Close", exact: true }).click();
    await page.getByRole("button", { name: /Web search/ }).waitFor();
    await ctx.close();
});

// A document that changes while the conversation is open must show up in the
// canvas without a reload. The panel used to fetch the open document once and
// keep it: every version the assistant appended mid-turn stayed invisible
// until the page was refreshed, so the reader was looking at v4 of a document
// the model had already grown to v9.
//
// The hand-edit route stands in for the assistant's write here (it appends a
// version through the same store), and the end of a turn stands in for the
// mid-turn `sidebar_changed` — both cues run the same client refresh.
test("a document version written behind the panel's back shows up without a reload", async () => {
    const cookie = process.env.GATEWAY_SESSION_COOKIE;
    assert.ok(cookie, "set GATEWAY_SESSION_COOKIE to the dev-ui seed cookie");
    const cookieValue = cookie.startsWith("id=") ? cookie.slice("id=".length) : cookie;
    const ctx = await browser.newContext({ viewport: { width: 1400, height: 950 } });
    await ctx.addCookies([{ name: "id", value: cookieValue, url: BASE }]);
    const page = await ctx.newPage();

    const workspaceUrl = await conversationUrl(`id=${cookieValue}`, "Draft a project brief");
    await page.goto(`${BASE}${workspaceUrl}`, { waitUntil: "domcontentloaded" });
    const canvas = page.getByRole("complementary", { name: "Canvas", exact: true });
    await canvas.getByText("Release criteria", { exact: true }).waitFor();
    const sessionId = page.url().split("/").at(-1);

    // Unique per run: a save whose content matches the head mints no version,
    // and this file is run repeatedly against the same seeded fixture.
    const proof = `Live refresh proof ${Date.now()}`;
    const saved = await page.evaluate(async ([id, line]) => {
        const listed = await fetch(`/api/v0/chat/sessions/${id}/documents`);
        const { documents } = await listed.json();
        const documentId = documents[0].id;
        const read = await fetch(`/api/v0/chat/sessions/${id}/documents/${documentId}`);
        const opened = await read.json();
        const response = await fetch(`/api/v0/chat/sessions/${id}/documents/${documentId}`, {
            method: "PUT",
            headers: { "content-type": "application/json" },
            body: JSON.stringify({ content: `${opened.version.content}\n\n## ${line}` }),
        });
        return { status: response.status, before: opened.document.current_ver, after: (await response.json()).document.current_ver };
    }, [sessionId, proof]);
    assert.equal(saved.status, 200);
    assert.equal(saved.after, saved.before + 1, "the hand edit must mint a new version");

    // Nothing polls, so the panel is still showing the version it opened.
    assert.equal(await canvas.getByText(proof, { exact: true }).count(), 0);

    // Finish a turn in this conversation — the cue the SPA gets in production
    // when the assistant writes to the canvas.
    if (await page.getByRole("combobox", { name: "Chat model", exact: true }).count()) {
        await chooseSearchable(page, "Chat model", "demo-model");
    }
    await page.locator("textarea").fill("carry on");
    await page.getByRole("button", { name: "Send", exact: true }).click();

    // No reload anywhere in here: the panel has to re-read the document itself.
    await canvas.getByText(proof, { exact: true }).waitFor({ timeout: 15_000 });
    await page.getByRole("combobox", { name: "Version", exact: true }).getByText(`v${saved.after}`, { exact: true }).waitFor({ timeout: 5000 });
    await ctx.close();
});

test("the transcript keeps edit, retry, code, tool-detail, and canvas workflows", async () => {
    // Every other suite reads this variable as the bare cookie value; accept
    // either spelling so one export drives the whole run.
    const cookie = process.env.GATEWAY_SESSION_COOKIE;
    assert.ok(cookie, "set GATEWAY_SESSION_COOKIE to the dev-ui seed cookie");
    const cookieValue = cookie.startsWith("id=") ? cookie.slice("id=".length) : cookie;
    const ctx = await browser.newContext({ viewport: { width: 1400, height: 950 } });
    await ctx.addCookies([{ name: "id", value: cookieValue, url: BASE }]);
    const page = await ctx.newPage();

    // `conversationUrl` sends this as a Cookie header, so it needs the name.
    const transcriptUrl = await conversationUrl(`id=${cookieValue}`, "Enabling gzip in nginx");
    await page.goto(`${BASE}${transcriptUrl}`, { waitUntil: "domcontentloaded" });
    await page.getByRole("separator", { name: "Earlier messages condensed to save context", exact: true }).waitFor();
    await page.getByRole("button", { name: "Copy code", exact: true }).waitFor();
    await page.locator("details").filter({ hasText: "search_web" }).locator("summary").click();
    await page.getByText("Input", { exact: true }).first().waitFor();
    await page.getByRole("button", { name: /Edit/ }).click();
    await page.getByRole("dialog", { name: "Edit your message:" }).waitFor();
    await page.getByRole("dialog", { name: "Edit your message:" }).getByRole("button", { name: "Cancel", exact: true }).first().click();
    page.once("dialog", (dialog) => dialog.dismiss());
    await page.getByRole("button", { name: /Retry/ }).click();

    const workspaceUrl = await conversationUrl(`id=${cookieValue}`, "Draft a project brief");
    await page.goto(`${BASE}${workspaceUrl}`, { waitUntil: "domcontentloaded" });
    await page.getByRole("button", { name: "Document", exact: true }).waitFor();
    await page.getByText("Ship a reliable, accessible gateway experience.", { exact: true }).waitFor();

    const workspace = page.locator("[data-chat-workspace]");
    const transcript = page.locator("[data-chat-transcript]");
    const composer = page.locator("[data-chat-composer]");
    const workspaceBox = await workspace.boundingBox();
    const composerBox = await composer.boundingBox();
    assert.ok(workspaceBox && composerBox);
    assert.ok(composerBox.y + composerBox.height <= 950, "the composer must be visible without scrolling the page");
    assert.ok(Math.abs(composerBox.width - workspaceBox.width) <= 1, "the composer must span chat and canvas");
    const composerY = composerBox.y;
    await transcript.evaluate((element) => { element.scrollTop = element.scrollHeight; });
    assert.equal((await composer.boundingBox()).y, composerY, "transcript scrolling must not move the composer");

    await chooseSearchable(page, "Version", "v1", "v1");
    await page.getByText("Complete feature parity", { exact: true }).waitFor();
    await chooseSearchable(page, "Version", "v2", "v2");
    await page.getByText("Release criteria", { exact: true }).waitFor();

    // A canvas narrower than the option popup. The popup has to leave the
    // canvas's scroll box for the top layer: clipped to the panel, its rows
    // were cut off at the panel edge — what showed looked like an empty box,
    // and every click in it went to the chat behind.
    await page.evaluate(() => localStorage.setItem("chat-canvas-width", "340"));
    await page.reload({ waitUntil: "domcontentloaded" });
    await page.getByText("Release criteria", { exact: true }).waitFor();
    await page.getByRole("combobox", { name: "Version", exact: true }).click();
    const popup = await page.evaluate(() => {
        const trigger = [...document.querySelectorAll('[role="combobox"]')].find((element) => element.getAttribute("aria-label") === "Version");
        const listbox = document.getElementById(`${trigger.id}-listbox`);
        const panel = listbox.closest("[popover]");
        const option = listbox.querySelector('[role="option"]');
        const box = option.getBoundingClientRect();
        const hit = document.elementFromPoint(box.left + box.width / 2, box.top + box.height / 2);
        const panelBox = panel?.getBoundingClientRect();
        return {
            inTopLayer: panel?.matches(":popover-open") ?? false,
            reachable: option.contains(hit),
            onScreen: !!panelBox && panelBox.left >= 0 && panelBox.top >= 0 && panelBox.right <= window.innerWidth && panelBox.bottom <= window.innerHeight,
        };
    });
    assert.ok(popup.inTopLayer, "the option popup must open in the top layer");
    assert.ok(popup.reachable, "a click in the middle of an option must reach the option, not the chat behind the canvas");
    assert.ok(popup.onScreen, "the option popup must stay inside the viewport");
    await page.keyboard.press("Escape");

    const desktopCanvas = page.getByRole("complementary", { name: "Canvas", exact: true });
    const initialCanvasBox = await desktopCanvas.boundingBox();
    const resizeHandle = page.getByRole("slider", { name: "Resize canvas", exact: true });
    const resizeBox = await resizeHandle.boundingBox();
    assert.ok(initialCanvasBox && resizeBox);
    await page.mouse.move(resizeBox.x + resizeBox.width / 2, resizeBox.y + 100);
    await page.mouse.down();
    await page.mouse.move(resizeBox.x - 96, resizeBox.y + 100);
    await page.mouse.up();
    const resizedCanvasBox = await desktopCanvas.boundingBox();
    assert.ok(resizedCanvasBox && resizedCanvasBox.width >= initialCanvasBox.width + 80);

    await page.getByRole("button", { name: /Assets/ }).click();
    await page.getByRole("complementary", { name: "Canvas", exact: true }).getByText("project-brief.md", { exact: true }).waitFor();

    await page.setViewportSize({ width: 390, height: 844 });
    await page.getByRole("button", { name: "Close canvas", exact: true }).click();
    await page.getByRole("button", { name: "Show / hide the document canvas", exact: true }).click();
    const canvas = page.getByRole("complementary", { name: "Canvas", exact: true });
    const box = await canvas.boundingBox();
    assert.ok(box && box.x >= 0 && box.x + box.width <= 390, `canvas overflowed mobile viewport: ${JSON.stringify(box)}`);
    await ctx.close();
});
