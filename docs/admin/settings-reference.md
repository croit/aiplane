# System settings field reference

This reference covers every field declared by the system settings registry. Open `/admin/settings` with the `admin` role to see the effective values for your installation. Follow [Configure system features](settings.md) for save, secret and restart behavior.

Model fields use the catalog for the pool kind named in the table. Paths refer to the AIplane process filesystem. Restart is required for marked fields; other fields reload on save. The effective value shown by the application includes stored configuration; it is not necessarily a clean-install default.

## Field catalog

### content_guard

Category: **Access**.

| Key | Input / choices | Meaning | Restart |
|---|---|---|---|
| `content_guard.enabled` | Boolean | Check only the compliance area the selected model pool does not cover. | No |
| `content_guard.model` | Model (SystemOne pool) | System One model used for GDPR and NDA decisions. | No |
| `content_guard.mode` | monitor, enforce | Monitor logs decisions; enforce applies the selected action. | No |
| `content_guard.gdpr_action` | allow, confirm, deny | Independent action when the System One model classifies request content as personal data and the destination pool is not declared GDPR-covered. | No |
| `content_guard.nda_action` | allow, confirm, deny | Independent action when the System One model classifies request content as confidential or NDA-protected and the destination pool is not declared NDA-covered. | No |

The checks run only for compliance areas not declared covered by the destination chat pool. When both areas are uncovered, both questions are sent to the selected System One model. The model also receives the request messages, so assess its own pool declarations before selecting it. See [content guard setup and behavior](settings.md#access-and-content-guard).

### chat.ocr

Category: **Chat**.

| Key | Input / choices | Meaning | Restart |
|---|---|---|---|
| `chat.ocr.enabled` | Boolean | Master switch for reading text out of uploaded documents. | No |
| `chat.ocr.model` | Model (Ocr pool) | Which model reads the pages. It must be served by a pool of kind ocr; leave it automatic to use the first one available. | No |
| `chat.ocr.max_tokens` | Integer | Token budget for one OCR request. | No |
| `chat.ocr.ngram_window` | Integer | Overlap used to stitch page texts together without repeating content. | No |
| `chat.ocr.max_bytes` | Integer | Largest document accepted, in bytes. | No |
| `chat.ocr.max_pages` | Integer | Most pages read from a single document. | No |
| `chat.ocr.dpi` | Integer | Resolution PDF pages are rendered at before reading, in DPI. | No |
| `chat.ocr.max_output_chars` | Integer | Cap on the text extracted from one document, in characters. | No |
| `chat.ocr.timeout_secs` | Integer | Deadline for one document, in seconds. | No |
| `chat.ocr.max_concurrency` | Integer | How many pages are read at once. | No |
| `chat.ocr.auto_min_text_chars_per_page` | Integer | Below this many embedded characters per page, a PDF is treated as scanned and sent to OCR. | No |

### chat.compaction

Category: **Chat**.

| Key | Input / choices | Meaning | Restart |
|---|---|---|---|
| `chat.compaction.enabled` | Boolean | Master switch for summarising long conversations. | No |
| `chat.compaction.default_context_window` | Integer | Context window in tokens assumed for a model that does not report one. | No |
| `chat.compaction.trigger_ratio` | Decimal | Fraction of the context window that triggers compaction (0.7 = at 70% full). | No |
| `chat.compaction.keep_recent_turns` | Integer | Turns kept verbatim at the end of the conversation. | No |
| `chat.compaction.min_turns_to_compact` | Integer | Never compact a conversation shorter than this many turns. | No |
| `chat.compaction.summary_max_tokens` | Integer | Token budget for the summary that replaces the compacted turns. | No |

### chat.turns

Category: **Chat**.

| Key | Input / choices | Meaning | Restart |
|---|---|---|---|
| `chat.turns.max_parallel` | Integer | Turns one user may have running in parallel in different conversations. With 1, a second chat waits. | No |

### chat.loops

Category: **Chat**.

| Key | Input / choices | Meaning | Restart |
|---|---|---|---|
| `chat.loops.retries` | Integer | How many times a chat answer is tried again, one thinking level lower each time, after the model collapsed into repeating itself. Default 2, at most 5 (higher values count as 5); 0 stops at the first loop. | No |

### chat.s3

Category: **Data**.

| Key | Input / choices | Meaning | Restart |
|---|---|---|---|
| `chat.s3.enabled` | Boolean | Off means chat attachments are unavailable. | No |
| `chat.s3.endpoint` | Text | For example https://s3.eu-central-1.amazonaws.com, or a MinIO address. | No |
| `chat.s3.region` | Text | Region name. | No |
| `chat.s3.bucket` | Text | Bucket holding the attachments. | No |
| `chat.s3.key_prefix` | Text | Prefix every object key is written under. | No |
| `chat.s3.access_key` | Write-only secret | Identifier of the access key used to reach the bucket. | No |
| `chat.s3.secret_key` | Write-only secret | Secret half of that access key. Stored encrypted. | No |

### sandbox

Category: **Tools**.

| Key | Input / choices | Meaning | Restart |
|---|---|---|---|
| `sandbox.enabled` | Boolean | Register the tools that let the model run code. | No |
| `sandbox.runner_url` | Text | Base URL of the sandbox-runner service. It executes arbitrary code, so it must be reachable only from the gateway. | No |
| `sandbox.timeout_secs` | Integer | HTTP deadline for one run, in seconds. | No |
| `sandbox.max_artifact_bytes` | Integer | Largest single file accepted back from a run, in bytes. | No |

### comfyui

Category: **Tools**.

| Key | Input / choices | Meaning | Restart |
|---|---|---|---|
| `comfyui.enabled` | Boolean | Register the comfyui_* tools. | No |
| `comfyui.base_url` | Text | Base URL of the ComfyUI instance. It has no authentication, so it must be reachable only from the gateway. | Yes |
| `comfyui.content_dir` | Path | Holds one subdirectory per workflow. Use the reload button on /admin/comfyui to re-scan it without a restart. | Yes |
| `comfyui.timeout_secs` | Integer | Deadline for one workflow run, in seconds. | No |
| `comfyui.queue_poll_interval_ms` | Integer | How often the gateway asks ComfyUI about a running job, in milliseconds. | No |
| `comfyui.max_concurrent_jobs` | Integer | Workflows the model may have running at once. | No |

### rag

Category: **Data**.

| Key | Input / choices | Meaning | Restart |
|---|---|---|---|
| `rag.enabled` | Boolean | Master switch for RAG indexing and retrieval. | Yes |
| `rag.data_dir` | Path | Where indexes are stored. Must be on the persistent volume, or every restart reindexes. Existing indexes do not move with it — point this somewhere new and everything is reindexed from scratch. | Yes |
| `rag.clone_concurrency` | Integer | How many git clones and indexing jobs run at once. | Yes |

### skills

Category: **Data**.

| Key | Input / choices | Meaning | Restart |
|---|---|---|---|
| `skills.enabled` | Boolean | Master switch for the skills managed at /admin/skills. | No |
| `skills.dir` | Path | Directory holding the skill bundles. | No |

### typst

Category: **Tools**.

| Key | Input / choices | Meaning | Restart |
|---|---|---|---|
| `typst.enabled` | Boolean | Master switch for PDF export and the document tools. | No |
| `typst.templates_dir` | Path | Directory holding the templates. Re-scanned on save, so adding one needs no restart. | No |

### geoip

Category: **Tools**.

| Key | Input / choices | Meaning | Restart |
|---|---|---|---|
| `geoip.enabled` | Boolean | Master switch for the get_user_location tool. | No |
| `geoip.db_path` | Path | Path to the IP2Location BIN database. | No |
| `geoip.update_token` | Write-only secret | IP2Location token used to refresh the database. Stored encrypted. | No |

### usage

Category: **Access**.

| Key | Input / choices | Meaning | Restart |
|---|---|---|---|
| `usage.enabled` | Boolean | Per-request accounting behind /usage. | No |
| `usage.retention_days` | Integer | How many days records are kept. | No |
| `usage.currency` | Text | Currency that costs are reported in. | No |

### limits

Category: **Access**.

| Key | Input / choices | Meaning | Restart |
|---|---|---|---|
| `limits.enabled` | Boolean | Off means the rules at /admin/limits are ignored. | No |

### feedback

Category: **Notifications**.

| Key | Input / choices | Meaning | Restart |
|---|---|---|---|
| `feedback.enabled` | Boolean | Master switch for the in-app feedback button. | No |
| `feedback.provider` | github, gitlab | Which tracker receives the reports. The fields below are grouped by tracker; only the selected one is used. | No |
| `feedback.github_owner` | Text | GitHub user or organisation that owns the issue tracker. | No |
| `feedback.github_repo` | Text | Repository name issues are filed in. | No |
| `feedback.github_token` | Write-only secret | Needs issues:write, plus contents:write if screenshots are attached. Stored encrypted. | No |
| `feedback.github_api_base` | Text | REST API base URL. Change it for GitHub Enterprise. | No |
| `feedback.assets_branch` | Text | Orphan branch that screenshots are committed to. | No |
| `feedback.gitlab_url` | Text | Instance base URL, e.g. https://gitlab.com — without the /api/v4 path. | No |
| `feedback.gitlab_project_id` | Text | Numeric project ID, or the full path such as group/subgroup/project. | No |
| `feedback.gitlab_token` | Write-only secret | Access token with the api scope (creates issues and uploads screenshots). Stored encrypted. | No |
| `feedback.labels` | List | Labels applied to every issue filed. | No |
| `feedback.extraction_model` | Model (Chat pool) | Chat model that turns a voice note into the form fields. | No |
| `feedback.voice_model` | Model (Transcription pool) | Model that turns the voice note into text. | No |

### push

Category: **Notifications**.

| Key | Input / choices | Meaning | Restart |
|---|---|---|---|
| `push.enabled` | Boolean | Serve the push endpoints and notify when a turn finishes. | No |
| `push.contact` | Text | A mailto: or https: URI the push service can use to reach you. | No |

### gateway

Category: **Access**.

| Key | Input / choices | Meaning | Restart |
|---|---|---|---|
| `gateway.token_ttl_days` | Integer | How many days a freshly minted gwk_… token stays valid. | No |
| `gateway.session_ttl_days` | Integer | Sliding idle timeout for a browser login, in days: every request pushes it forward, so it is how long someone may stay away before signing in again. | No |
| `gateway.session_absolute_max_days` | Integer | Hard cap in days on a browser login since sign-in, which no amount of activity extends. It also forces a periodic trip through the identity provider, the only point at which group claims are re-read. | No |
| `gateway.allow_impersonation` | Boolean | Let admins act as another user for debugging. Every impersonation is audited and shows a persistent banner; off hides the buttons and the endpoint refuses. | No |

### metrics

Category: **Access**. The endpoint answers only when `metrics.enabled` is on and `metrics.token`, `metrics.allowed_ips` or both are set; with both set, a scrape must pass both. See [monitoring](../operations/monitoring.md).

| Key | Input / choices | Meaning | Restart |
|---|---|---|---|
| `metrics.enabled` | Boolean | Serve `GET /metrics`. Off, or on without a token and without allowed IPs, it answers like a path that does not exist (`404`). | No |
| `metrics.token` | Write-only secret | Bearer token a scraper sends as `Authorization: Bearer <token>`; a missing or wrong token answers `401`. Stored encrypted. | No |
| `metrics.allowed_ips` | List of IP addresses and CIDR networks | Client addresses allowed to scrape; any other answers `403`. Each entry must be an address or a CIDR network, or Save is refused naming the entry and the reason. Behind a reverse proxy this works only when `AIPLANE_TRUSTED_PROXIES` names the proxy. | No |

## Web search provider fields

Web search uses a separate settings card and API. It is not a section of the registry above.

| Submitted field | Meaning |
|---|---|
| `provider` | Selected search provider |
| `searxng_url` | SearXNG service URL |
| `brave_api_key` | Write-only Brave key; blank preserves the stored key |
| `clear_brave_key` | Explicitly remove the Brave key |
| `tavily_api_key` | Write-only Tavily key; blank preserves the stored key |
| `clear_tavily_key` | Explicitly remove the Tavily key |
| `tavily_enabled` | Enable the optional Tavily service |

## Interpretation and limits

The registry describes input types and closed choice sets. The settings endpoint accepting a number does not establish that every number is useful for its consumer. Read the field help, check prerequisites and verify the resulting operation. Secrets are sealed at rest and never returned as plaintext by the settings list endpoint.
