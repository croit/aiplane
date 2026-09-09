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

// An unknown path belongs to the SPA, not to the server: the client router
// decides whether it is a real route or a typo, so the server hands back the
// app shell. What must NOT happen is the catch-all swallowing the API, so the
// assertion that carries weight is the second one.
test("an unknown route serves the app, but does not shadow the API", async () => {
    const r = await fetch(`${BASE}/this-route-does-not-exist`);
    assert.equal(r.status, 200);
    assert.match(r.headers.get("content-type") ?? "", /text\/html/);

    const api = await fetch(`${BASE}/api/v0/this-endpoint-does-not-exist`);
    assert.equal(api.status, 404, "the API must still 404 rather than return the shell");
});
