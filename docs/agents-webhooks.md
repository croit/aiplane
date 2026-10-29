# Organization agent webhooks

## Status and scope

Organization webhooks extend the existing webhook product, management forms
and private principal execution. Personal webhooks and
[organization schedules](agents-organizations.md) share the same admission,
approval and history mechanisms.

An external event starts a fresh private conversation using the organization's
named agent, a selected retained published version and its own principal and
connector identities. The user manages it entirely through the web application.
The creator is attribution, not the job's execution identity or lifetime owner.

## Shared mechanisms

Reuse `webhooks` and `webhook_runs`, secret mint/hash helpers, the existing trigger
route, `WebhookForm`, rows and history, principal sessions, the actual version
catalog, typed `AgentSpec`, existing model choices/defaults, `GrantedToolSource`,
selected credential epochs, `chat_turn_suspensions`, private TestChat/StreamedChat,
exact-turn Stop, Inbox and usage metering. Use the existing capped body readers
and API error envelope. Actual outbound tools retain their pinned `net_guard`
clients; the inbound trigger does not fetch a submitted URL.

Use the shared `server::background::AutomationOwner` for the User/Principal
owner of both automation sources. Extend the existing strict `OrganizationAdmission`
with an exact webhook source/run/generation binding. Extend the existing pinned
principal loader and background reconciliation; do not introduce another worker,
model picker, approval store, owner enum or OAuth service.

## Data model

Amend the unreleased agent migration and its checksum/schema fixture. Rebuild
`webhooks` while preserving existing rows and child history with foreign keys on.
Owner is an immutable User/Principal XOR. User ownership retains its cascading
user FK and explicit model; Principal ownership has a RESTRICT identity FK,
nullable model, positive authored version, no personal agent binding, asynchronous
mode and fresh conversations. Creator attribution is nullable and has no user FK.
Configuration generation is opaque, nonempty and stored on the source.

Each new run stores its actual source generation, authored version, prompt,
untrusted payload, source kind, tools/history limits and exact private assistant
turn. Legacy User history keeps missing publication/generation snapshots as
missing; it is never assigned an invented creator or publication. All actual
indices and historical fields are preserved.

Execution edits change generation; names, no-op saves, source pause and bookkeeping
do not. Management edits, toggles, secret rotation and explicit replay require the current
generation. A retained source prevents agent identity deletion. Source deletion refuses active, waiting
or unbound starting runs and cascades recorded history after settlement.

## Authority and admission

All human inventory, configuration, secret reveal/rotation, history and replay
require current organization membership plus Write in the final write transaction.
Personal agent management permission and nonmember administrator recovery do not
allow private organization execution or content inspection. Inactive agents and
disabled principals remain configurable by their current member writers.
Replay rechecks the initiating member's current Write authority after execution
and before returning private status, errors or conversation identity. Revocation
does not change an admitted run's own principal authority.

Public firing authenticates the actual current enabled secret. In the final
transaction, compare the original secret hash and configuration generation,
record the run, validate current active agent/enabled own principal, grants,
credential epochs and the retained published pin, and open/attach its exact fresh
private principal session and assistant turn. No employee preferences, model
selection, credentials or user payer enter this path. Actual execution uses
`UsageSource::Webhook` and the existing principal metering and quotas.

Recheck exact source/run/generation/publication authority before further model,
tool and resumed dispatch. Captured tools intersect the current ceiling. Existing
approval and per-action rules remain effective; enabling tools is not approval.

## Trigger, rotation and replay

Organization triggers are asynchronous only. Return the existing accepted
response shape with an opaque session ID. Do not return output, private errors or
pending decision details to the secret holder. Current member Write still guards
private conversation access. Invalid, unknown, paused and rotated secrets retain
the indistinguishable 404 behavior.

Preserve the existing bounded 256 KiB body-prefix contract, nonce-delimited
untrusted input and execution limits. Prefix truncation is not oversize rejection;
this package introduces no webhook-specific rate admission mechanism. Do not
describe the prompt delimiter as a security boundary.

Rotation retains the existing last-writer behavior, requires current management
generation and changes only the secret hash. Final fire admission rechecks that
hash. Already admitted pauses retain authority after rotation or source pause.
There is no separate credential revision or concurrent-rotation conflict promise.

Explicit member-writer replay is allowed while the source is paused. It captures
the submitted trusted prompt and actual stored untrusted payload in a fresh run,
with fresh own-principal execution checks and the source's current authored pin.
A paused public trigger cannot fire. Execution configuration edits invalidate
further dispatch from older generations; structurally bound Stop and terminal
reconciliation retain the exact original owner/session/turn/publication proof.

## Web application flow

1. Open an organization agent and its scoped Webhooks page as a current member writer.
2. Use the existing new-webhook form, enter name/prompt and select an actual
   published version. Show its authored model or actual missing value. Hide
   personal model selection, conversation reuse and synchronous output controls.
3. Explicitly choose whether tools may run. Save and show the actual newly minted
   secret once using the existing secret presentation.
4. Send an event to the real secret URL and inspect the recorded private history.
   Open its existing TestChat with the captured published source and version.
5. Answer a pending decision through the existing private chat or Inbox, or Stop
   its exact turn. Pause prevents new public firing without cancelling that pause.
6. Rotate the secret, replay a stored payload or edit the source with the current
   generation. A conflict preserves input and requests reload of the actual row.
7. Settle outstanding runs before deleting the source or agent identity.

## Verification

Finite migrated-SQLite tests cover exclusive immutable ownership, creator deletion,
identity RESTRICT, legacy preservation, generations and exact history bindings.
Actual principal/model/tool tests cover own credentials, revocation, private
admission, webhook usage, waiting/restart/Stop and historical reconciliation.
Router tests cover secret races, paused firing, private rights, bounded body input,
explicit paused replay and deletion. Existing architecture guards enforce shared
admission. Frontend checks use actual API shapes and six translation catalogs;
Chrome verifies the real trigger, private history, decision/Stop and cleanup flow
without intercepted backend requests. Coordinator owns schema, generated files,
shared documentation, the build queue and final browser inspection.
