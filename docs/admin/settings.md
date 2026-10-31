# Configure system features

Open **Administration → Settings** (`/admin/settings`) with [administrator rights](access.md). The categories are Chat, Tools, Data, Access, Notifications and Web search. Settings cards show the effective configuration from the server; model pickers use models available for the required pool kind.

## Change a setting

1. Choose the category and find the feature's card.
2. Read its prerequisites and set its model, service URL or directory before enabling it.
3. Save that card. Cards save independently.
4. Check the success message and restart-pending banner.
5. For restart-required fields, restart through your deployment's normal process and verify the feature afterwards.

Most fields reload without restart. The declared restart fields are `comfyui.base_url`, `comfyui.content_dir`, `rag.enabled`, `rag.data_dir` and `rag.clone_concurrency`. The UI also reports its pending list explicitly. A disabled feature does not become usable merely because its service URL is filled in.

Settings are stored in the database. Secrets are write-only: the UI shows whether one is set, not its plaintext. Where a stored secret is long enough for a fragment to be safe, the UI shows a head/tail fingerprint (for example `tvly-…f99`) so you can tell which key is stored. Leaving a secret input blank during Save preserves the stored value. Use its explicit Clear action, with confirmation, to remove it. Clearing a stored setting uses its built-in fallback; it does not re-import an arbitrary value from a configuration file.

Save validates typed settings before storing them. Invalid whole numbers, negative integer values, non-numeric or non-finite values, unlisted choice values, and IP-list entries that are not an address or a CIDR network show an inline error and leave the stored setting unchanged. Decimal settings accept either a period or a comma as the decimal separator.

The [field reference](settings-reference.md) lists every declared field and its meaning. The operating system's persistent paths and externally reachable services remain deployment responsibilities.

## Chat and documents

- **OCR:** Select an OCR-pool model and configure document/page/output limits, resolution, timeout, concurrency and the text threshold used to identify scanned PDFs. This reads document text; it does not provision an OCR upstream.
- **Compaction:** Control when long conversation history is summarized, the assumed context window for unknown models, recent turns preserved verbatim, minimum conversation length and summary token budget. Verify model-specific context windows in the [catalog](models.md).
- **Parallel turns:** Set how many different conversations a user may run simultaneously. The default is one.
- **Loop retries:** When a model collapses into repeating itself (repeated text, or the same tool call over and over), the call is stopped either way. This sets how often the answer is then tried again, one thinking level lower each time — default 2, at most 5, 0 to stop at the first loop. A level the model cannot express, or one that would send the same request again (OpenAI sends `high` for **high** and **xhigh**), is skipped; a model with no reasoning control, or a turn already at **off**, is not retried. The allowance is per answer, the attempts are counted per tool round: a loop in a later round, after an earlier one was retried, is that round's first attempt. A retry after a repeated tool call does not reset that call's count, so a tool with side effects runs no more often than without retries. Retries apply to chat turns and to scheduled and agent runs, which use the same driver. The stopped attempts stay visible, collapsed, in the answer; each is a metered model call ([counting loops](../operations/troubleshooting.md#count-loops)).
- **Attachments:** Configure the S3 endpoint, region, bucket, key prefix and credentials in Data. Enable attachment storage only with a working bucket.
- **Welcome text:** `chat.welcome.text` is shown on a card centred in an empty conversation before the first message. It is plain operator content (Markdown supported), not a translated UI string; empty means nothing is shown, and a built-in default ships until the operator stores a value. Placeholders `{name}` (display name, falling back to the address local part), `{first_name}`, `{email}` and `{language}` (interface language code) are filled in per person; values are Markdown-escaped, so a name cannot inject markup. Text wrapped in `<prompt>…</prompt>` becomes a chip that sends exactly that text in one click; the tag renders only in this text, never in a model's answer.

OCR uses the document-aware sidecar under `deploy/ocr-sidecar` through a dedicated `ocr` pool. The sidecar owns PDF rasterization and model inference; AIplane owns derivative caching, configured limits, queued/running/completed/failed state and usage accounting. OCR models are excluded from user-facing chat model lists. A generic chat endpoint is not sufficient for this internal multipart contract. Extracted text is treated as untrusted document content. Verify a scanned document and its page ordering after configuration.

## Tools and data services

| Card | Configure before enabling | Where to check the result |
|---|---|---|
| Sandbox | Private sandbox-runner URL, deadline and returned-file size cap | A permitted code tool in chat |
| ComfyUI | Private worker URL, workflow directory and job limits | [Workflow catalog and Jobs](integrations.md#operate-comfyui-workflows) |
| Typst | Template directory available to AIplane | Document/PDF tools |
| GeoIP | IP2Location database path and optional update token | Location tool |
| RAG | Persistent index directory and indexing concurrency | [Knowledge collections](knowledge.md) |
| Skills | Readable/writable bundle directory | [Skill management](integrations.md#install-and-grant-skills) |

Sandbox-runner and ComfyUI are privileged services. Their settings help explicitly requires making them reachable only from the gateway. Adding a service is separate from granting its tools to users and agents.

The sandbox gateway client sends code execution to the standalone runner; runner configuration enforces actual isolation, egress policy and resource caps. Generic code execution can reuse a working directory during one turn. Test stdout/stderr and an artifact with an allowed account. File delivery in browser chat also needs configured attachment storage. The gateway's `sandbox.timeout_secs` is an HTTP deadline, not a replacement for runner resource limits.

Typst rendering needs the `typst` executable on the AIplane process PATH. One subdirectory per template contains `template.typ`, `template.toml` and its local assets. The manifest defines the template's ID, title/description, input fields and output basename. A settings save rescans the configured template directory and rebuilds its tool surface. The renderer applies a 30-second compile timeout and a 25 MiB PDF size cap. Optional editable office exports use the sandbox. Verify the declared template inputs and rendered output before granting the `typst_<id>` tool family.

GeoIP reads an IP2Location LITE DB11 BIN file. Its optional token supports database updates; the file can be reloaded when changed. Client-IP location is approximate and fields may be missing. Browser-shared position is separate user-provided data. Behind a proxy, configure trusted proxy handling so the gateway uses the actual client address; a missing/private IP or unavailable database must not be interpreted as a precise location.

Moving the RAG data directory does not move existing indexes. Review persistent storage before changing it. A profile, credentials or an embedding model is configured on the collection, not on the global RAG card.

## Access and data classification

The Usage card enables per-request accounting, retention and display currency. The Limits card enables enforcement of rules from `/admin/limits`. The agents card sets `agents.max_active_per_user`, the maximum number of active personal agents each user may own, including idle agents; the clean-install default is four. The same card sets `agents.max_active_per_organization`, independently defaulting to four active agents per organization, including idle agents. It has no feature master switch. Administrators separately grant **Personal agent manager** in the [group form](access.md); the limit does not grant management rights. Lowering the cap does not deactivate existing agents, but blocks new agents or reactivation until the active count is below the new cap. Gateway settings control token/session lifetimes and impersonation. See [access management](access.md).

### Prometheus metrics

The **Prometheus metrics** card controls the `GET /metrics` scrape endpoint. It has three fields:

- **Serve /metrics** switches the endpoint on.
- **Scrape token** is the secret a scraper sends as `Authorization: Bearer <token>`. It is write-only like every secret.
- **Allowed IPs** lists the addresses and CIDR networks, such as `10.0.0.0/8, 2001:db8::/32`, that a scrape must come from.

The endpoint answers only when it is switched on and at least one of the token and the IP list is set; otherwise it answers like a path that does not exist (`404`). With both set, a scrape must pass both. A client address outside the list gets `403`; a missing or wrong token gets `401`. Saved changes apply to the next scrape without a restart. Behind a reverse proxy, the IP list works only when `AIPLANE_TRUSTED_PROXIES` names the proxy; otherwise every scrape arrives from the proxy's address. The IP list guards only `/metrics`, not other gateway access. See [monitoring](../operations/monitoring.md) for the scrape configuration and the metrics.

### Data classification

**Data classification** ranks how sensitive a conversation is. It is defined by an ordered set of **levels** (lowest rank first), **data classes** that the system recognises, **markings** that map a document's label or stamp to a level, and **assignments** that give a pool, tool, connector or knowledge collection its level. See [data classification](../classification.md) for the data model and the API.

It is edited on **Administration > Data classification** (`/admin/classification`), not on the settings page. Until a scheme is active the page offers the setup assistant. It asks three things: the standard (ISO, TISAX, BSI, TLP or a custom scheme), which level each model pool may see (prefilled from where the pool's backends run: in the own network the highest level, in the cloud the lowest, unrated when a host does not resolve) together with the detector model that checks content, and then Activate. Everything else comes from the standard and is edited afterwards on the tabs. Without a detector model the questions of the data classes stay in the scheme but are inactive (the classes tab marks them) and only patterns, markings and word lists apply; they take effect once a model is chosen. On the data classes tab every class is a collapsed card showing its name, level, action and chips for what detects it (each built-in pattern, the rule question, the word list); the full editor opens inside the card. Afterwards it shows the scheme on tabs (levels, data classes, markings, destinations, output, policy, linkable as `?tab=<name>`). Edits on every tab belong to one draft, and **Save** stores the whole scheme in one step; a refused save names the field and changes nothing.

A fresh installation has a single level, `public`. With one level the feature is inactive: every model is offered, no question is asked and the chat shows no level. It becomes active once the scheme has a second level. **Templates** fill the editor with a starting point: ISO 27001 (public, internal, confidential, strictly confidential), TISAX (including the German stamps automotive documents carry), BSI (normal, high, very high) and TLP 2.0 (clear, green, amber, amber+strict, red). A template replaces the levels, classes and markings in the editor only; nothing changes until you save, and each template can be adjusted before saving. Every template brings the same six data classes: personal data, special categories, trade secrets, payment data, tax identification number and credentials and keys. The last one is a pattern-only class at the template's highest level that asks for confirmation; it has no rule question. Template, level and data class names and descriptions are shown in your language and become the scheme's own text once saved, so changing the interface language later does not retranslate them. The data classes' rule questions stay English, because the detector models are measured on English prompts; stamps, label names and filename tags stay as the documents carry them.

- A **destination's level** is the most sensitive data it is cleared for. A destination without an assignment is at the lowest level. A model served by several pools is cleared only to the lowest of their levels. Pools, knowledge collections and tools that keep data inside AIplane can only be hidden above their level; a connector or a tool that sends data out of AIplane can also be set to **gate**, which keeps it available and asks the person to confirm.
- A **conversation's level** starts at the policy's *default level* and only rises while the conversation is used. The model picker lists only models cleared for the conversation's level, and the gateway refuses a turn that targets a model that is not, so a direct API request cannot bypass the picker.
- **Lowering** follows the policy: **Never** refuses it; **With a reason, audited** accepts it with a non-empty reason. Every raise and lowering is recorded with its cause and actor and listed under **Events**.
- A **data class** matches by built-in patterns (e-mail, IBAN, payment card, tax id), by an uploaded **word list** and by a question the detector model answers (the *detector model* of the policy). A class matches at its score threshold (default 0.5). The strongest action of the matched classes decides: **raise** lifts the conversation to the class's level, **confirm** asks the person in the chat, **block** refuses the request. Content is sent to the detector model, so choose a pool whose provider arrangement suits that content. The detector is a System One model (recommended) or, as a fallback when no System One pool exists, a chat model; the editor lists them in two groups. A chat model is slower, costs a model call per checked message, is billed to the person whose message is checked and is less robust against manipulated content, which is why the content is wrapped in per-call random markers, content imitating a marker is never sent to the model and counts as a match of every questioned class, and the answer must follow a strict schema (details in [data classification](../classification.md#the-detector-model)). The model must itself be cleared for the highest level when any class has a question or any destination is set to gate.
- **Word lists** are uploaded per class: one term per line, previewed with the counts of duplicates and of terms under 3 characters (which are left out) before the list replaces the previous one. A saved scheme keeps the word lists of the classes it keeps.
- **Markings** map a Purview label (GUID), a TSCP or sensitivity label or a visible stamp to a level. **Inspect a file** shows which markings a document carries, so you can copy the value into a marking. **Test** classifies a sample file or text with the saved scheme without creating a conversation or an event; it asks the detector model only when one is configured.
- **Removing a level** moves each conversation held at it to the next higher level that remains (the highest level when none is above; a conversation at the removed lowest level goes to the new lowest level) and records the move with the cause `scheme_change`. A word list survives for every class that keeps its id.
- **Users may override the model clearance** (the policy's *People may send to a model that is not cleared*, on by default): when a person confirms content a data class asks about, they may still send it to the selected model even if it is not cleared for the conversation's level. Turned off, the confirmation only covers the content and the turn is refused until a cleared model is chosen.
- **Link sharing**: a conversation above the policy's *highest level for link sharing* cannot be shared by link, and a link already shared stops working as soon as the conversation is raised above it. An empty or unknown value means the lowest level; with one level there is no limit.
- **Monitor** mode logs what the detector finds and a failed evaluation, and changes nothing. **Enforce** applies the action, and a detector failure refuses the request. The levels already held by a conversation and the model filter apply in both modes.

The detector runs after the caller's [limits](access.md#set-request-token-or-cost-limits), so an over-budget request is never sent to it. The detector request has a five-second timeout. A confirmation in browser chat needs a connected prompt receiver and an affirmative answer within three minutes.

An upgrade converts the previous GDPR and NDA pool flags, the content guard and sticky confidential conversations into levels and classes, which you then edit on the same page. Classification is a model-based check and an operator declaration: it is not a certification and does not guarantee that every sensitive passage is detected.

## Notifications and feedback

Configure Feedback with the selected GitHub or GitLab tracker, its issue destination, credentials and labels. GitHub screenshot assets have a separate assets branch. Set the extraction and voice models when those feedback inputs are needed. Stored credentials remain write-only; switching the selected tracker leaves the other tracker's configured fields available.

Push settings enable browser push support and set the VAPID contact identity. Users still need to enable notifications in their own browser/settings. Agent Slack/Discord webhook channels are configured on the agent's [Sharing screen](../agent-guide/run-observe.md), not on this global card.

## Web search

The Web search category has its own provider card. Choose the provider, set the SearXNG URL or Brave API key as appropriate, and configure the optional Tavily settings. Existing API keys are preserved by blank inputs; use each clear-key control to remove one. Save and test a permitted search tool in chat. Selecting a provider does not grant the tool to users.

## Branding

The Branding category is a single card with one slot per theme: the light logo shown on the light theme, and an optional dark variant. Upload an SVG or a raster image (PNG, JPEG) per slot; the gateway resamples raster uploads down to 1200 px on the longest side and stores them as PNG, while SVG passes through unchanged. A stored image is capped at 512 KB, and an upload over the cap after resizing is refused. The logo appears in the app header, on the login screen and as the browser tab icon; removing a slot returns the built-in mark for that theme. The public read routes are documented in [the API reference](../reference/api.md#branding).

## Troubleshooting

| Symptom | Check |
|---|---|
| Model menu empty | Backend health and correct pool kind |
| Feature enabled but unavailable | Feature prerequisites, process-readable directory and tool grants |
| Old worker or index path still in use | Restart-pending list and deployment restart |
| Secret seems blank | Set/unset indicator plus a head/tail fingerprint; write-only values are intentionally not returned in full |
| Data classification rejects everything | Detector service error, enforce mode and detector model availability |
| Confirm action fails without a prompt | Connected browser chat versus API caller; three-minute deadline |
| New RAG path has no indexed data | Indexes do not move with a settings change |

## Request prioritization

Under **Access → Gateway**, enable `gateway.priority_enabled` and choose `gateway.default_priority` (Low, Medium or High; initially Medium). Saved settings apply to new admissions. High-priority requests take the next available slot before Medium and Low; arrival order applies within each class. Sustained high traffic can cause lower classes to reach the existing upstream wait timeout. Running requests retain their slots. Assign group and token priorities in [Access](access.md), and configure backend forwarding in [Models and routing](models.md). See [Request priority](../request-priority.md).
