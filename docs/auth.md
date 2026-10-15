# Authentication

Two distinct concerns, often conflated; keep them separate in code and docs.

1. **Login** — OIDC against a generic provider, used once to establish identity.
2. **Ongoing API auth** — gateway-minted bearer tokens, used on every `/v1/*` call.

## Login (OIDC)

AIplane uses its own OIDC client in `aiplane_core::server::auth::oidc`, with `reqwest` for discovery and authorization-code exchange. It uses SHA-256 PKCE, state and nonce, verifies ID-token signatures against the provider's JWKS, and validates issuer, audience and token times. Configure the provider by issuer URL and register AIplane's callback URL with that provider.

### Config

The provider lives in the **database**, entered through the setup wizard at
`/setup` (see below). Issuer, client id, scopes and the roles claim are plain
`app_settings` rows; the client secret is sealed with the at-rest key
(`aiplane_core::server::setup`).

There is no config-file path into this. A fresh install lands in the wizard;
an established one already has its provider in the database.

### Setup wizard (`/setup`)

Two screens, one proof. Screen 1 takes the public URL and the provider's
issuer/client id/secret and shows the exact `{public_url}/auth/callback`
redirect URI to whitelist. Submitting runs a **genuine authorization-code round
trip** — through that same production redirect URI, marked by
`pending_logins.purpose = 'setup'` so `/auth/callback` routes it to the wizard
rather than minting a session. Screen 2 shows the verified ID token's claims and
asks which claim value grants admin.

The round trip is the point: discovery only proves a URL answers. It does not
prove the client secret, the redirect whitelisting, or — the thing nobody can
guess — what the provider calls its groups claim and what values it contains.
There is no way to reach screen 2 without a login that worked.

Finishing stores the provider, creates an `admins` group (`is_admin`) mapped to
the chosen value plus a default `users` group, and calls
`AppState::set_runtime` — so the live OIDC client and public URL are swapped in
without a restart.

**Access.** `SetupAccess::FirstRun` (nothing configured) is open: there is no
account to authenticate against and nothing configured worth stealing.
`SetupAccess::Closed` (configured) 404s. `SetupAccess::Recovery` is opened by
`restore-setup` on the host for 30 minutes and needs the one-time token that
command prints, carried afterwards by a `gw_setup` cookie scoped to `/api/v0/setup`.

**Recovery is not first-run mode.** With a recovery window open AIplane
keeps serving normally — chats, `/v1`, existing sessions — and only `/setup`
becomes reachable again. Conflating the two would let one locked-out admin take
a production gateway offline for everyone else. `setup_wizard.rs` pins this.

The wizard cannot help if the provider itself is gone, since it proves a
provider by signing in through it. `$AIPLANE_BOOTSTRAP_ADMIN_GROUPS` remains
the break-glass anchor that does not depend on the group tables.

Required env: none for OIDC. `AIPLANE_SESSION_KEY` is required for AIplane
to boot at all (it signs sessions and derives the at-rest key).

### Browser flow (web UI users)

Standard server-side OIDC:

1. The SPA detects a missing session and sends the browser to `/login`; the sign-in action starts `/auth/login`.
2. `/auth/login` generates PKCE verifier, nonce and state, stores the pending flow in `pending_logins`, sets a browser-binding cookie, and redirects to the provider's authorization endpoint.
3. Provider redirects back to `/auth/callback?code=…&state=…`.
4. Gateway verifies state, exchanges code for ID/access tokens, validates the ID token signature, extracts subject + email + roles claim.
5. Gateway upserts the user in SQLite, attaches the user id to the session, redirects to the originally requested page.

Sessions use `SessionStore` (see `aiplane_core::rama_server::session`): an HMAC-SHA256-signed cookie `id=<session_id>.<hmac-b64url>` plus a row in the `sessions` table. The pending OIDC handshake (PKCE verifier + nonce + return_to) lives in `pending_logins`, keyed by the OIDC `state` parameter. The callback also checks that the browser-binding cookie matches the returned state. Session cookies use `HttpOnly`, `SameSite=Lax`, `Path=/` and `Max-Age`; `Secure` is set when the configured public URL uses HTTPS.

### Endpoints

| Method | Path | Auth | Purpose |
|---|---|---|---|
| GET  | `/auth/login`        | none | Start browser OIDC flow |
| GET  | `/auth/callback`     | state cookie | OIDC redirect target — for a sign-in *and* for the wizard's test login, told apart by `pending_logins.purpose` |
| POST | `/auth/logout`       | session cookie when present | Delete that browser session and clear its cookie; API tokens remain active |
| GET  | `/setup`             | open on a first run; one-time token in recovery | Setup wizard |
| GET | `/api/v0/setup/state` | same | Read wizard state |
| POST | `/api/v0/setup/test`        | same | Stash the entered provider and start the test login |
| POST | `/api/v0/setup/restart`     | same | Discard the proven login, back to screen 1 |
| POST | `/api/v0/setup/finish`      | same | Persist, create the admin group, swap the live client in |

## Ongoing API auth (gateway tokens)

After login, any OpenAI SDK pointed at us sends `Authorization: Bearer <gateway-token>` on every `/v1/*` call.

### Token format

Random 256-bit value (32 bytes from `OsRng`, hex-encoded), prefixed `gwk_` so the tokens are greppable in logs and accidentally-pushed configs. Wire form: `gwk_<64 hex chars>`. Stored in SQLite as the **SHA-256 hex** of the bearer string — not the plaintext, not an argon2id hash.

Why SHA-256, not argon2id:
- Argon2id is designed for *low-entropy* secrets (passwords) that need slow-down to resist brute-force.
- Our tokens are 256 bits of OS entropy. Brute-forcing them is computationally infeasible regardless of hash speed.
- Fast hashing matters: every `/v1/*` request hashes the bearer and does a DB lookup. SHA-256 keeps that well under a millisecond.
- The lookup column is hex-encoded so it's a normal indexed string column. No special index needed.

We **don't** use JWTs for gateway tokens. Rationale:
- Revocation is trivial with DB-backed tokens (`UPDATE … SET revoked_at = …`).
- We don't need cross-service verification; AIplane is the only verifier.
- One fewer crate (no `jsonwebtoken`).

### Token-bound metadata

Each token row carries:
- `user_id` (FK to users)
- `name` (user-supplied, e.g. "laptop")
- `created_at`, `last_used_at`, `expires_at`
- `revoked_at` (nullable)

The web UI lets users create, configure, rotate, revoke and remove their tokens at `/settings/tokens`. Token plaintext is revealed on creation and rotation, not returned by a normal listing. Tokens also carry tool policy and can have model restrictions and quotas. See [the token guide](guide/account-and-usage.md#create-an-api-token).

### Auth resolution on rama

The rama proxy router resolves auth inline at the top of each handler (no middleware layer — rama Service-style handlers receive the full `Request` and run their own gate):

1. On `/v1/*`, read `Authorization: Bearer …` or `x-api-key`. The bearer header takes precedence; its scheme name matches in any case (`bearer`, `BEARER`), the token exactly. Browser session cookies do not authenticate this surface.
2. For bearer: hash + look up in `tokens`. Reject 401 on miss / revoked / expired.
3. Resolve the token's owner and model restrictions.
4. Update a person's token's `last_used_at` in the background. System-token touches use the separate debounced path described below.
5. Build a `UserCtx` whose `principal` is `Principal::User { id, roles }`; the allowed-tools set is derived from it per request (`AppState::api_tool_layer`).

A bearer starting `gws_` takes a different branch before any of this: see below.

Session-scoped `/api/v0/*` routes authenticate with the browser cookie and return JSON errors when authentication is missing. The SPA handles navigation to `/login`. API bearer authentication and browser-session authentication are distinct gates.

## System principals and `gws_` tokens

A **system principal** is an identity that is not a person — CI, an
integration, and every agent. Design: [`agents.md`](agents.md#principals).

- **Own tables.** `system_principals` (a slug `name`, `display`,
  `description`, `created_by`, `disabled_at`), `principal_grants` and
  `system_tokens` (migration `0077_agent_builder.sql`). Not a `kind` on `users`: none of the
  person paths — default groups, the OIDC upsert, per-user MCP, memory — can
  reach a principal by accident.
- **Own prefix.** System tokens are `gws_<64 hex>`, stored as SHA-256 hex
  exactly like `gwk_`. `require_bearer` routes on the prefix: `gws_` is looked
  up only in `system_tokens` (joined to a principal that is not disabled),
  `gwk_` only in `tokens`. A forged prefix swap never authenticates.
- **What it resolves to.** `UserCtx.principal = Principal::System` with the
  grants loaded once per request and capped at the token's minter (below, reused for at most 30 s). `tools_enabled` is always on — the grants are
  the policy — and there is no model allowlist; its models are its `model`
  grants.
- **Default deny.** A new principal has no rights at all, not even a model:
  until one is granted, `/v1/chat/completions` answers 404 for every model. See
  [`tools-rbac.md`](tools-rbac.md#system-principals) for what each grant kind
  unlocks.
- **Revocation.** `POST …/tokens/{token_id}/revoke` revokes one token;
  `POST …/disable` disables the principal and revokes every token it holds in
  the same transaction. A disabled principal cannot be issued new tokens.
- **Attribution.** Usage rows carry `principal_kind = 'system'` with the
  principal id in `user_id` and its name in `user_email`; `mcp_tool_audit` does
  the same. Rate limits apply the subject's own and global rules — never a
  default group's.
- **Audit.** Every management change (created, disabled, grant added/removed,
  token issued/revoked) is a row in `agent_audit` with the acting user, written
  in the same transaction as the change. Every tool call inside an agent run
  is a row there too (`kind = 'tool_call'`, no acting user, the run's call
  chain in `chain`; see [`agent-runs.md`](agent-runs.md#the-call-chain)).
  `GET /api/v0/system-principals/{id}` returns the trail, `chain` included.

### Management API

All routes need a session whose groups include `can_manage_agents`
(`gateway_groups.can_manage_agents`; `is_admin` implies it). Set the flag
through `PUT /api/v0/admin/groups` with `"can_manage_agents": true`; omitting
the field leaves it unchanged.

| Method | Path | Purpose |
|---|---|---|
| GET  | `/api/v0/system-principals` | List principals with their grants |
| POST | `/api/v0/system-principals` | Create `{name, display?, description?}` — 201, 409 on a taken name |
| GET  | `/api/v0/system-principals/{id}` | Principal, grants, tokens (never a hash or plaintext), audit trail |
| POST | `/api/v0/system-principals/{id}/disable` | Disable and revoke every token |
| POST | `/api/v0/system-principals/{id}/grants` | Grant `{kind, ref}` — 403 `grant_exceeds_manager` when you don't hold it |
| POST | `/api/v0/system-principals/{id}/grants/revoke` | Remove `{kind, ref}` — anyone who may change the principal may narrow it |
| POST | `/api/v0/system-principals/{id}/tokens` | Issue `{name, ttl_days?}` — 403 `token_exceeds_manager` unless you hold every grant; the plaintext is in this response only |
| POST | `/api/v0/system-principals/{id}/tokens/{token_id}/revoke` | Revoke one token |

An agent's principal (created by `POST /api/v0/agents`, see
[`agents.md`](agents.md#agent-definition)) is reachable here too. For one of
those, the caller also needs a share on the agent: `read` for `GET`, `write`
for every change. Without one it answers 404 and is left out of the list.
Admins need no share: they hold `write` on every agent.

Any other principal belongs to the manager who created it
(`system_principals.created_by`): only that user and admins see or change it.
For everyone else it answers 404 and is left out of the list. Without this,
a second manager could mint a token for, or keep changing, a principal that
someone with more rights had granted — the grant-time cap would mean nothing.

**Issuing a token is granting again.** A `gws_` token hands every grant of
its principal to whoever holds it. So a non-admin may issue one only while
they hold every grant the principal has, checked the same way as a new grant
(a grant whose resource is gone counts as not held). Otherwise `403
token_exceeds_manager` names the first missing grant. Admins are not capped.

**A token is capped at its minter, at every request.** The token records who
minted it (`system_tokens.created_by`). When a `gws_` bearer authenticates,
`require_bearer` keeps only the principal's grants that the minter holds *at
that moment*, by the same check as a new grant
(`aiplane_runtime::server::grant_holding::capped_to_minter`): a grant added
to the principal later that the minter does not hold is not usable through
that token, a grant the minter has since lost stops working through it, and a
minter who is gone leaves the token with no grants at all. A model grant
kept this way routes only through the pools of the model's kind the minter
may use, intersected with the grant's own pools (none left, no grant). A
token an admin minted is uncapped while the minter is an admin. *Chosen* over checking only
at issue time: the principal can be granted more after a token exists — by
an admin, or by another manager with a share — and a manager's token must
never become a way to use what that manager could not hand out. The
principal's grants themselves still survive the granting manager losing
rights; use an admin-minted token for a principal meant to outlive its
manager.

**The cap is reused, never past a change.** Working the cap out costs a
lookup per grant (a connector, a RAG collection, an agent share), so
`grant_holding::capped_for_token` keeps it per token in
`AppState::grant_caps` for at most `CAP_TTL` (30 s). The token row and the
principal with its grants are still read on every request, so revoking the
token, disabling the principal or removing a grant takes effect on the next
request: the cached cap is only used while the principal's grants and the
minter are the ones it was worked out from. Everything that can change what
a minter holds calls `GrantCaps::invalidate` — an OIDC login (the minter's
roles), `reload_rbac` (groups, mappings, tool and skill grants),
`reload_settings` (pools, features), a connector's create, edit, toggle,
delete or re-seed, a RAG collection's groups and an agent share — and a cap
worked out while an invalidation ran is not stored. `CAP_TTL` bounds only
what this process cannot see: a skill directory edited on disk, a second
gateway writing the same database. The token's `last_used_at` is written at
most once a minute per token (`TOUCH_EVERY`).

A principal that is not an agent has no SPA screen; it is managed through this API. An agent's principal is managed from the agent's workbench.

## Embed keys (`gwe_`) and visitor tokens (`gwv_`)

The public agent endpoint `/api/v0/embed/*` serves anonymous visitors of a
website that embeds an agent. How it works: [`agent-visitors.md`](agent-visitors.md).

- **Embed key** `gwe_<64 hex>`, one per website, created by a manager with a
  `write` share under `/api/v0/agents/{id}/embed-keys` with a list of exact
  origins. It ships in the website's page source, so it is **not a secret**.
  It is still stored only as SHA-256 (`agent_embed_keys.key_hash`) and shown
  once. Revoking it ends every conversation started with it at the next
  request.
- **Origin allowlist.** Every embed request must carry an `Origin` the key
  lists, and, when the conversation's agent version sets `publish.origins`,
  that list too. This keeps other websites from embedding the agent; it does not stop
  abuse, since a non-browser client sets any `Origin` it likes. Abuse is
  bounded by the agent's default-deny grants, its gates, the per-visitor and
  per-IP limits and the owner's budget.
- **Visitor token** `gwv_<64 hex>`, minted by `POST /api/v0/embed/sessions`
  and stored as SHA-256 (`visitor_sessions.token_hash`). The widget keeps it
  in `sessionStorage` and sends it as `Authorization: Bearer`. No cookie is
  involved, so third-party cookie blocking does not matter and there is no
  CSRF surface.
- **Scope.** A visitor token names one conversation of one agent and is read
  only by the `/api/v0/embed/*` handlers. Everywhere else it fails: `/api/v0`
  reads only the session cookie, and `require_bearer` on `/v1/*` routes by
  prefix and knows no `gwv_`. A visitor is not a principal; the agent's turn
  runs as the agent's principal.
- **Expiry.** Each accepted request slides the session's expiry by the live
  spec's `publish.idle_ttl` (default 30 min), up to an absolute 24 h. An
  expired token gets `401 visitor_session_expired`; the widget then starts a
  new session. A request refused for another reason does not slide it.
- **CORS** is answered only on `/api/v0/embed/*`, and only for an origin some
  live key of an enabled agent lists. Credentials mode stays off.

## A2A callers (`gws_` + `a2a_caller`)

Another agent platform calls an AIplane agent over A2A
(`/a2a/agents/{id}`, [`agent-a2a.md`](agent-a2a.md#serving-an-agent-over-a2a)) as a **system
principal** with its own `gws_` token. There is no new credential type:

- **Scope is a grant.** The token's principal must hold `principal_grants`
  `(kind = 'a2a_caller', ref = <agent id>)`. Default deny: a principal
  without it — including one with every other grant — gets `403
  PERMISSION_DENIED`, and the grant names one agent only. No `Authorization`
  or an unknown, revoked or expired token gets `401` with
  `WWW-Authenticate: Bearer`; a person's `gwk_` token gets `403`.
- **Who may grant it.** `POST /api/v0/system-principals/{id}/grants
  {"kind": "a2a_caller", "ref": "<agent id>"}` needs `can_manage_agents` and
  a `write` share on that agent (admins hold one): the grant-time cap, with
  "holding" an agent meaning being allowed to change it. Revoking the grant,
  the token or disabling the principal ends access at the next request.
- **The caller is not the actor.** A task runs as the agent's principal with
  the agent's grants; the caller's own grants play no part. The caller is
  recorded instead: on the context it opened (`a2a_contexts`: caller, token
  id, client IP), in the run's call chain (`chain.caller`) on every audit and
  usage row of the task, and in an `a2a_task` audit row per started,
  answered or cancelled task.
- **Isolation between callers.** A context and its tasks are visible only to
  the principal that opened them; another caller of the same agent gets
  "task not found" (A2A §13.1).
- The agent card itself is public: it advertises the bearer scheme
  (`securitySchemes.aiplaneSystemToken`), not a secret.

The other direction — an AIplane agent calling an external A2A agent from a
route — is grant kind **`a2a_agent`** (ref: the external agent's card URL).
Only an admin can make it, because it lets visitor-derived data leave the
gateway; the credential for the external agent lives sealed in the route's
`a2a.auth` ([`agent-a2a.md`](agent-a2a.md#external-agents-as-route-targets)).

## What's intentionally out of scope

- **Refresh tokens between CLI and gateway** — re-login is acceptable for a 90-day TTL.

## Request priority

Administrators can assign Low, Medium or High to existing gateway groups and API tokens. A token inherits the highest explicit priority of its owner’s groups unless it has an override; an override may also lower priority. The default is Medium. Owners can select priority at token creation or through `/api/v0/tokens/{id}/priority`, up to their current inherited priority, or restore inheritance. Administrators can select any priority through personal token management or `/api/v0/admin/tokens/{id}/priority`. The server checks ownership and the current priority ceiling before saving. Rotation preserves the existing override. [Request priority](request-priority.md) explains scheduling and backend forwarding.

## Personal Dot management

Personal Dot list, create and active-state routes require `can_manage_dots`
on a resolved gateway group. Administrator and bootstrap-admin membership
imply it; `can_manage_agents` does not. Set it through the existing
`PUT /api/v0/admin/groups` with `"can_manage_dots": true`; omission preserves
the stored value. Fresh groups default to false. `GET /api/v0/me` reports the
resolved flag. Reloading group rights applies grants and revocations immediately.

This flag controls lifecycle management. Existing owner-scoped memory,
activity, tool selection, private chat and background binding checks remain
in place. Revocation does not deactivate existing Dots or stop existing runs;
execution still checks active state, actual user grants, enabled tools,
selected Dot rules and approvals. See [Dots](dots.md).
