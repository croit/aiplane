# Automatic model routing

Automatic routes are gateway-owned virtual chat models. A client requests the
route's alias through the ordinary OpenAI or Anthropic `model` field; no custom
client protocol or request extension is required. The gateway asks a configured
System One model to choose among the route's eligible candidates, then sends the
unchanged chat request through the existing chat pool and backend router.

Configure automatic routes under **Admin → Models → Automatic model routes**.
Each route has:

- a client-facing alias that is unique among automatic routes;
- a System One selector model;
- an optimization objective (`quality`, `balanced`, or `cost`) and optional
  routing instructions;
- at least two candidates, each with an opaque key, a real chat model or static
  alias target, and a description for the selector;
- a confidence threshold, timeout, and candidate-backed fallback target;
- optional session affinity with a TTL; and
- `shadow` or `active` rollout mode.

Candidate keys are deliberately opaque. A selector receives the descriptions
and keys, not authority to name an arbitrary model. Its returned key is accepted
only when it belongs to the eligible candidate set.

An automatic route takes precedence when its alias also exists as a static
model alias. This is the intended way to turn an established client-facing
name into an automatic route without changing clients. Its candidates must use
other real model ids or static aliases; an automatic route cannot target itself.

## Request flow

```text
client model alias
  → token and pool authorization
  → healthy/capable candidate filter
  → System One choice
  → confidence / timeout / shadow fallback
  → static alias resolution
  → existing pool and replica routing
  → existing retry and offline fallback
```

Policy selection and infrastructure routing remain separate. System One chooses
a model target; it does not choose a provider replica, bypass group policy, or
replace health checks and failover.

An API token that allows an automatic alias grants access to that virtual model.
Its candidate targets do not also need to be named in the token allowlist, but
pool group restrictions still apply to the selector and every candidate.
Automatic routes cannot target other automatic routes.

## Failure and rollout behavior

- A selector timeout, transport error, malformed answer, unknown key, or answer
  below the confidence threshold uses the configured fallback.
- `shadow` runs and records the selector but always sends the request to the
  fallback. This makes a policy measurable before it changes production traffic.
- Session affinity caches the effective target for the configured TTL. A policy
  version change or an ineligible target invalidates the cached decision. The
  cache is scoped to the authenticated user or API token as well as the session,
  and is bounded to protect the gateway from attacker-controlled session ids.
- When no candidates or the configured fallback are currently eligible, the
  request returns a retryable `503` instead of bypassing policy.
- A static alias that can resolve to different real model ids is not eligible.
  This prevents capability checks for one model from being followed by dispatch
  to another.

Every decision stores the alias, policy version, eligible targets, suggestion,
effective target, confidence, reason, and selector latency. Prompt contents are
not stored in the routing audit table. Recent decisions appear in the admin
editor; the table retains the newest 100,000 decisions.

Responses expose the decision through:

- `X-Gateway-Resolved-Model`
- `X-Gateway-Route-Alias`
- `X-Gateway-Route-Version`
- `X-Gateway-Route-Reason`
- `X-Gateway-Route-Target`
- `X-Gateway-Route-Suggested-Target`
- `X-Gateway-Route-Confidence`

The feature applies consistently to `/v1/chat/completions`, `/v1/messages`,
`/v1/messages/count_tokens`, and the built-in persisted chat driver.
Token counting stays a cheap, quota-independent operation for static models.
When an automatic alias requires selector inference, the normal request and
quota gate runs before that inference.

## Data boundary

The selector receives the request content required to classify it. Inline
base64/media payloads and full tool parameter schemas are removed from the
selector state; media types and tool names/descriptions remain available for
classification. A cloud System One backend therefore still sees the remaining
content even when the selected generation model is local. Place the selector in
an appropriate provider/pool and make this boundary part of the operator's
compliance decision.
