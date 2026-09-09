// API-level checks that don't need a browser — just plain fetch against the
// running gateway. These are quick smoke tests for the public HTTP surface.

import { test, before } from "node:test";
import assert from "node:assert";

import { BASE, ensureDevFixture, gatewayIsUp } from "./helpers.mjs";

before(async () => {
    assert.ok(
        await gatewayIsUp(),
        `gateway is not reachable at ${BASE}; run \`mise run dev\` in another terminal`,
    );
    // /readyz answers 503 `setup_required` until setup completes; make that
    // deterministic instead of depending on file ordering. Idempotent and
    // delete-free, so it cannot race the parallel test files.
    await ensureDevFixture();
});

test("/healthz returns 200 ok", async () => {
    const r = await fetch(`${BASE}/healthz`);
    assert.equal(r.status, 200);
    assert.equal((await r.json()).status, "ok");
});

test("/readyz returns 200", async () => {
    const r = await fetch(`${BASE}/readyz`);
    assert.equal(r.status, 200);
});

test("/api/v0/me returns 401 with the error envelope when anonymous", async () => {
    const r = await fetch(`${BASE}/api/v0/me`);
    assert.equal(r.status, 401);
    const body = await r.json();
    assert.equal(body.error.code, "unauthorized");
    assert.equal(body.error.type, "unauthorized");
});

test("/api/v0/tokens returns 401 when anonymous", async () => {
    const r = await fetch(`${BASE}/api/v0/tokens`);
    assert.equal(r.status, 401);
});

test("/v1/chat/completions returns 401 with no bearer", async () => {
    const r = await fetch(`${BASE}/v1/chat/completions`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ model: "x", messages: [] }),
    });
    assert.equal(r.status, 401);
});

test("/v1/chat/completions returns 401 with a malformed bearer", async () => {
    const r = await fetch(`${BASE}/v1/chat/completions`, {
        method: "POST",
        headers: {
            "content-type": "application/json",
            authorization: "Bearer not-a-real-token",
        },
        body: JSON.stringify({ model: "x", messages: [] }),
    });
    assert.equal(r.status, 401);
});

// The catch-all serves the app shell only for paths the *client* router owns.
// `/this-route-does-not-exist` is not one, so nothing serves it and the status
// says so — while a real client route still gets the shell, and the API is
// never shadowed.
test("only the SPA's own routes get the app shell", async () => {
    // Served by nobody. A plain fetch (Accept: */*) gets the error envelope;
    // a browser (Accept: text/html) gets the shell so the client can render
    // its styled 404, but the status is 404 either way.
    const unknown = await fetch(`${BASE}/this-route-does-not-exist`);
    assert.equal(unknown.status, 404);
    assert.match(unknown.headers.get("content-type") ?? "", /json/);

    const asBrowser = await fetch(`${BASE}/this-route-does-not-exist`, {
        headers: { accept: "text/html" },
    });
    assert.equal(asBrowser.status, 404, "a browser still gets 404, not 200");
    assert.match(asBrowser.headers.get("content-type") ?? "", /text\/html/);

    // A real client route: the shell, with a 200.
    const clientRoute = await fetch(`${BASE}/chat`, {
        headers: { accept: "text/html" },
    });
    assert.equal(clientRoute.status, 200);
    assert.match(clientRoute.headers.get("content-type") ?? "", /text\/html/);

    // Server-owned paths that the old denylist missed entirely.
    for (const path of ["/healthzz", "/rag/typo", "/hooksfoo"]) {
        const r = await fetch(`${BASE}${path}`);
        assert.equal(r.status, 404, `${path} must not answer with the app shell`);
    }

    const api = await fetch(`${BASE}/api/v0/this-endpoint-does-not-exist`);
    assert.equal(api.status, 404, "the API must still 404 rather than return the shell");
});
