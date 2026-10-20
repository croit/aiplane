// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

// croit AIplane provider for OpenCode v2. The gateway serves this file at
// /clients/opencode/aiplane.js with GATEWAY set to its own address; OpenCode
// loads it from ~/.config/opencode/plugins/. `opencode auth login aiplane`
// stores the person's own gateway token, and the plugin lists the chat models
// that token may use, sized from the facts /v1/models reports. The listing is
// polled on a slow tick (five minutes by default): the gateway changes slowly,
// and a granted model or a newly set context window should reach the client
// without a restart.

const GATEWAY = "__AIPLANE_GATEWAY__"
const ID = "aiplane"
const NAME = "croit AIplane"

// OpenCode's own fallbacks for a model its catalog does not know, used where
// the gateway reports nothing.
const OPENCODE_CONTEXT = 200_000
const OPENCODE_OUTPUT = 32_000

export function modelInfo(model) {
  const context = model.context_window ?? OPENCODE_CONTEXT
  // OpenCode reserves the output limit inside the window; an output limit as
  // large as the window would leave no room for the prompt.
  const output = Math.min(model.max_output_tokens ?? OPENCODE_OUTPUT, Math.floor(context / 2))
  return {
    id: model.id,
    modelID: model.id,
    providerID: ID,
    name: model.id,
    // Images only where the gateway confirmed vision. Tools stay on unless it
    // learned the model refuses them: a coding agent cannot work without them,
    // and the gateway records the refusal on the first one.
    capabilities: {
      tools: model.capabilities?.tools !== false,
      input: model.capabilities?.vision === true ? ["text", "image"] : ["text"],
      output: ["text"],
    },
    variants: [],
    time: { released: 0 },
    cost: [],
    status: "active",
    enabled: true,
    limit: { context, output },
  }
}

export function providerInfo(gateway) {
  return {
    id: ID,
    integrationID: ID,
    name: NAME,
    activation: "auto",
    package: "@opencode/ai/providers/openai-compatible",
    settings: { baseURL: `${gateway}/v1` },
  }
}

export async function chatModels(gateway, token, fetchImpl = fetch) {
  const response = await fetchImpl(`${gateway}/v1/models`, {
    headers: { authorization: `Bearer ${token}` },
  })
  if (!response.ok) {
    const detail = await response.text().catch(() => "")
    throw new Error(
      `${NAME} at ${gateway} answered ${response.status} when listing models` +
        (response.status === 401 ? " — the token is unknown, revoked or expired; run `opencode auth login aiplane` with a current one" : "") +
        (detail ? `: ${detail}` : ""),
    )
  }
  const body = await response.json()
  return body.data.filter((model) => model.kind === "chat").map(modelInfo)
}

export function plugin(gateway, fetchImpl = fetch, intervalMs = 300_000) {
  return {
    id: "croit.aiplane",
    setup: async (ctx) => {
      const loaded = { models: [], connection: undefined }

      await ctx.integration.transform((editor) => {
        editor.update(ID, (integration) => {
          integration.name = NAME
        })
        editor.method.update({ integrationID: ID, method: { type: "key", label: "AIplane API token (gwk_…)" } })
      })

      await ctx.provider.transform((editor) => {
        if (!loaded.connection) return
        editor.add({ info: providerInfo(gateway), models: loaded.models, sourceConnection: loaded.connection })
      })

      let generation = 0
      const listModels = async () => {
        const connection = await ctx.integration.connection.active(ID)
        const credential = connection ? await ctx.integration.connection.resolve(connection) : undefined
        if (credential?.type !== "key") return undefined
        return { models: await chatModels(gateway, credential.key, fetchImpl), connection }
      }

      const refresh = async () => {
        const current = ++generation
        let next = { models: [], connection: undefined }
        try {
          next = (await listModels()) ?? next
        } catch (error) {
          console.error(`${NAME}: ${error instanceof Error ? error.message : error}`)
        }
        // A sign-in that changed while this listing was in flight has its own.
        if (current !== generation) return
        Object.assign(loaded, next)
        await ctx.provider.reload()
      }

      // The gateway changes slowly — a model granted to the token, a context
      // window an operator set — and clients size their requests from the
      // listing, so it is polled. Only a successful, changed listing replaces
      // the loaded one: a transient failure keeps the last good models, and an
      // unchanged listing does not churn the provider every interval.
      const tick = async () => {
        const current = ++generation
        let next
        try {
          next = await listModels()
        } catch (error) {
          console.error(`${NAME}: ${error instanceof Error ? error.message : error}`)
          return
        }
        if (current !== generation || !next) return
        if (JSON.stringify(next.models) === JSON.stringify(loaded.models)) return
        Object.assign(loaded, next)
        await ctx.provider.reload()
      }

      const stop = new AbortController()
      const timer = setInterval(() => {
        tick().catch((error) => console.error(`${NAME}: periodic model refresh failed: ${error instanceof Error ? error.message : error}`))
      }, intervalMs)
      const watch = async () => {
        for await (const event of ctx.event.subscribe({ signal: stop.signal })) {
          if (event.type === "credential.switched" && event.data.integrationID === ID) {
            await refresh().catch((error) => console.error(`${NAME}: reloading models failed: ${error}`))
          }
        }
      }
      watch().catch((error) => {
        if (!stop.signal.aborted) console.error(`${NAME}: watching sign-ins stopped: ${error}`)
      })
      await refresh().catch((error) => console.error(`${NAME}: reloading models failed: ${error}`))
      return () => {
        stop.abort()
        clearInterval(timer)
      }
    },
  }
}

export default plugin(GATEWAY)
