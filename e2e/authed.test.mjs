// Authed browser flows on the app shell and the tokens surface. Each test
// seeds a fresh canonical fixture session via the debug-only
// /__dev/seed-session endpoint (compiled in by cfg(debug_assertions), never in
// release), so all tests are independent and don't depend on test ordering.
//
// The fixture: user alice@example.com (roles engineering + admin) with
// exactly three tokens — Local laptop (active), CI pipeline (revoked),
// Production API (active), newest first.
//
// Only this file may call /__dev/seed-session: it deletes alice's tokens to
// reset the canonical counts, which would race the other files (node --test
// runs test files in parallel). Everyone else uses the delete-free
// /__dev/session.

import { test, before, after } from "node:test";
import assert from "node:assert";

import { BASE, gatewayIsUp, launchBrowser } from "./helpers.mjs";

let browser;

before(async () => {
    assert.ok(
        await gatewayIsUp(),
        `gateway is not reachable at ${BASE}; run \`mise run dev\` in another terminal`,
    );
    // Pre-flight: confirm the dev seed endpoint exists (i.e. server is debug).
    // Success is the 303 sign-in redirect itself — `Response.ok` is false for
    // every 3xx, and a release build answers 404 instead.
    const probe = await fetch(`${BASE}/__dev/seed-session`, { redirect: "manual" });
    assert.ok(
        probe.status === 303,
        `\`/__dev/seed-session\` did not answer 303 (got ${probe.status}). Server must be a debug build (e.g. \`mise run dev\`).`,
    );
    browser = await launchBrowser();
});

after(async () => {
    if (browser) await browser.close();
});

/// Returns a Playwright context that's already authenticated as the fixture
/// user, with the DB wiped+reseeded to the canonical 3-token state.
async function seededContext() {
    const ctx = await browser.newContext({ colorScheme: "dark" });
    const page = await ctx.newPage();
    const resp = await page.goto(`${BASE}/__dev/seed-session`);
    assert.ok(resp && resp.ok(), `seed-session failed: ${resp?.status()}`);
    await page.close();
    return ctx;
}

test("the app shell signs in as the fixture user with her roles", async () => {
    const ctx = await seededContext();
    const page = await ctx.newPage();
    await page.goto(`${BASE}/tokens`, { waitUntil: "networkidle" });

    // The sidebar shows who is signed in…
    await page.waitForSelector("text=alice@example.com", { timeout: 5000 });
    // …the account card distils it (email + OIDC roles)…
    await page.waitForSelector("text=Signed in as alice@example.com", { timeout: 5000 });
    await page.waitForSelector("text=engineering, admin", { timeout: 5000 });
    // …and the primary navigation is present.
    for (const label of ["Chat", "Tokens", "Usage"]) {
        assert.equal(
            await page.locator(`#app-sidebar a.app-sidebar__nav-link:has-text("${label}")`).count(),
            1,
            `the sidebar must link to ${label}`,
        );
    }
    await ctx.close();
});

test("the tokens page renders the canonical three rows with correct status badges", async () => {
    const ctx = await seededContext();
    const page = await ctx.newPage();
    await page.goto(`${BASE}/tokens`, { waitUntil: "networkidle" });
    await page.waitForSelector("#token-list li", { timeout: 5000 });

    const rows = page.locator("#token-list li");
    assert.equal(await rows.count(), 3);
    for (const name of ["Local laptop", "CI pipeline", "Production API"]) {
        assert.equal(await page.locator(`#token-list li:has-text("${name}")`).count(), 1, name);
    }

    // The 2 active rows have an "active" badge; the revoked row has "revoked".
    const activeBadges = page.locator('#token-list li span.badge:has-text("active")');
    const revokedBadges = page.locator('#token-list li span.badge:has-text("revoked")');
    assert.equal(await activeBadges.count(), 2);
    assert.equal(await revokedBadges.count(), 1);
    await ctx.close();
});

test("creating a token surfaces a plaintext banner with gwk_ prefix and adds a row", async () => {
    const ctx = await seededContext();
    const page = await ctx.newPage();
    await page.goto(`${BASE}/tokens`, { waitUntil: "networkidle" });
    await page.waitForSelector("#token-list li");

    await page.locator("input#name").fill("e2e-test-token");
    await page.locator('button:has-text("Create token")').click();

    // Plaintext banner — shown exactly once, never again afterwards.
    const plaintext = page.locator("pre#minted-token-value");
    await plaintext.waitFor({ state: "visible", timeout: 5000 });
    await page.waitForSelector("text=Token created", { timeout: 5000 });
    const text = (await plaintext.textContent()) ?? "";
    assert.match(text.trim(), /^gwk_[0-9a-f]{64}$/);

    // List now has 4 rows, one carrying the name we typed.
    await page.waitForFunction(
        () => document.querySelectorAll("#token-list li").length === 4,
        null,
        { timeout: 5000 },
    );
    assert.equal(await page.locator('#token-list li:has-text("e2e-test-token")').count(), 1);
    await ctx.close();
});

test("an empty name creates nothing", async () => {
    const ctx = await seededContext();
    const page = await ctx.newPage();
    await page.goto(`${BASE}/tokens`, { waitUntil: "networkidle" });
    await page.waitForSelector("#token-list li");

    // Leave the name field blank, click Create. Whether the browser's native
    // `required` validation swallows the submit or the server rejects it,
    // the observable contract is the same: no row, no plaintext banner.
    await page.locator('button:has-text("Create token")').click();
    await page.waitForTimeout(500);

    assert.equal(await page.locator("#token-list li").count(), 3);
    assert.equal(
        await page.locator("pre#minted-token-value").count(),
        0,
        "no plaintext may ever be shown for a refused create",
    );
    await ctx.close();
});

test("revoking a token flips its row from active to revoked", async () => {
    const ctx = await seededContext();
    const page = await ctx.newPage();
    await page.goto(`${BASE}/tokens`, { waitUntil: "networkidle" });
    await page.waitForSelector("#token-list li");

    // The first (newest) row is "Local laptop"; its id is fixed by the
    // fixture, so the SSE row swap can be asserted precisely.
    await page.locator("#token-row-devseed-laptop button:has-text(\"Revoke\")").click();

    await page.locator("#token-row-devseed-laptop span.badge:has-text(\"revoked\")")
        .waitFor({ state: "visible", timeout: 5000 });
    assert.equal(
        await page.locator('#token-list li span.badge:has-text("active")').count(),
        1,
    );
    assert.equal(
        await page.locator('#token-list li span.badge:has-text("revoked")').count(),
        2,
    );
    await ctx.close();
});

test("signing out ends the session", async () => {
    const ctx = await seededContext();
    const page = await ctx.newPage();
    await page.goto(`${BASE}/tokens`, { waitUntil: "networkidle" });
    await page.waitForSelector("text=alice@example.com");

    // The sign-out button is a real form POST to /auth/logout; the redirect
    // chain then lands the now-anonymous visitor on the sign-in page.
    await page.locator('button[aria-label="Sign out"]').click();
    await page.waitForURL((u) => u.pathname === "/login", { timeout: 5000 });
    await page.waitForSelector("text=Sign in to LLM Gateway", { timeout: 5000 });
    await ctx.close();
});

test("the chat surface renders the composer and model picker", async () => {
    const ctx = await seededContext();
    const page = await ctx.newPage();
    // / 303s into the latest (here: fresh) conversation.
    await page.goto(`${BASE}/chat`, { waitUntil: "networkidle" });
    await page.waitForURL((u) => /^\/chat\/.+/.test(u.pathname), { timeout: 5000 });

    // `#message` — the composer; the page also carries a hidden edit
    // textarea (same name) inside each turn's edit affordance.
    assert.equal(await page.locator('textarea#message[name="message"]').count(), 1);
    // The model picker is a <select> when the gateway has chat models on
    // offer (dev-ui) and falls back to a free-text <input> when it has
    // none (a bare `mise run dev`) — both are "the model picker".
    assert.ok(
        (await page.locator('select[name="model"], input[name="model"]').count()) >= 1,
        "a model picker must be present (select when models are on offer, free-text input otherwise)",
    );
    assert.equal(await page.locator("button.chat-composer__send").count(), 1);
    await ctx.close();
});
