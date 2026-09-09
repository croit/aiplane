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

    // The SPA home funnels into the chat list; start a fresh conversation.
    await page.goto(`${BASE}/chat`, { waitUntil: "networkidle" });
    await page.locator('button:has-text("New chat")').click();
    await page.waitForURL((u) => /\/chat\/.+/.test(u.pathname), { timeout: 5000 });

    // With SSR off, the composer only exists once Svelte has booted —
    // waiting for it also means its listeners are attached, so the Send
    // click can't race hydration under a loaded test run.
    await page.locator("textarea").waitFor({ state: "visible", timeout: 5000 });

    // Compose: model + message, send. The picker is a <select> when the
    // gateway offers models, free-text otherwise.
    const picker = page.locator('[aria-label="Model"]');
    if ((await picker.evaluate((el) => el.tagName)) === "SELECT") {
        await picker.selectOption("demo-model");
    } else {
        await picker.fill("demo-model");
    }
    await page.locator('textarea').fill("hello from the e2e suite");
    await page.locator('button:has-text("Send")').click();

    // The user bubble appears immediately…
    await page.waitForSelector("text=hello from the e2e suite", { timeout: 5000 });
    // …then the reply streams in over the event protocol and the composer
    // unlocks again once the turn finalises.
    await page.waitForSelector("text=How can I help?", { timeout: 10_000 });
    await page
        .locator('button:has-text("Send")')
        .waitFor({ state: "visible", timeout: 10_000 });
    await ctx.close();
});

test("the voice-mode modal opens with its tap-to-talk control", async () => {
    const cookieValue = await devSessionCookie();
    const ctx = await browser.newContext();
    await ctx.addCookies([{ name: "id", value: cookieValue, url: BASE }]);
    const page = await ctx.newPage();
    await page.goto(`${BASE}/chat`, { waitUntil: "networkidle" });
    await page.locator('button:has-text("New chat")').click();
    await page.waitForURL((u) => /\/chat\/.+/.test(u.pathname), { timeout: 5000 });
    await page.locator("textarea").waitFor();

    await page.locator('button[aria-label="Voice mode"]').click();
    // The modal renders with its state control and the idle caption. (The
    // actual mic capture can't run headless — capability errors surface as
    // the modal's note, which is itself the wiring under test.)
    await page.locator("dialog.modal-open").waitFor({ timeout: 5000 });
    await page.waitForSelector("text=Tap to talk", { timeout: 5000 });
    await page.locator('dialog.modal-open .modal-action button:has-text("Close")').click();
    await page.locator("dialog.modal-open").waitFor({ state: "detached", timeout: 5000 });
    await ctx.close();
});
