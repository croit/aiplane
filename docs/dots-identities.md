# Dot connector identities and organization ownership

## Status

Adopted proposal for upcoming vertical slices under the maintainer's delegated product decisions. Own Dot credentials and organization Dots are not implemented. Current personal Dots use their owner's effective permissions and existing user or gateway-shared MCP connections. This proposal extends those mechanisms; it does not provision email accounts or connect any external account.

## Verified source baseline

| Source | Current mechanism and constraint |
|---|---|
| `aiplane-core/src/server/db/user_mcp.rs`; migration `0023_user_mcp.sql` | One connection per user and connector. Access/refresh/DCR secrets are ciphertext plus nonce. Pending OAuth records bind state to the initiating user and connector. |
| `aiplane-core/src/server/auth/mcp_oauth.rs` | OAuth discovery, PKCE, dynamic client registration, authorization, code exchange and refresh. Token results do not provide a general verified provider-account identity. |
| `aiplane-api/src/pages/integrations.rs` | Existing connect and shared `/integrations/callback` flow. Callback consumes pending state, verifies the signed-in user, exchanges at the recorded token endpoint, seals secrets and saves the user connection. |
| `aiplane-runtime/src/server/tools/mcp/manager.rs` | User layers resolve connected user accounts and allowed global connectors. Principal layers resolve explicitly granted shared `global`/`agent` connector identities; they do not read user OAuth credentials. Refresh/cache keys currently use user and connector, with a distinct global key. |
| `aiplane-core/src/server/db/mcp_catalog.rs` | One actual connector catalogue, scope `per_user`, `global` or `agent`, auth `oauth2`, `none` or `static_bearer`, enabled state and allowed gateway groups. |
| `aiplane-agents/src/db/system_principals.rs` | Principals and explicit resource grants, initially empty; existing grant audit and model/pool constraints. |
| `aiplane-agents/src/db/agents.rs` | Agent definitions and user/group shares with read/write/respond access; last-write-share protection. Personal Dot ownership is explicitly a user. |
| `aiplane-core/src/server/db/gateway_groups.rs`; migration `0044_gateway_groups.sql` | IdP-independent groups, OIDC mappings, tool/model rights and administration. A group is an authorization unit, not a tenant or organization owner. |

A search of schema and ownership/accessors found no organization entity or typed organization membership. Group shares do not establish a separate organization's ownership or data boundary. Connection records also lack a verified provider email/display identity. Neither absence should be hidden by invented labels.

## Shared mechanisms

Reuse the catalogue, MCP manager and live tool inventory; crypto sealing and existing token fields; OAuth discovery/PKCE/state/callback/refresh; pinned and guarded network requests; `AgentSpec` tools and approval rules; principal grants; agent user/group shares; gateway-group RBAC; activity and MCP audit; existing user session identity and error/translation conventions.

Introduce one neutral typed `ConnectionOwner::User(id) | Principal(id)` in the connection storage/manager contract. Extend existing accessors and connection rows to support both rather than creating a second OAuth subsystem. Add one Dot-aware identity-selection resolver in runtime that chooses a single connection before composing tools. The core connection layer must not import Dot, organization or runtime types.

Organization metadata is a distinct typed owner entity in `aiplane-agents`, with membership derived from the existing gateway-group resolver and links to named gateway groups. Do not create another role/grant engine or reinterpret every gateway group as an organization automatically. `session-core` remains owner-agnostic.

## Chosen minimum personal-Dot behavior

For each enabled supported connector, the owner can select the existing user connection or a separate connection owned by that Dot's existing system principal. The Dot's name identifies the agent; the external account actually authorized determines who Gmail, a ticket server or another provider sees. Authorizing the same external account twice can still produce the same external actor. The product must explain that a separately stored credential does not itself create a separate provider account.

Reuse the owner's current effective connector rights as the ceiling for both modes. A Dot-owned token cannot restore a connector the owner is no longer allowed to use. Tool availability comes from the chosen connection's actual tool listing, narrowed by owner grants/settings and the Dot's existing rules. Do not require a user OAuth token as a prerequisite for authorizing a Dot-owned token when the catalogue permits that user to connect.

Keep existing shared/global connections clearly labelled as shared resources for personal Dots that already use them. Do not offer a fabricated Dot-owned choice for an authless or gateway-shared connector. The first own-credential slice supports catalogue OAuth2 connectors that already use the per-user authorization flow; static credentials can be extended through the existing credential form only when its real supported storage/validation path is included. Unsupported modes display their actual state, not an enabled nonfunctional option.

An explicit identity choice applies to future calls. Store the resolved connection owner/connection ID with a suspended MCP call. Resume must still match that recorded identity, current Dot selection and live authority. A changed identity requires a new invocation and approval rather than sending the old arguments under a different account. Disconnect/reconnect of the same credential owner also changes the connection generation; a pending call cannot silently adopt a replacement credential.

## Minimum schema amendment

Amend unreleased migration `0081_personal_dot_sessions.sql` and its schema/checksum fixtures once for this change set; table names below are the chosen proposal, not current schema claims.

- Generalize `user_mcp_connections` to `mcp_connections`, preserving existing user records, token/scopes/status fields and timestamps. Add mutually exclusive nullable `user_id` and `principal_id`, each with a real cascading foreign key, plus partial uniqueness for owner and connector. The typed owner is derived from exactly one nonnull field. No compatibility view or second token store.
- Extend `pending_mcp_oauth` with the target connection owner, initiating user ID, Dot ID where applicable and immutable authorization generation. Pending state stores the chosen target; callback query parameters cannot change it. Preserve token endpoint, resource, PKCE, DCR fields, expiry and safe return path.
- Add `dot_connector_bindings(dot_id, connector_key, identity_mode, connection_generation)` with a constrained mode `user` or `principal`, uniqueness per Dot/connector and foreign keys. This table is identity selection only; tool/action policy stays in `AgentSpec`.
- Give each connection a generation changed on reauthorization, revoke or replacement. Include it in live-cache keys and suspended-call identity facts. Serialize refresh by typed owner and connector; refresh preserves generation while disconnect/reconnect changes it.
- Add organization metadata and organization-to-gateway-group membership bindings, plus an organization-Dot ownership record linking existing principal/agent to exactly one organization. Organization rows have their actual configured name. Avoid polymorphic IDs with no integrity checks.
- If organization-shared OAuth accounts are included, allocate one managed system principal per organization as their credential owner. Organization Dots select that explicit principal or their own principal; all secrets remain in the same `mcp_connections` table.

The organization's existence and grants are independent of its creator's account lifetime. User deletion removes user connections and personal Dot-owned connections through existing personal ownership cascades; it must not delete organization-owned credentials merely because that user initiated OAuth. Organization deletion and principal deletion invalidate relevant connections, pending flows and cached tools. Audits preserve actor and selected resource attribution without secrets.

## OAuth ownership and lifecycle

1. Authenticate the initiating person and validate ownership/manage rights and catalogue connect rights before discovery. A personal Dot target must belong to that user; an organization target requires current membership plus existing write/management authority.
2. Record the target and initiator in pending state, using the existing random state, PKCE and expiry. Reuse the callback route and provider redirect URI to avoid duplicating provider setup.
3. On callback, consume state once and verify the initiating session. Reload target existence, ownership/membership, connector enablement/connect rights and generation before exchange and again before transactionally saving tokens. Revoked rights or a deleted Dot must not produce an orphan credential.
4. Exchange through the recorded trusted token endpoint and resource, seal secrets with the existing crypto service and upsert only the recorded owner. Invalidate only that owner's connector cache. Redirect only to a validated internal return path.
5. Refresh through the existing endpoint/client-secret handling and error status lifecycle, keyed by typed owner. Concurrent refresh and disconnect need generation-checked writes so a late refresh cannot resurrect a disconnected connection.
6. Disconnect removes or revokes the selected local credential, invalidates its cache and pending flows and disables affected tools. Provider-side revocation is only claimed when actually supported and implemented by the connector.

Token responses are not proof of an email or account name. Initially show actual connector name, local ownership label and connection status. A verified account label may be persisted only through a real provider-supported profile response with documented scope and validation; absent data is shown as unavailable. Do not decode arbitrary access tokens to invent an identity or add generic profile requests to unknown providers.

## Organization authority and privacy

An organization Dot executes as its own system principal with explicit grants and narrowed `AgentSpec`. It never inherits the initiating member's tool/model grants, OAuth credentials or personal memory. Organization membership grants access to the organization's resources; it is not itself a principal grant for external execution. Reuse existing agent share levels for visibility, configuration and response rights, narrowed by organization membership. Existing gateway administration rights continue to control resource provisioning.

The organization must choose an explicit credential owned by its Dot principal or organization credential principal. No fallback to an employee's user connection or the gateway's shared identity is permitted when the chosen credential is absent. Whether a particular shared service identity is appropriate is represented by explicitly provisioning it as the organization's credential, not inferred from catalogue scope.

Keep user-wide memory unavailable to organization execution. Organization context needs a separately scoped knowledge/memory design; do not silently promote existing personal notes. Run results and secret configuration follow the existing agent share privacy boundary. People with respond access can settle only the requests authorized by existing approval rules; a share does not grant reading credentials. Organization approval ownership must be concrete before organization background execution is enabled: choose the existing responding member/share path, persist that authorization context and recheck current membership on settlement.

## Proposed API and actual-data UI walkthrough

Extend existing Dot access disclosure and integration cards, not a second capability picker. Return connector rows from the catalogue and live connections with actual `key`, `name`, `auth`, supported identity choices, selected mode, connection status and nullable verified account label. Do not return token ciphertext, plaintext or internal refresh errors that contain secrets.

- `GET /api/v0/dots/{id}/connections`: owner-visible inventory and selected bindings.
- `PUT /api/v0/dots/{id}/connections/{key}`: select a supported connected identity, validating owner/catalogue/current rights and generation. An explicit inactive Dot's connections may be managed without enabling execution.
- `POST /api/v0/dots/{id}/connections/{key}/connect`: initialize the reused OAuth flow for the Dot principal and return its redirect through the existing navigation mechanism.
- `DELETE /api/v0/dots/{id}/connections/{key}`: disconnect that Dot-owned credential, never the user's ordinary account. Switching selection is separate from revoking credentials.

Walkthrough: open Harald's **Access and approvals**, inspect an actual Workspace connector row and its current account ownership. Select **My connection** when one really exists, or **Connect an account for Harald** for a supported OAuth connector. The provider controls which account the person authorizes. Returning shows its actual local connection status and verified account label if available. Select that connected identity, save, then choose existing tool actions/rules. Warn with the real connector name that pending calls under another identity require a new call; do not automatically approve or replay them. Errors preserve the previous saved identity and give a reconnect/retry path.

Organization management reuses existing agent forms, model selectors, resource grant controls and share editing after choosing a real organization row. Its connector selector shows only that Dot's or its organization's managed credentials, and never **My connection**. Missing connections remain visible as unavailable; users are not offered fictional identities or account provisioning. Organization creation/membership editing needs a concrete walkthrough with configured group data before UI implementation. Initial membership setup is operator-controlled through existing group rights, with actual group selectors.

All visible strings use six Fluent catalogs. Locale groups cover actual connection ownership, missing account metadata, provider-controlled account selection, connect/disconnect outcomes, changed pending-call identity, expired authorization and membership/ownership errors. Existing connector labels and statuses should reuse translations rather than introduce alternate terminology.

## Vertical slices and state-based verification

1. Generalize existing connection ownership/storage/cache/refresh with tests preserving ordinary user/global behavior. Add the real personal Dot OAuth caller and callback target proof in the same complete slice.
2. Add per-Dot saved identity selection, current tool inventory and suspended-call identity/generation checks; integrate the existing disclosure and test through the real seeded gateway.
3. Add typed organization/group membership and organization-owned principal/connection management with grants and existing shares. Organization background sessions and approvals require their own confirmed private owner/settlement bridge before execution.

The coordinator owns migration/checksum/shared schema and docs; one backend agent owns connection DB/OAuth/manager; one runtime agent owns resolver, suspended facts and tool execution; one UI agent owns disclosure/client/locales/generated files. Leases must be disjoint; callback/manager contracts are fixed before fan-out. Build locking and one complete reviewed commit follow the existing delegation rules.

Use in-memory SQLite, existing real crypto and mocked OAuth/MCP HTTP servers. Assert tokens remain scoped after user versus Dot authorization, callback cannot change target, consumed/expired/mismatched state fails, deleted targets and revoked memberships fail before save, refresh races cannot resurrect revoked generations, two owners cannot share refresh locks/cache entries, and connection loss cannot fall back to another identity. Exercise pending approved arguments after identity change and reconnect, expecting refusal before an external write. Assert principal grants, owner ceilings and connector modes remain enforced.

Organization tests prove mapped membership and shares intersect, user deletion does not erase organization credentials, member removal prevents management/approval, and organization runs never read user tokens/preferences. Browser tests use actual connector metadata with unsupported modes, missing verified account labels, authorization failures and mobile/keyboard access. Do not require live provider accounts for the automated suite.

## Verified gaps and implementation constraints

- No generic verified provider-account lookup exists; account display may remain unavailable. Separate external actor identities require separately authorized provider accounts, not generated Dot names.
- No organization entity exists. Typed metadata and group-derived membership are proposed; quotas, organization memory and user-owned-session replacement are not implemented by naming an organization.
- Principal MCP currently resolves shared catalogue credentials only; own principal OAuth requires the generalized storage/manager, not merely adding grants.
- OAuth flow supports the existing catalogue/provider features. Static credentials, provider account chooser hints and external account provisioning must be included only when a real supported path is verified.
- Organization background approval and conversation ownership cannot borrow personal Dot sessions. That bridge is a separate prerequisite, while personal Dot credential selection can ship independently.
