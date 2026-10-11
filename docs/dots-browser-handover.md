# Dot browser login and human handover

## Status and scope

Proposed, unimplemented design following the
[Grok comparison](dots-grok-comparison.md). This document prepares a browser
authentication flow; it does not authorize new rights, select a remote desktop
dependency or claim that login secrets are already protected by this flow.

Depend on the adopted [isolated workspace and browser proposal](dots-workspace.md).
A user operates the browser belonging to one owned Dot, completes a login or
challenge, and explicitly returns control. The Dot retains its own profile;
neither another Dot nor the user's local browser supplies that identity
implicitly. Organization access and shared human control require their separate
ownership and responder designs.

Initial product scope is browser interaction through the web application:
view, click, type, supported keys and scroll. Exclude terminal/desktop access,
file dialogs, arbitrary JavaScript, clipboard synchronization, download/upload,
session export and browser-cookie inspection. Support only flows proved by
acceptance tests; provider-specific authentication compatibility is not assumed.

## Verified existing mechanisms

| Source inspected | Actual behavior and consequence |
|---|---|
| `crates/aiplane-runtime/src/server/tools/sandbox/browse.rs` | Playwright daemon retains a browser in a leased container and serializes JSON commands on `/tmp/gwbp.sock`. Its current context is ephemeral. Numbered element references come from the live DOM and become stale on navigation. |
| Same browser daemon | `describe` returns URL, title, body text, interactive labels and console output. A label can fall back to `el.value`. `fill` returns a description afterward. `screenshot` writes an artifact file. None is a suitable unchanged human-secret channel. |
| Runtime `server/tools/sandbox/mod.rs::SandboxLease` | A mutex covers a runner round-trip; calls reuse, recreate or lose a container. Release and Drop are best effort. A stale lease can produce a new container. This does not establish human/model exclusivity or preserve a pending challenge. |
| `crates/aiplane-runtime/src/openai_driver.rs` | The common turn exit releases both sandbox and browser leases, including when work pauses. A durable suspension alone cannot keep the same live authentication page available. |
| `crates/sandbox-runner/src/{pool,server,backend}.rs` | One runner manages lease capacity, busy flags, TTL and teardown. Its HTTP boundary is private-network/mTLS deployment isolation, not an end-user authentication service. Backend execution already uses child-process pipes. No browser viewer/control endpoint exists. |
| `crates/aiplane-tools/src/browser_control.rs`; `web/src/lib/browser-bridge.ts` | Existing extension control sends action batches through the chat SSE stream and receives authenticated feedback. Every connected chat client may receive a batch. It targets a paired local browser and feeds results to the model; it is not a server-browser viewer. |
| `crates/aiplane/src/rama_server/api.rs::{turn_feedback_body,browser_feedback}` | Existing feedback verifies browser session and turn ownership, caps the body and uses a per-request ID. The browser feedback hub is a transient rendezvous, not durable handover state. |
| `crates/aiplane-core/src/rama_server/session.rs` | Browser sessions use signed HttpOnly cookies, SameSite=Lax and Secure according to deployment configuration. An explicit Origin/CSRF handover guard was not found in the inspected feedback handler. |
| Runtime `suspend.rs`, `openai_driver/resume.rs`; `session-core/src/db/suspensions.rs` | Existing suspensions retain original calls, expiry and run context. Resume claims once. SecureInput keeps its answer out of Debug formatting and carries a value through the resumed tool; it is not proof that a browser interaction cannot echo that value into a result. |

## Shared mechanisms

Reuse the typed Dot owner/session context and live tool authorization;
`GrantedToolSource`; the existing browser daemon, SandboxClient, backend and
runner pool; the workspace scope/profile lock and confirmed teardown proposal;
existing durable suspensions, Inbox, reserve-before-claim resume, cancellation,
background reconciliation, usage limits and translated web forms.

Use existing JSON-SSE only for nonsecret state changes. Frames and human input
must be separate from model tool results, chat events and attachment storage.
Extend the same runner service with narrowly typed browser operations; do not
expose its Unix socket, CDP port, container ID or arbitrary execution interface
to a browser client. No second scheduler, approval engine, computer service or
worker registry is proposed.

One new shared primitive is necessary: typed ownership transfer of a scoped
browser lease, enforced by the runner for both model and human commands. It
must exist, with tests and real callers, before the viewer or login tool uses
it. The workspace lifecycle already needs confirmed teardown; handover extends
that lifecycle rather than replacing it.

## Authority and minimal data

The gateway resolves the owner and Dot from the authenticated browser session
and existing Dot association. The runner receives trusted owner/Dot scope,
not model-supplied paths. Each control handle identifies the actual browser
lease generation, mode, deadline and one controlling client. A random handle
is an identifier, not standalone authorization.

Use the runner's existing lease record for transient mode/control state. For a
Dot waiting on login, store only the typed nonsecret browser binding in the
existing suspension `run_context`: Dot ID, opaque lease generation/control
reference and request generation. Do not add a separate decision queue or
copy the conversation into a browser session table. The retained browser may
disappear; a persisted reference never proves it is still live.

Do not store typed characters, credentials, full login URLs with query/fragment,
DOM, console logs or image bytes in this metadata. The model receives only
that browser authentication needs human help and, later, its actual outcome.
Show the owner an actual validated origin where available; do not invent an
account name or assert that clicking Done proves successful authentication.

Every operation revalidates cookie/session, owner, Dot binding, lease generation,
control client and deadline. A revoked session, deactivated/deleted Dot or lost
lease cannot continue operating the browser. This initially requires an active
Dot even for human control because it creates an authenticated execution
environment; changing that rule is a separate product decision.

Require HTTPS outside local development, same-origin mutation requests and an
explicit anti-CSRF control. Reject foreign/missing Origin where appropriate to
the browser-only endpoint contract; use a per-control client nonce in a header
or body, not a URL. Keep it in memory and never use it as a replacement for
the authenticated owner check. A second tab receives Busy rather than silently
stealing control. Same-origin malicious script remains inside the web origin's
trust boundary; neither these checks nor the existing extension pairing makes
an XSS harmless.

## Transfer and race prevention

Proposed runner modes are `model`, `transfer_pending`, `human`, and `sealed`.
They extend the same scoped lease admission mechanism used for Dot/profile
exclusivity. One monotonically changing generation fences stale commands.

1. A user may request login setup on an idle Dot browser. A running Dot may
   request help through a concrete new `browse_page` handover action, subject
   to its existing selected-tool and approval rules. This is a proposed
   action, not part of today's schema. Do not infer that every browser error
   is an authentication challenge.
2. Under runner lease admission, move to `transfer_pending`, refuse new model
   commands and let the admitted command finish. A gateway-only mutex is
   insufficient: other turns, generic code and runner requests must obey the
   same scope fence.
3. For a running turn, persist the existing durable suspension with a typed
   browser continuation before making human control available. Retain the
   exact browser container/page and profile lock through an explicit lease
   transfer. The ordinary turn-exit release must recognize transferred
   ownership and cannot kill that browser. Code/scratch leases still release.
4. Confirm the durable wait and runner transfer correspond to the same
   generation, then admit the authenticated owner in `human` mode. While the
   wait remains, existing session admission prevents another turn. Idle login
   setup uses the same runner scope lock to prevent a new Dot computer turn
   from starting against that profile.
5. Human input and viewing run only against that generation. No model DOM,
   console, capture or navigation command is admitted. Extend the scope fence
   to generic code access too; its filesystem mount must not expose the browser
   profile or control socket.
6. On explicit Return control, stop human admission, drain the admitted input,
   fence late requests and complete the privacy transition below. Only then
   release the human controller and let the existing durable turn resume.
   Reserve the existing worker before claiming its suspension. A normal
   approval click, timeout or stale request cannot bypass an incomplete
   browser transfer.

Use a bounded control deadline and existing runner TTL/lifecycle machinery,
not an indefinitely retained container. Propose five minutes of idle control,
15 minutes maximum per takeover; these are reviewable new defaults, not
existing settings. Frame polling alone must not renew active control forever.
Expiry or disconnect seals human input and leaves autonomous control blocked
until deliberate handback or confirmed teardown; it never silently returns
the browser to a model. Existing suspension expiry settles the waiting work
under its deny/cancel semantics.

The runner is authoritative for fencing. DB-to-runner transitions are not a
distributed atomic transaction. Design idempotent prepare/confirm/revoke
operations and reconcile failed transitions through existing lifecycle cleanup.
A crash between phases yields unavailable/sealed control, not dual control.
Runner restart invalidates transient handles; preserve a durable wait, report
the lost page and require a new explicit login session. Persistent profile
state may survive, but a CAPTCHA or in-progress 2FA transaction is not promised
to survive process loss.

## Browser-only web flow

Proposed owner-scoped endpoints extend the existing Dot resource:

| Endpoint | Purpose |
|---|---|
| `POST /api/v0/dots/{id}/browser/handover` | Start or accept a takeover using an optional actual waiting-turn/request reference; return state, opaque handle and expiry. |
| `GET /api/v0/dots/{id}/browser/handover/{handle}` | Read nonsecret control state; unavailable or Busy is explicit. |
| `POST /api/v0/dots/{id}/browser/handover/{handle}/frame` | Fetch one capped volatile viewport frame for the controlling client; no shared chat delivery. POST carries the control nonce outside the URL. |
| `POST /api/v0/dots/{id}/browser/handover/{handle}/input` | Apply one strictly typed, capped click/key/text/scroll operation with generation and sequence checks. |
| `POST /api/v0/dots/{id}/browser/handover/{handle}/finish` | Explicit handback through fencing, privacy cleanup and existing resume. |
| `DELETE /api/v0/dots/{id}/browser/handover/{handle}` | Revoke control; stop or retain the durable wait according to its existing cancellation flow. |

All endpoints remain proposals. The implementation must define numeric frame,
input, rate and timeout bounds before code and prove them in tests. Polling
bounded viewport frames with existing HTTP primitives is the initial transport
recommendation; do not select WebRTC/VNC or a desktop service without measured
need. Coordinate mapping must use the returned frame's viewport/generation and
reject stale input; no full-page screen recording is required.

The web surface shows the actual Dot, current state and expiry. It states that
the owner is operating that Dot's browser, offers Stop and Return control, and
does not combine handback with approval of unrelated external writes. Keyboard
and touch handling need accessible translated controls. Changes in the private
chat/Inbox can link to this surface but cannot embed its pixels. Only the
controlling view sees frames.

## Secret handling and output transition

Human input goes through a capped, typed transient channel to the daemon and
must not be embedded in generated code, command-line arguments, staged `/work`
files, RunRequest dumps or ordinary tool-call JSON. Reuse backend child-process
pipes with a fixed reviewed command client; extend its typed interface where
needed. The current generic RunRequest/RunResponse/shape_response path is not
the human-input implementation.

Disable daemon DOM descriptions, input-value extraction, console capture,
tracing and artifact screenshots during human control. Deliver viewport bytes
only to the authenticated controlling view, with `Cache-Control: no-store`,
no attachment key, no analytics capture and no body logging. Clear frame buffers
and revoke their URLs at exit. Live visual frames may inherently display
credentials visible on the website; they are private transient viewing, not
retained screenshots or model evidence. If even that display is disallowed,
the product cannot offer a normal pixel viewer for that authentication flow.

Before returning control, clear console buffers and captured observations,
invalidate element references and require a new safe browser observation.
Remove input values from model-facing element descriptions, exclude URL
queries/fragments from their metadata and keep model screenshots blocked for
the authentication phase. No raw browser state is placed in a resume result;
return only actual completion/unavailability/cancellation facts.

These measures cannot prove that an arbitrary page will not echo a secret in
body text, canvas pixels or an error. Automatic secret discovery or DOM masking
is not a complete guarantee. A supported post-login output boundary, and
tests proving that submitted values never reach the model through any channel,
are implementation prerequisites. If the owner cannot reach a supported safe
handoff state, keep the browser human-only or close it; do not claim a generic
zero-secret-exposure guarantee. SecureInput's redacted Debug is useful prior
art but does not solve this browser-output problem.

## Acceptance and unresolved limits

Write finite state-based tests before implementation using real SQLite,
temporary profile directories, the existing fake runner/backend, wiremock and
local test pages. The coordinator also verifies the real web/browser flow.

- Same-owner success and wrong-owner/session, stale handle, foreign Origin,
  expired controller, second-tab and cross-Dot refusal.
- A model command admitted before transfer drains; commands admitted afterward
  fail. Late human commands after handback cannot execute. Generic code cannot
  reach another Dot's profile or bypass the controller socket fence.
- A suspended browser retains its exact challenge page during takeover;
  ordinary leases release. Lost lease does not silently recreate a page under
  the same handle. Failed teardown retains the profile lock.
- Dropped requests, cancellation, Dot deactivation, expiry and restart settle
  control without duplicate resume or stuck dual ownership. Source/history and
  original tool arguments remain those of the existing background turn.
- Password and OTP markers are absent from chat rows, suspension context,
  memory, model requests, tool results, console/error bodies, logs, telemetry
  and attachment storage; frame caching/export is disabled. Exercise pages
  that deliberately echo submitted values rather than testing only password
  fields.
- Input/body/frame limits, bounded polling, stale coordinates, mobile controls,
  keyboard access and explicit failure/unavailability states.

Verified gaps still requiring implementation evidence: persistent profiles and
checked teardown; safe browser ownership transfer; typed private frame/input
transport; post-authentication output filtering; heartbeat/expiry integration
with the current pool; and the compatibility of real CAPTCHA, passkeys,
hardware-backed 2FA and third-party popups. Existing Playwright is headless;
this document does not assume it can satisfy challenges requiring a native
interactive desktop. Sharing rights, organization responders and data policies
remain separate decisions. No code or build changes accompany this proposal.
