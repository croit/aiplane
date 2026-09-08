// Anonymous (no-session) browser flows: what a visitor who is not signed in
// sees on the gateway's HTML surface. Since the dashboard became the chat
// surface, "what they see" is the sign-in funnel — every protected page
// bounces to /login with a return_to, and /login is the one page that
// renders for them.

import { test, before, after } from "node:test";
import assert from "node:assert";

import { BASE, ensureDevFixture, gatewayIsUp, launchBrowser } from "./helpers.mjs";

let browser;

before(async () => {
    assert.ok(
        await gatewayIsUp(),
        `gateway is not reachable at ${BASE}; run \`mise run dev\` in another terminal`,
    );
    // The funnel only exists once setup has completed; on a fresh dev
    // database every page would 303 to /setup instead. Idempotent and
    // delete-free, so it cannot race the parallel test files.
    await ensureDevFixture();
    browser = await launchBrowser();
});

after(async () => {
    if (browser) await browser.close();
});

test("an anonymous visitor to / is funnelled to the sign-in page", async () => {
    const ctx = await browser.newContext({ colorScheme: "dark" });
    const page = await ctx.newPage();
    await page.goto(`${BASE}/`, { waitUntil: "domcontentloaded" });

    await page.waitForURL((u) => u.pathname === "/login", { timeout: 5000 });
    assert.match(page.url(), /[?&]return_to=%2F\b/, "the original destination rides along");
    await page.waitForSelector("text=Sign in to LLM Gateway", { timeout: 5000 });
    await ctx.close();
});

test("protected pages bounce anonymous visitors to sign-in", async () => {
    const ctx = await browser.newContext({ colorScheme: "dark" });
    const page = await ctx.newPage();
    for (const path of ["/tokens", "/chat"]) {
        await page.goto(`${BASE}${path}`, { waitUntil: "domcontentloaded" });
        await page.waitForURL((u) => u.pathname === "/login", { timeout: 5000 });
        assert.match(
            page.url(),
            new RegExp(`[?&]return_to=${encodeURIComponent(path).replaceAll("/", "%2F")}`),
            `${path} must be preserved as return_to`,
        );
    }
    await ctx.close();
});

test("/login posts to /auth/login via a form action", async () => {
    const ctx = await browser.newContext({ colorScheme: "dark" });
    const page = await ctx.newPage();
    await page.goto(`${BASE}/login`, { waitUntil: "networkidle" });
    // The page is intentionally a focused, standalone landing: a single
    // <form action="/auth/login" method="get"> wrapping the CTA button.
    const form = page.locator('form[action="/auth/login"]');
    await form.first().waitFor({ state: "visible", timeout: 5000 });
    await page.locator('button:has-text("Continue with OIDC")').waitFor({ state: "visible" });
    await ctx.close();
});
