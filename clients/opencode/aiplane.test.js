// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

import assert from "node:assert/strict"
import { createServer } from "node:http"
import { test } from "node:test"

import { chatModels, modelInfo, plugin } from "./aiplane.js"

const TOKEN = "gwk_test"

// `models` is the listing for TOKEN, or a map from token to listing; a
// listing may carry `delayMs` to answer late.
async function gateway(models) {
  const byToken = Array.isArray(models) ? { [TOKEN]: { data: models } } : models
  const server = createServer((req, res) => {
    if (req.url !== "/v1/models") return res.writeHead(404).end()
    const listing = byToken[req.headers.authorization?.replace(/^Bearer /, "")]
    if (!listing) {
      return res.writeHead(401, { "content-type": "application/json" }).end('{"error":{"message":"invalid token"}}')
    }
    setTimeout(
      () => res.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify({ object: "list", data: listing.data })),
      listing.delayMs ?? 0,
    )
  })
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve))
  return { url: `http://127.0.0.1:${server.address().port}`, close: () => server.close() }
}

// A stand-in for OpenCode's plugin host: the real one only runs inside the
// OpenCode service. It keeps the registered transforms and replays them on
// reload the way the host does, and lets a test switch the stored credential.
function host(credential, { failNextResolve = false } = {}) {
  const integrations = new Map()
  const providerTransforms = []
  const listeners = []
  const reloads = []
  const state = { credential, providers: [], failNextResolve, reloadCount: 0 }
  const ctx = {
    integration: {
      transform: async (fn) => {
        fn({
          update: (id, update) => {
            const current = integrations.get(id) ?? { ref: { id, name: id }, methods: [] }
            integrations.set(id, current)
            update(current.ref)
          },
          method: {
            update: ({ integrationID, method }) => integrations.get(integrationID).methods.push(method),
          },
        })
      },
      connection: {
        active: async (id) => (id === "aiplane" && state.credential ? { integrationID: id, credentialID: "c1" } : undefined),
        resolve: async () => {
          if (state.failNextResolve) {
            state.failNextResolve = false
            throw new Error("credential store unavailable")
          }
          return state.credential
        },
      },
    },
    provider: {
      transform: async (fn) => providerTransforms.push(fn),
      reload: async () => {
        state.reloadCount++
        state.providers = []
        providerTransforms.forEach((fn) => fn({ add: (record) => state.providers.push(record) }))
        reloads.splice(0).forEach((done) => done())
      },
    },
    event: {
      subscribe: ({ signal }) => ({
        async *[Symbol.asyncIterator]() {
          while (!signal.aborted) {
            const event = await new Promise((resolve) => {
              listeners.push(resolve)
              signal.addEventListener("abort", () => resolve(undefined), { once: true })
            })
            if (event) yield event
          }
        },
      }),
    },
  }
  const nextReload = () => new Promise((resolve) => reloads.push(resolve))
  const switchTo = async (next) => {
    const reloaded = nextReload()
    state.credential = next
    listeners.splice(0).forEach((deliver) => deliver({ type: "credential.switched", data: { integrationID: "aiplane" } }))
    let timer
    const timeout = new Promise((_, reject) => {
      timer = setTimeout(() => reject(new Error("the plugin did not reload its models after the credential switch")), 2000)
    })
    await Promise.race([reloaded, timeout]).finally(() => clearTimeout(timer))
  }
  return { ctx, state, integrations, switchTo, nextReload }
}

test("a model the gateway describes keeps every reported fact", () => {
  const info = modelInfo({
    id: "Qwen/Qwen3.6",
    kind: "chat",
    context_window: 131072,
    max_output_tokens: 8192,
    capabilities: { vision: false, tools: true },
  })
  assert.equal(info.id, "Qwen/Qwen3.6")
  assert.equal(info.providerID, "aiplane")
  assert.deepEqual(info.limit, { context: 131072, output: 8192 })
  assert.deepEqual(info.capabilities, { tools: true, input: ["text"], output: ["text"] })
})

test("an output cap never fills the window", () => {
  assert.deepEqual(modelInfo({ id: "small", context_window: 32768 }).limit, { context: 32768, output: 16384 })
  assert.deepEqual(modelInfo({ id: "unknown" }).limit, { context: 200000, output: 32000 })
  assert.deepEqual(
    modelInfo({ id: "capped", context_window: 32768, max_output_tokens: 32768 }).limit,
    { context: 32768, output: 16384 },
  )
})

test("a capability is claimed only when the gateway reports it", () => {
  assert.deepEqual(modelInfo({ id: "unknown" }).capabilities.input, ["text"])
  assert.deepEqual(modelInfo({ id: "seeing", capabilities: { vision: true } }).capabilities.input, ["text", "image"])
  assert.equal(modelInfo({ id: "no-tools", capabilities: { tools: false } }).capabilities.tools, false)
})

test("only chat models are offered to OpenCode", async () => {
  const gw = await gateway([
    { id: "chat-a", kind: "chat", context_window: 65536 },
    { id: "whisper", kind: "transcription" },
    { id: "embed", kind: "embedding" },
  ])
  try {
    const models = await chatModels(gw.url, TOKEN)
    assert.deepEqual(models.map((m) => m.id), ["chat-a"])
  } finally {
    gw.close()
  }
})

test("a refused token says what to do about it", async () => {
  const gw = await gateway([])
  try {
    await assert.rejects(chatModels(gw.url, "gwk_revoked"), /401.*opencode auth login aiplane/)
  } finally {
    gw.close()
  }
})

test("the plugin offers a key sign-in and the token's models once signed in", async () => {
  const gw = await gateway([{ id: "chat-a", kind: "chat", context_window: 65536 }])
  const h = host(undefined)
  const cleanup = await plugin(gw.url).setup(h.ctx)
  try {
    assert.deepEqual(h.integrations.get("aiplane").methods.map((m) => m.type), ["key"])
    assert.equal(h.integrations.get("aiplane").ref.name, "croit AIplane")
    assert.deepEqual(h.state.providers, [], "no provider before a token is stored")

    await h.switchTo({ type: "key", key: TOKEN })
    assert.equal(h.state.providers.length, 1)
    const [record] = h.state.providers
    assert.equal(record.info.settings.baseURL, `${gw.url}/v1`)
    assert.equal(record.info.integrationID, "aiplane")
    assert.deepEqual(record.models.map((m) => m.id), ["chat-a"])
    assert.ok(record.sourceConnection, "models are bound to the connection that listed them")

    await h.switchTo(undefined)
    assert.deepEqual(h.state.providers, [], "signing out withdraws the models")
  } finally {
    await cleanup()
    gw.close()
  }
})

test("a slow listing for an earlier token never replaces the current token's models", async () => {
  const gw = await gateway({
    gwk_old: { data: [{ id: "old-model", kind: "chat" }], delayMs: 300 },
    gwk_new: { data: [{ id: "new-model", kind: "chat" }] },
  })
  const h = host({ type: "key", key: "gwk_old" })
  const starting = plugin(gw.url).setup(h.ctx)
  try {
    await new Promise((resolve) => setTimeout(resolve, 50))
    await h.switchTo({ type: "key", key: "gwk_new" })
    const cleanup = await starting
    assert.deepEqual(h.state.providers.flatMap((p) => p.models.map((m) => m.id)), ["new-model"])
    await cleanup()
  } finally {
    gw.close()
  }
})

test("a failed refresh keeps the plugin watching for sign-ins", async () => {
  const gw = await gateway([{ id: "chat-a", kind: "chat" }])
  const h = host(undefined, { failNextResolve: true })
  const cleanup = await plugin(gw.url).setup(h.ctx)
  try {
    await h.switchTo({ type: "key", key: TOKEN })
    assert.deepEqual(h.state.providers, [], "the failed refresh offers nothing")
    await h.switchTo({ type: "key", key: TOKEN })
    assert.deepEqual(h.state.providers.flatMap((p) => p.models.map((m) => m.id)), ["chat-a"])
  } finally {
    await cleanup()
    gw.close()
  }
})

test("a periodic refresh picks up models the gateway adds later", async () => {
  const listing = { [TOKEN]: { data: [{ id: "chat-a", kind: "chat" }] } }
  const gw = await gateway(listing)
  const h = host({ type: "key", key: TOKEN })
  const cleanup = await plugin(gw.url, undefined, 50).setup(h.ctx)
  try {
    assert.deepEqual(h.state.providers.flatMap((p) => p.models.map((m) => m.id)), ["chat-a"])
    listing[TOKEN].data.push({ id: "chat-b", kind: "chat" })
    await new Promise((resolve) => setTimeout(resolve, 250))
    assert.deepEqual(
      h.state.providers.flatMap((p) => p.models.map((m) => m.id)),
      ["chat-a", "chat-b"],
      "a gateway-side change reaches the client without a restart",
    )
  } finally {
    await cleanup()
    gw.close()
  }
})

test("a failed periodic refresh keeps the last good models", async () => {
  let requests = 0
  const server = createServer((req, res) => {
    if (req.url !== "/v1/models") return res.writeHead(404).end()
    requests++
    if (requests > 1) return res.writeHead(503, { "content-type": "application/json" }).end('{"error":{"message":"hiccup"}}')
    res.writeHead(200, { "content-type": "application/json" }).end(
      JSON.stringify({ object: "list", data: [{ id: "chat-a", kind: "chat" }] }),
    )
  })
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve))
  const h = host({ type: "key", key: TOKEN })
  const cleanup = await plugin(`http://127.0.0.1:${server.address().port}`, undefined, 50).setup(h.ctx)
  try {
    await new Promise((resolve) => setTimeout(resolve, 250))
    assert.deepEqual(
      h.state.providers.flatMap((p) => p.models.map((m) => m.id)),
      ["chat-a"],
      "a transient gateway failure must not withdraw the models",
    )
  } finally {
    await cleanup()
    server.close()
  }
})

test("an unchanged listing does not reload the provider", async () => {
  const gw = await gateway([{ id: "chat-a", kind: "chat" }])
  const h = host({ type: "key", key: TOKEN })
  const cleanup = await plugin(gw.url, undefined, 50).setup(h.ctx)
  try {
    await new Promise((resolve) => setTimeout(resolve, 250))
    assert.equal(h.state.reloadCount, 1, "the initial listing is the only reload")
  } finally {
    await cleanup()
    gw.close()
  }
})
