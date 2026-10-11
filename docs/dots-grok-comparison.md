# Grok Bot comparison and Dot product gaps

## Scope and evidence

Research checked on 11 October 2026 against the official pages below and the
current AIplane worktree. This is a product comparison and implementation
recommendation, not authorization to implement every listed addition. Existing
behavior is described in [Dots](dots.md); adopted proposals are explicitly
separate from implemented features.

The external pages describe advertised capabilities and employee examples.
They do not establish Grok's internal scheduler, isolation implementation,
approval enforcement, reliability or security guarantees. No executable was
installed and no third-party runtime is proposed.

| Official source | Relevant stated behavior |
|---|---|
| [Launch announcement](https://x.ai/news/introducing-grok-bot) | Persistent cloud work, parallel specialists, a coordinator, shared threads, demonstrated workflows saved for reuse, corrections, and proactive follow-up of stalled work. |
| [Product page and FAQ](https://x.ai/bot) | Bots of one user share a persistent computer, files, browser and logins. The FAQ describes transit/storage encryption, review of sensitive actions, enterprise DLP, certificates, proxies and network controls. Plans describe usage and team administration. |
| [Grok Bot 101](https://x.ai/bot/guides/grok-bot-101) | A user can take over the computer for CAPTCHA or 2FA; credentials can be entered through a secure form. The guide describes multiple connector accounts, schedules/events and permission review. |
| [Grok Bot for Work](https://x.ai/bot/guides/grok-bot-for-work) | An employee's read-only triage assistant prioritizes urgent items and waits for explicit sending approval; a coordinator acts as the main entry point. These are example configurations. |
| [Grok Bot for PMs](https://x.ai/bot/guides/grok-bot-for-pms) | An employee maintains an attention list from authorized work sources and delegates through specialists, retaining human review for consequential actions. |
| [Templates for Grok Bot](https://x.ai/bot/guides/templates-for-grok-bot) | Reviewed templates package reusable configuration for another instance; the guide describes excluding private information and choosing team or public sharing. |

The launch page describes an enterprise waitlist; the product FAQ describes
enterprise availability. This comparison uses capability descriptions, not
availability dates, pricing or subscriptions. Such commercial details are not
requirements for a self-hosted AIplane implementation.

## Coverage in AIplane

**Implemented** means code exists in this worktree, not that it has been
deployed. **Planned** means an existing specification covers it. **New gap**
means additional design and acceptance criteria are needed. **Excluded** means
the confirmed scope does not request it.

| Capability | Classification | Verified AIplane behavior or remaining work |
|---|---|---|
| Named personal assistant | Implemented | Owned Dot inventory, active lifecycle and private chat; user authority with separate Dot attribution. Creation currently requires `can_manage_agents`; broad user creation remains a planned settings refinement. |
| Scheduled and event-driven execution without an open browser | Implemented | Existing schedules/webhooks run a selected personal Dot through headless execution; durable source correlation and existing user usage limits. This does not imply an uninterrupted model process. |
| Tool access and human decisions | Implemented | Explicit selected tools, live authorization, current MCP connections, original-argument approvals and Inbox. Unknown/write effects default to Ask; changing trust does not change permissions automatically. |
| Remember preferences and lessons | Implemented foundation; planned curation | Separate Dot notes, read-only user-wide memory context and existing memory controls; no automatic evidence consolidation or conflict resolution. |
| Persistent files and server browser identity | Planned | [Workspace proposal](dots-workspace.md) extends the existing runner and `browse_page`; current browser context is ephemeral and container/turn scoped. |
| Human computer takeover and login | New gap | The workspace proposal explicitly excludes remote control. The current extension controls the user's connected browser, not the Dot's unattended computer. Human handover needs its own web interaction and security design. |
| Separate connector identities and multiple accounts | Planned | Personal execution currently uses the owner's available or operator-shared MCP connection. Dot-owned OAuth and selectable account identities remain unimplemented. |
| Dot-to-Dot requests | Planned | [Communication proposal](dots-communication.md) covers bounded same-owner text delegation with consent in both directions, receiver rules and durable child waiting. Existing published-agent routes already dispatch children. |
| Group threads, coordinator and project ownership | New gap beyond delegation | No personal Dot group conversation, project roster or task ownership surface is implemented. A child request/reply is not a group collaboration product. |
| Demonstrate, correct and reuse a workflow | New gap; reusable foundation | Existing personal/global Skills store instructions and supporting files. No opt-in demonstration capture, correction-to-skill review or Dot-specific workflow teaching surface exists. |
| Attention triage and stalled-work follow-up | New gap; runnable foundation | Schedules and MCP reads can produce a triage report today. Inbox holds pending decisions; it does not represent a durable evidence-backed attention list with acknowledgment, deduplication and follow-up lifecycle. |
| Work progress, failure and retry | Implemented foundation; product refinement | Shared SSE, stopping, tool results and run histories exist. Webhook replay may repeat side effects. A consolidated Dot progress/recovery view and safe automatic retry contract are not implemented. |
| Reusable Dot configuration templates | New gap; reusable foundation | Skills can be authored/exported and agents have spec revisions. A Dot blueprint preview/install flow with explicit resource mapping and no private-state export is not available. |
| Organization agents and team governance | Planned | System principals, grants, shares and audit exist. Groups do not own Dot data; organization ownership, responders and organization/Dot credentials require explicit modeling. |
| Computer network and data controls | Partial foundation; planned review | Runner isolation, default-deny egress/proxy controls and gateway outbound guards exist. A model-request content guard is not a computer-wide DLP system. Dot-specific certificate lifecycle and an equivalent DLP guarantee have not been established. |
| Separate Dot billing allowance | New optional requirement | Existing user/principal usage and limits are reusable. A separate Dot allowance/dashboard is absent; external subscription packaging does not justify adding a second billing system. |
| Slack channel, native desktop/mobile client, external bot runtime | Excluded | The requested product works through AIplane's web interface and own model/runtime infrastructure. Existing generic integrations do not make a Slack-facing Dot feature part of this scope. |

## Recommended additions

These are AIplane design recommendations, not claims about undocumented Grok
internals. Prioritize the existing isolated workspace and consent-based
communication proposals, then add the smallest complete user flows below.

### 1. Preserve isolation and enable explicit project resources

Keep the adopted per-Dot file and browser-profile boundary. User authority is
not permission to expose every browser login to every Dot. A shared project
should name its resources and permitted participants explicitly; no implicit
mount sharing, credential copying or cross-Dot memory access.

Acceptance: a second Dot cannot read the first Dot's files or profile by
guessing an ID or invoking a delegated task. An explicitly shared resource has
a visible owner, participants and revocation behavior. Implement sharing only
after the underlying isolated storage is proven.

### 2. Add a web computer handover for authentication

The concrete preparatory design is [Browser handover](dots-browser-handover.md).

Extend the existing runner/browser lease and lifecycle rather than introduce a
second computer service. Define a web session where the owner can inspect and
temporarily control the actual Dot browser. The Dot pauses computer actions
during human control and resumes only after an explicit handback. Tie the
session to the authenticated owner, Dot and current lease, with expiry and
revocation.

Passwords, one-time codes and session secrets must not enter chat history,
memory, teaching traces, screenshots retained as training evidence or audit
payloads. Reuse the existing sensitive-value machinery only after verifying
that its storage and model exposure satisfy this boundary; a password input
widget alone does not establish it. First support browser authentication, not
an unrestricted desktop. No transport or remote-desktop library is selected
by this comparison.

Acceptance: an owner completes a real login in the isolated browser, future
authorized runs retain that session, other Dots cannot use it, and takeover
and handback cannot race autonomous clicks.

### 3. Teach workflows through existing Skills

The concrete preparatory design is [Workflow teaching](dots-workflow-teaching.md).

Offer opt-in teaching with visible start, pause and stop controls. Initially
use user-written steps and selected non-sensitive tool observations; do not
record a user's whole desktop or account activity. Produce a draft in the
existing personal-skill editor, showing sources, parameters, required tools
and consequential steps. The user corrects and saves it, then explicitly
selects it for the Dot and configures an existing schedule if needed.

Extend the existing Skills APIs and access checks; do not create a parallel
routine engine or scheduler. A saved instruction does not grant its tools or
approve an external write. Tool rules remain explicit user choices. Validate
that the Dot's selected `read_skill` access and existing skill grants permit
the saved skill before offering execution.

Acceptance: a taught, reviewed workflow runs in a fresh authorized Dot chat,
accepts corrections through the same skill editor and still pauses at required
write approvals. Login steps and secrets are absent from the saved package.

### 4. Develop coordination separately from child delegation

First implement the adopted request/reply boundary. A later group flow should
use existing chat turns, SSE, worker lifecycle, RunChain, child results and
Inbox. Define explicit project/thread participants, who owns a task, how a
coordinator assigns it, and how the user can inspect the resulting work.

Group membership must not imply delegation consent, tool authority or access
to private history. Send only deliberately selected context; retain each
receiver's permissions and approval authority. Parallel specialist tasks need
bounded fan-out and shared spending limits as well as depth/cycle checks.

Acceptance: a coordinator assigns two authorized tasks, their outputs remain
attributable, failures are visible, the user can stop work, and removal of a
participant prevents further delivery. A renamed single-child tool does not
satisfy this flow.

### 5. Extend Inbox and existing run history for attention and recovery

Keep human decisions in the existing Inbox. Add a separate item type there
only after defining a real caller and lifecycle for findings: source link,
evidence timestamp, reason, next step, owner acknowledgment and dismissal.
Reuse existing schedules/events to refresh authorized sources. Suppress
unchanged findings and let users choose the cadence; no fabricated urgency or
continuous surveillance by default.

Distinguish waiting for approval, missing authentication, retryable failure,
terminal error and completion using actual recorded facts. Surface attempts
and errors in the existing run/chat/activity views. Automatic retries require
action-specific idempotency or a verified absence of side effects; otherwise
offer an explicit human retry with its risk visible. A replay button alone is
not evidence of safe retry.

### 6. Curate memory and offer reviewed blueprints

Use the existing scoped memory stores and side-call metering for bounded
curation. Before implementation, settle authorized evidence, retention,
cadence, conflict handling and how users inspect, correct or erase inferred
notes. Preserve source/provenance where needed; never promote untrusted
instructions into authority or broaden permissions because a Dot appears
successful.

For a later reusable blueprint, preview only selected instructions, skills,
tool references and trigger definitions. Exclude user/Dot memories, transcripts,
workspace contents and credentials by default. Installation maps resources
against the recipient's actual catalog and keeps triggers disabled until
explicitly enabled. Use existing spec revisions and Skills export rather than
a new plugin dependency. Team/public sharing needs a separate data-exposure
decision; it is not authorized by this research.

## Shared mechanisms and source map

| Existing mechanism | Relevant source / design |
|---|---|
| Ownership, selected tools and live grants | `aiplane-agents/src/db/{agents,dot_sessions}.rs`; runtime `persona/{personal_dot,live}.rs`; `GrantedToolSource`; [Dots](dots.md). |
| Durable background work and decisions | runtime `server/{headless,background}.rs`, scheduled worker, webhooks; `chat_turn_suspensions`; shared worker registry; [Background contract](dots-background.md). |
| Browser and persistent storage extension | runtime `server/tools/sandbox/browse.rs` uses `new_context`; runner backend/pool; [Workspace proposal](dots-workspace.md); [Sandbox](sandbox.md). |
| Connected personal browser | [Browser control](browser-control.md); existing browser action audit and extension pairing. |
| Local delegation and durable child pauses | runtime `agents/{router,resume}.rs`; `RunChain::enter` and `MAX_DEPTH = 3`; [Communication proposal](dots-communication.md); [Agent runs](agent-runs.md). |
| Reusable instructions | features `server/skills.rs`; tools `read_skill.rs`; runtime principal-aware skill catalog; [Skills user flow](guide/tools-and-integrations.md#author-a-personal-skill). |
| Attention, progress and stopping | [Automation and Inbox](guide/automation-and-inbox.md); existing schedule/webhook histories, chat JSON-SSE and cancellation. |
| Usage and administrative boundaries | [Access](admin/access.md), [content guard settings](admin/settings.md#access-and-content-guard), [agent observation](agent-guide/run-observe.md). |

No code, migration, API contract or permission has changed through this
comparison. Implemented-path evidence comes from source inspection; tests and
browser verification of the ongoing background slice remain the coordinator's
separate acceptance work.
