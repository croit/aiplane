# Tool inventory

This inventory explains which tools are available, when they appear, and how
their switches work. An administrator configures the required service and
grants access; people can then choose tools for a conversation. See the
[tool contracts reference](reference/tools.md) for arguments, results and
limits, or [tools and integrations](guide/tools-and-integrations.md) for the
browser workflow.

## How to read the columns

- **Gate** — the configuration or service that must be available before the
  tool appears.
- **Chat-only** — the tool needs an AIplane conversation and is unavailable to
  direct model API requests.
- **Toggle key** — the switch an administrator can grant and a person can
  enable for their conversation. Related tools may share one switch.

The `/tools` UI shows admins read-only rows for optional image, GeoIP, and
sandbox tools when their service is absent. Their switches are disabled and
link to the relevant setup page. Tools that need storage, knowledge indexing,
or notification setup also show when that prerequisite is missing.

## Always registered

No configuration required. A tool here can still fail at runtime when its
*storage* is unconfigured (e.g. S3 attachment storage), but it fails with a clear
message rather than being absent.

| Tool | Chat-only | Toggle key | What it does |
|---|---|---|---|
| `enable_tools` | — | *(hidden, always on)* | The lazy-disclosure bootstrap: turns other tool groups on for the rest of the conversation. Not presented as a toggle — `allowed_tools_for_session` force-keeps `BOOTSTRAP_TOOL_ID`, so a switch for it would be inert. |
| `search_gateway_tools` | — | *(hidden, `/v1` only)* | Searches the Auto capabilities allowed by an API token and exposes at most five matching tool schemas in the next model round. |
| `company_echo` | — | *(hidden)* | Diagnostic echo tool for checking a custom installation's tool connection. It is hidden from normal tool selection. |
| `get_current_timestamp` | — | `get_current_timestamp` | Timezone-aware current date/time, from the caller's `users.timezone`. |
| `convert_currency` | — | `convert_currency` | Currency conversion at daily ECB reference rates. |
| `ask_user` | yes | `ask_user` | Ask the user a short question mid-turn and wait for the answer, rather than guessing. Needs a live chat turn *and* someone watching it; times out and reports `answered: false` otherwise. |
| `forward_request` | yes | `forward_request` | An active personal Dot sends an explicit task to another active Dot it may address: a personal Dot belonging to its owner (both directed consents), or an organization Dot the caller's member organization consented to receive from (cross-boundary consent plus the caller's current membership). The receiver uses its own tools, memory, credentials and approval rules. Ordinary chats and stateless `/v1` requests do not offer this personal variant. See [Dot communication](dots-communication.md) and [cross-organization delegation](dots-cross-organization.md). |
| `notify_user` | — | `notify_user` | Send the user a Web Push notification (long work finished, a scheduled action found something). Deliberately *not* chat-only — a notification lands on a device, not in a conversation, so it also works from the headless scheduler. Hard limit of **one per turn**, latched in `PushNotifier`; errors clearly when Web Push is unconfigured or the user has no subscribed device. |
| `get_user_location` | — | `get_user_location` | Caller's location: a browser GPS prompt when a live chat turn is watching, else coarse GeoIP. |
| `generate_qr_code` | yes | `generate_qr_code` | QR codes (URL, WiFi, vCard, SEPA) as PNG/SVG, rendered in-process. |
| `search_web` | — | `search_web` | Web search via SearXNG, Brave, or Tavily, with optional domain and recency filters. A confirmed exhausted provider quota triggers another configured provider; the result identifies the one used. Tavily has a separate Enable switch and is eligible only when enabled with a stored key. Configured on `/admin/settings?tab=web-search`. |
| `browser_control` | yes | `browser_control` | Act in the user's own logged-in browser (navigate, read, click, type, screenshot) through the extension paired with the open chat page. Actions travel as a batch and run in order. The extension, not AIplane, decides what may run; nothing happens until the user switches it on. Users set it up at `/tools/browser`. See [tools and integrations](guide/tools-and-integrations.md). |
| `show_screenshot` | yes | `show_screenshot` | Show the user a capture of their browser page — viewport, whole page, one element (`ref`) or a rectangle (`region`) — as an inline attachment in the reply. Browser-control screenshots are for the assistant; this tool shows a capture to the user. It uses the browser extension and needs attachment storage. See [tools and integrations](guide/tools-and-integrations.md). |
| `fetch_url` | — | `fetch_url` | HTTP GET through `outbound_guard` (public hosts only unless `$AIPLANE_ALLOW_PRIVATE_NETWORKS`; every redirect re-checked; 32 MiB read cap, 4 MiB returned-text cap). HTML is reduced to readable text unless `raw` is set; images come back viewable; other binary returns metadata. |
| `wikipedia` | — | `wikipedia` | Summary of the best-matching Wikipedia article. |
| `dns_lookup` | — | `dns_lookup` | DNS records over DoH. |
| `whois_lookup` | — | `whois_lookup` | Domain registration via RDAP. |
| `tls_cert` | — | `tls_cert` | TLS certificate inspection (issuer, validity, days to expiry, SANs). The host is checked through `outbound_guard` like a `fetch_url` URL. |
| `fetch_attachment` | — | `fetch_attachment` | Read any file of the conversation — an attachment or (by id/title) a canvas document, which comes back as text plus its version. Tiered PDF reading — text layer, rasterised pages, `mode="ocr"` (gateway OCR, images too), `mode="auto"` (text-or-OCR) — with `page_from`/`page_to`; Office files return structured content. |
| `upload_attachment` | yes | `upload_attachment` | Attach a model-generated file to the reply. |
| `offer_download` | yes | `upload_attachment` | Hand a file the conversation *already holds* to the user as a download chip on the current reply — an attachment, or a canvas document (its current version is written out as a file, named from the document's title) — including objects with no chip of their own (a typst render's hidden `.json` data base, an intermediate artifact) and files from earlier turns. Takes a reference, never content: the object is copied inside S3, so a large payload never round-trips through the model as prose. Session-scoped twice over — a marker-backed id is proven in-session by the enumeration, an unlisted `<turn_id>/<filename>` by `turn_in_session`. Shares the `upload_attachment` toggle: one switch for "let the assistant hand me files". |
| `zip_attachments` | yes | `upload_attachment` | Bundle several files the conversation already holds into one `.zip` and attach it as a single download chip. Takes the same reference spellings as `offer_download` (`<turn_id>/<filename>`, canvas `document_id`, bare filename) in `ids`, plus an optional archive `filename` (`.zip` appended when missing). Unlike `offer_download` the bytes cannot stay inside S3 — the archive is assembled in memory — so it is capped at 64 entries and 256 MiB of uncompressed members, and members are read sequentially so the first bad id is the one reported. Colliding entry names are suffixed (`report.md`, `report-2.md`) rather than overwriting each other; a canvas document contributes a snapshot of its current version. Shares the `upload_attachment` toggle. |
| `list_attachments` | yes | `list_attachments` | Inventory of the conversation's files, so assets get reused rather than regenerated. |
| `load_image_url` | yes | `load_image_url` | Fetch an image from a URL (through `outbound_guard`, like `fetch_url`) and keep it as a reusable conversation attachment. |
| `import_file` | yes | `document` | Turn a text attachment (upload or produced artifact) into an editable, versioned canvas document — server-side, so the content never round-trips through the model. The on-ramp that makes an uploaded `.typ`/`.csv`/`.json`/`.md` editable a passage at a time (and hand-editable by the user); `offer_download` is the exit ramp. Text formats only: binary attachments stay attachments, already usable by id (`att:` refs, sandbox staging, `fetch_attachment`). Capped at the same 512 KB the document tools can write. |
| `create_document` | yes | `document` | Open a canvas document. |
| `edit_document` | yes | `document` | Apply passage edits to text-format canvas documents, or JSON Patch to JSON/TOML documents; every edit creates a version. |
| `edit_document_section` | yes | `document` | Edit one section of a canvas document. |
| `read_document` | yes | `document` | Read a canvas document back. |
| `list_documents` | yes | `document` | List the conversation's canvas documents. |
| `list_document_versions` | yes | `document` | Version history of a canvas document. |
| `restore_document_version` | yes | `document` | Roll a canvas document back to an earlier version. |
| `delete_document` | yes | `document` | Soft-delete a canvas document: hidden from the canvas and from `list_documents`, version history kept, reversible. |
| `undelete_document` | yes | `document` | Undo a `delete_document`. Deliberately *not* named `restore_document` — that would sit one suffix away from `restore_document_version`, which does something else entirely. |
| `schedule_action` | yes | `schedule` | Create a recurring prompt (5-field cron + IANA timezone, defaulting to `users.timezone`). Validates with `Cron::parse` and returns `describe()` + the next 3 run times, so a wrong-but-valid expression is caught before the first run. Inherits the turn's model from `ToolContext.model`; **always** creates the action with tools off. |
| `list_scheduled_actions` | — | `schedule` | The caller's own scheduled actions, each with the same human schedule preview. Read-only, so it works off-chat. |
| `delete_scheduled_action` | yes | `schedule` | Delete one of the caller's actions. Another user's id is reported as missing, not forbidden (no existence leak). |
| `rag_search` | — | `rag_search` | Hybrid search (dense kNN fused with FTS5/BM25) over an indexed collection. Returns chunks with path, line range, score. Optional `path_glob` scopes it to part of the corpus — filtered inside the query on the lexical side, and after kNN candidate generation on the dense side (which is why the candidate pool widens when it is set). |
| `rag_grep` | — | `rag_grep` | Regex scan over an indexed collection's chunk text: matching lines with file, line number and context. For patterns BM25 cannot express (`TODO\(.*\)`, `impl .* for Tool`). Full scan with no index behind it, so it is bounded by result / row / wall-clock limits and reports which one it hit. |
| `rag_list_collections` | — | `rag_list_collections` | Discover which collections exist before searching them. |
| `rag_query_documents` | — | `rag_query_documents` | Filter, sort and total the documents of a collection by the fields its extraction profile pulled out (vendor, date, amount, project). The answer to any question about a *set* of documents — the latest, the largest, how many, how much — which top-k passage retrieval structurally cannot give: it returns five similar chunks and no way to know whether that was all of them. Always reports `total_matches` alongside the returned page, and surfaces the distinct values a text filter matched so `ACME` hitting both `ACME GmbH` and `ACME Deutschland AG` is visible rather than silently resolved. |
| `rag_list_documents` | — | `rag_query_documents` | Folder-scoped document listing with the **stored** per-document summaries written at index time. Turns "find everything about project X and summarise it" into one call over ~200 tokens per document rather than re-reading every file. |
| `rag_fetch_document` | — | `rag_query_documents` | Full extracted text of one indexed document by `document_id`. The drill-down after a hit; truncates long documents with a note rather than flooding the context. |
| `remember` | — | `memory` | Persist a durable fact for the user or acting Dot. Personal and organization Dot writes remain in that Dot's own scope; new content is bounded to 2,000 UTF-8 bytes. |
| `recall` | — | `memory` | Read at most 200 recent notes with explicit scopes. A personal Dot reads own and user notes; an organization Dot reads own and shared organization notes, without employee profiles. |
| `update_memory` | — | `memory` | Correct an authorized fact by its returned ID and scope, within 2,000 UTF-8 bytes. Dots can change only their own notes. |
| `forget` | — | `memory` | Delete an authorized fact by ID and scope. Dots cannot delete user or shared organization notes. |

The canvas tools, the four memory tools and the three scheduling tools
collapse to the `document`, `memory` and `schedule` keys respectively — see
`DOCUMENT_IDS` / `MEMORY_IDS` / `SCHEDULE_IDS` in `catalog`. Turning `memory`
off has to remove the mutating tools too, not just the read/write pair; the
same reasoning groups `list_scheduled_actions` with the two tools that change
a schedule, since "can see my schedule but not change it" is not a
distinction anyone configures.

`schedule_action` and `delete_scheduled_action` are chat-only for a reason
that isn't about attaching output: both wait for the person's approval,
because the action they write later runs **as the user**, unattended, until
removed. That makes a scheduled action a persistent prompt-injection vector,
so a human has to approve it. The approval is the durable pause every
approval uses (`ask_first::approval`): the call shows a preview of the
schedule, the person answers in the chat or the inbox, and it survives a
restart. A scheduled run that calls them pauses too, and nothing is written
unless its owner approves from the inbox.

## Conditionally registered

Gated so the model is never offered a tool whose every call could only answer
"not configured".

| Tool | Gate | Chat-only | Toggle key |
|---|---|---|---|
| `lookup_ip` | GeoIP configured (`geoip.enabled`) | — | `lookup_ip` |
| `generate_image` | an `image`-kind upstream pool exists | yes | `generate_image` |
| `edit_image` | an image backend advertises `supports_edit` | yes | `edit_image` |
| `read_skill` | skills configured (`skills.enabled`) | — | *(always on when the caller has a permitted skill)* |
| `run_in_sandbox` | `sandbox.enabled` | — | `run_in_sandbox` |
| `generate_document` | `sandbox.enabled` | — | `generate_document` |
| `export_document` | `sandbox.enabled` | yes | `document` |
| `convert_document` | `sandbox.enabled` | — | `convert_document` |
| `edit_presentation` | `sandbox.enabled` | — | `edit_presentation` |
| `capture_webpage` | `sandbox.enabled` **and** the runner reports egress | — | `capture_webpage` |
| `browse_page` | `sandbox.enabled` **and** the runner reports egress | — | `browse_page` |
| `render_typst` | `sandbox.enabled` | — | `render_typst` |
| `render_excalidraw` | `sandbox.enabled` | — | `render_excalidraw` |
| `render_video` | `sandbox.enabled` | — | `render_video` |
| `read_sandbox_output` | `sandbox.enabled` | yes | `read_sandbox_output` |

`export_document` is the one sandbox tool that rides the canvas `document`
toggle: it exports a canvas document, so it belongs to that capability from
the user's point of view even though it needs the sandbox to run.

### Egress is a *runner* capability, not gateway config

Whether a sandbox can reach the network is decided by the **runner's**
deployment (`SANDBOX_EGRESS_NETWORK` + `SANDBOX_EGRESS_PROXY`), which nothing
in AIplane's own config can see. So AIplane asks: at boot it reads
`GET /healthz` on the runner, which reports `egress: true|false`
(`shared::sandbox::RunnerHealth`), and remembers the answer for the process
lifetime.

That answer changes what the model is offered:

- **No egress** → `capture_webpage` and `browse_page` are **not registered**
  at all, and `run_in_sandbox`'s schema has **no `network` property**, with a
  description that states plainly there is no network rather than implying a
  permission the model could ask for. This is the "absent beats
  always-failing" rule below.
- **Egress** → both tools register and `network` appears as an option.
- **Unknown** (runner unreachable at boot or health response without the egress field)
  → treated as *available*. Withdrawing capabilities needs positive evidence;
  an unreachable runner breaks every sandbox tool anyway, so hiding a subset
  would turn a transient outage into an apparent permanent capability loss.

Not re-probed: egress changes when an operator edits a unit file and restarts
things, and a capability set that shifted under a running conversation would be
worse than a slightly stale one.

**`render_typst` is deliberately not chat-only, even though part of it needs a
session.** It renders either inline `source` or a `document_id` from the canvas.
The inline path is the common one and works fine on `/v1`; only the
`document_id` path needs a chat session, and it fails there with a message
naming the reason ("canvas documents are only available inside a chat session")
rather than a generic error. Marking the whole tool chat-only would remove a
working capability from the proxy paths to protect an argument the model
wouldn't have a use for there — a `/v1` caller has no canvas to reference. Same
reasoning applies to `run_in_sandbox`'s optional canvas-document staging, which
degrades to a note rather than failing the run. `export_document` is different
and *is* chat-only: a canvas document is its only possible input.

### The two shapes a conversation's files come in

A conversation holds files in two stores, on purpose, and the tools cross
between them rather than duplicating either:

| | Attachments | Canvas documents |
|---|---|---|
| Address | `<turn_id>/<filename>` | `doc_…` |
| Mutable | no — one immutable blob per write | yes — every change appends a version |
| Content | any bytes (images, PDFs, archives) | UTF-8 text, ≤ 512 KB |
| User can edit | no | yes (the document panel) |
| Reached by | `fetch_attachment`, `att:` refs, sandbox `attachments`, `offer_download` | `fetch_attachment`, `read_document`/`edit_document`, sandbox `documents` *or* `attachments`, `typst_*` `document_id`/`base`, `export_document`, `offer_download` |

**One reference syntax, every tool.** A model should not have to know which
tool takes which way of naming a file. Every file-taking tool resolves through
`file_refs::resolve`, which accepts all of them — an `<turn_id>/<filename>` id, a bare filename (newest match wins), a
`doc_…` id, an unambiguous document title or its materialised filename, and a
leading `att:` / `doc:` / `file:` that some models add unprompted. So a
reference the model got from *any* result works in *any* argument, and a wrong
one produces the same message (naming both inventories) wherever it is passed.

Session scoping is part of resolution rather than a check each caller
remembers: a marker-backed attachment is proven in-session by the enumeration,
an unlisted `<turn>/<file>` by `turn_in_session`, a document by the
session-scoped `get_version`. Another conversation's real id resolves to the
same "not found" as a typo, so nothing leaks. A *deleted* document is its own
error, because the fix is `undelete_document` rather than a different id.

What is **not** a reference: a path inside the sandbox's working directory.
`docs/backend.md` is the same *shape* as an attachment id, so a model that just
wrote it in `/work` passes exactly that — and the generic "no file named that in
this conversation" reads as *your file is gone* rather than *you named the wrong
store*. A non-UUID first segment is therefore detected and answered with the
real explanation (`file_refs::looks_like_sandbox_path`): `/work` is unreachable
from outside a `run_in_sandbox` call, and a produced file becomes referenceable
only once that call returned it in `artifacts`, under the flattened name shown
there.

Crossing over: **`import_file`** turns a text attachment into a document;
**`offer_download`** writes a document's current version back out as a
downloadable file. Both copy inside AIplane — content never round-trips
through the model, which is what made "give me that file" cost two passes of
the whole payload and invite retyping drift.

The user can **hand-edit** a document in the panel (`PUT
/api/v0/chat/sessions/{id}/documents/{doc_id}`, owner-only, newest version only). That save
is a normal new version, marked as authored by the user — the panel's version
switcher labels those revisions (`v3 · you`) so a history stops reading as
interchangeable numbers, and `read_document` / `list_document_versions` /
`list_documents` report the author too — and the request
context then tells the model which documents were hand-edited and at what
version, because nothing in the transcript would: its history still holds the
content *it* wrote, so an unwarned edit reverts the correction. The warning
stops once the model writes on top (it has seen the change by then).

A **typst render** parks its field data in a canvas document (its
`document_id` comes back in the result) rather than a hidden
`<turn>/<basename>.json` attachment. That data is the file the model works on
constantly, so it must be visible: in the panel, downloadable, stageable, and
editable by the user. The `_read` / `_edit` / `_pptx`
tools take either id — a slash means an attachment — so a conversation whose
base is a `<turn>/<basename>.json` attachment keeps editing it, and
`_edit` writes back to whichever surface it read from. The render deliberately
does *not* push the panel open for a data document: the deliverable is the PDF.

All three refuse a **soft-deleted** document explicitly. `documents::get`
resolves deleted rows on purpose (see `delete_document`), so without that check
a stale id would quietly produce an export, a PDF, or a staged file from work
the user threw away.

## Dynamic families

These have no fixed id — one tool per discovered template / workflow /
connected server. The drift guard matches them by prefix. Auditing, approval
and live personal-Dot or organization-principal authorization wrappers retain the wrapped tool's actual
ID and schema; they do not add another tool or toggle.

| Tool IDs | When they are available | Toggle key |
|---|---|---|
| `typst_<id>` plus `_edit` / `_read` / `_pptx` | Each installed Typst template provides its own document and conversion actions. | `typst_<id>` |
| `comfyui_<id>` | Each enabled ComfyUI workflow provides a matching action. | `comfyui` |
| `mcp__<server>__<tool>` | Actions exposed by a connected personal or shared MCP integration. | `mcp__<server>` |
| Agent builder actions | Available to the assistant while creating or editing an agent, according to the builder user's permissions. | Managed by the agent builder |
| `set_<slot>` | Available during an agent run for slots the agent is allowed to fill. | Managed by the agent |
| `request_human` | Available when an agent can hand work to a person. | Managed by the agent |
| `finish` | Available when an agent has a result contract to satisfy. | Managed by the agent |
| `verify_<id>_request_code`, `verify_<id>_submit_code` | Available when an agent requires one-time-code verification. | Managed by the agent |
| `verify_<id>` | Available when an agent requires lookup-based verification. | Managed by the agent |

Published agents with configured sub-agent routes use a run-scoped
`forward_request` variant with the same ID. Its argument-free schema renders
the request from the agent's state and dispatches the configured route. That
variant is supplied by the existing agent run tool source; it does not use the
personal receiver/message schema.

Some document conversions also need the sandbox service and an enabled output
format. Connector-returned files become conversation attachments. For how to
configure these services, see [integrations](admin/integrations.md).

## Availability and access

Some tools need a configured service, such as image generation, geolocation,
knowledge indexing or the sandbox. Those tools appear when the service is
available. Other tools remain visible when setup is incomplete and show which
prerequisite is missing.

An administrator must grant access to a tool before it is available to a
person. The conversation switch controls whether the assistant can use an
available tool during that conversation. Tools that act through a connected
service can also require approval before they make a change. See [agent
permissions](agent-guide/permissions.md) and [approvals](guide/automation-and-inbox.md).
