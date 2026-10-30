# Operator settings (/admin/settings), stored in the database.
#
# Card titles (settings-s-*), field labels (settings-f-*) and their one-line
# help (settings-f-*-help) are keyed off the spec entries in
# aiplane_core::server::settings::SECTIONS, which derives each key from the
# dotted setting path: `sandbox.runner_url` -> `settings-f-sandbox-runner_url`. Dots
# become dashes because Fluent identifiers reject `.`; underscores stay.
#
# The Rust table holds no prose at all, so these files are the only copy.
# The editor prints the setting path itself next to the help text, untranslated —
# a localised label says what a field does, the identifier says what to grep
# for in the docs and the logs.

settings-save = Save section
settings-saved = Saved. In effect from the next request.
settings-reset-default = Reset to default
settings-restart-note = Fields marked "restart" are read once when the gateway starts; changing them needs a restart to take effect.
settings-secret-set = stored — type a new value to replace it
settings-secret-unset = not set
settings-secret-clear = Clear

# Category tabs.
settings-tab-chat = Chat
settings-tab-tools = Tools
settings-tab-data = Content & data
settings-tab-access = Access & usage
settings-tab-notifications = Notifications
settings-tab-web-search = Web search
settings-restart-pending-body = These settings are saved but only take effect after the gateway restarts:

# ─── Section cards ───────────────────────────────────────────────────────────

settings-s-chat-prompt = Chat prompt
settings-s-chat-prompt-blurb = Standing instructions the model follows in every chat a person has with the assistant.
settings-s-chat-ocr = Document OCR
settings-s-chat-ocr-blurb = Turning uploaded PDFs and images into text the model can read.
settings-s-chat-compaction = Conversation compaction
settings-s-chat-compaction-blurb = Summarising the older half of a long conversation so it keeps fitting in the model's context window.
settings-s-chat-voice = Voice
settings-s-chat-voice-blurb = The standing instructions for a chat that speaks: how the assistant should phrase what it says out loud.
settings-s-chat-turns = Parallel conversations
settings-s-chat-turns-blurb = How many of a user's chats may be answering at the same time.
settings-s-chat-loops = Loop retries
settings-s-chat-loops-blurb = What a chat does when the model starts repeating itself.
settings-s-chat-s3 = Attachment storage (S3)
settings-s-chat-s3-blurb = Object storage for chat attachments. Without it, uploads are refused.
settings-s-sandbox = Code sandbox
settings-s-sandbox-blurb = The isolated runner that executes model-written code.
settings-s-comfyui = ComfyUI image & video
settings-s-comfyui-blurb = The headless ComfyUI worker behind the image and video tools.
settings-s-rag = RAG indexing
settings-s-rag-blurb = Where indexed sources are stored, and how hard the indexer works.
settings-s-skills = Skills
settings-s-skills-blurb = The on-disk bundle directory behind /admin/skills.
settings-s-typst = Typst templates
settings-s-typst-blurb = Templates behind PDF export and the document tools.
settings-s-geoip = GeoIP
settings-s-geoip-blurb = Coarse client location, for the get_user_location tool.
settings-s-usage = Usage metrics
settings-s-usage-blurb = Per-request accounting behind /usage.
settings-s-limits = Rate limits & quotas
settings-s-limits-blurb = Master switch for the rules configured at /admin/limits.
settings-s-agents = Personal agents
settings-s-agents-blurb = Capacity for personal agents.
settings-s-feedback = Feedback widget
settings-s-feedback-blurb = Where the in-app feedback widget files issues.
settings-s-push = Web Push
settings-s-push-blurb = Turn-complete notifications. The keypair is generated and stored automatically.
settings-s-gateway = Sessions & tokens
settings-s-gateway-blurb = How long a browser login and an API token stay valid, and whether admins may impersonate.
settings-s-metrics = Prometheus metrics
settings-s-metrics-blurb = The GET /metrics scrape endpoint. It answers only with a token or an allowed IP list set; with both, a scrape must pass both.

# ─── Fields ──────────────────────────────────────────────────────────────────

content-guard-confirm-heading = Content protection
content-guard-confirm-question = This content was identified as protected. Do you want to send it to the selected model anyway?
content-guard-confirm-approve = Send anyway
content-guard-confirm-deny = Do not send

settings-f-chat-prompt-enabled = Enable the chat prompt
settings-f-chat-prompt-enabled-help = Master switch for the deployment-wide system prompt of a person's chat.
settings-f-chat-prompt-text = System prompt
settings-f-chat-prompt-text-help = Added to the system message of every person's chat while enabled. Agents keep their own instructions.
settings-f-chat-ocr-enabled = Enable OCR
settings-f-chat-ocr-enabled-help = Master switch for reading text out of uploaded documents.
settings-f-chat-ocr-model = OCR model
settings-f-chat-ocr-model-help = Which model reads the pages. It must be served by a pool of kind ocr; leave it automatic to use the first one available.
settings-f-chat-ocr-max_tokens = Token budget per request
settings-f-chat-ocr-max_tokens-help = Token budget for one OCR request.
settings-f-chat-ocr-ngram_window = Overlap window
settings-f-chat-ocr-ngram_window-help = Overlap used to stitch page texts together without repeating content.
settings-f-chat-ocr-max_bytes = Maximum document size
settings-f-chat-ocr-max_bytes-help = Largest document accepted, in bytes.
settings-f-chat-ocr-max_pages = Maximum pages
settings-f-chat-ocr-max_pages-help = Most pages read from a single document.
settings-f-chat-ocr-dpi = Rasterisation resolution
settings-f-chat-ocr-dpi-help = Resolution PDF pages are rendered at before reading, in DPI.
settings-f-chat-ocr-max_output_chars = Maximum extracted text
settings-f-chat-ocr-max_output_chars-help = Cap on the text extracted from one document, in characters.
settings-f-chat-ocr-timeout_secs = Timeout
settings-f-chat-ocr-timeout_secs-help = Deadline for one document, in seconds.
settings-f-chat-ocr-max_concurrency = Documents in parallel
settings-f-chat-ocr-max_concurrency-help = How many documents are read at once across the whole gateway. Others wait in a queue.
settings-f-chat-ocr-auto_min_text_chars_per_page = Scanned-page threshold
settings-f-chat-ocr-auto_min_text_chars_per_page-help = Below this many embedded characters per page, a PDF is treated as scanned and sent to OCR.

settings-f-chat-compaction-enabled = Enable compaction
settings-f-chat-compaction-enabled-help = Master switch for summarising long conversations.
settings-f-chat-compaction-default_context_window = Assumed context window
settings-f-chat-compaction-default_context_window-help = Context window in tokens assumed for a model that does not report one.
settings-f-chat-compaction-trigger_ratio = Trigger threshold
settings-f-chat-compaction-trigger_ratio-help = Fraction of the context window that triggers compaction (0.7 = at 70% full).
settings-f-chat-compaction-keep_recent_turns = Recent turns kept
settings-f-chat-compaction-keep_recent_turns-help = Turns kept verbatim at the end of the conversation.
settings-f-chat-compaction-min_turns_to_compact = Minimum conversation length
settings-f-chat-compaction-min_turns_to_compact-help = Never compact a conversation shorter than this many turns.
settings-f-chat-compaction-summary_max_tokens = Summary token budget
settings-f-chat-compaction-summary_max_tokens-help = Token budget for the summary that replaces the compacted turns.
settings-f-chat-compaction-summary_prompt = Summariser prompt
settings-f-chat-compaction-summary_prompt-help = The instructions the summariser works from. Leave empty to use the built-in prompt.

settings-f-chat-voice-directive = Voice directive
settings-f-chat-voice-directive-help = Standing instructions for voice turns, added to the system message. Leave empty to use the built-in directive.

settings-f-chat-turns-max_parallel = Conversations at once
settings-f-chat-turns-max_parallel-help = Turns one user may have running in parallel, in different conversations. 1 keeps the old behaviour: a second chat waits.
settings-f-chat-loops-retries = Retries
settings-f-chat-loops-retries-help = When the model collapses into repeating itself, the answer is tried again one thinking level lower, up to this many times (0–5). 0 stops at the first loop.

settings-f-chat-s3-enabled = Store attachments in S3
settings-f-chat-s3-enabled-help = Off means chat attachments are unavailable.
settings-f-chat-s3-endpoint = Endpoint URL
settings-f-chat-s3-endpoint-help = For example https://s3.eu-central-1.amazonaws.com, or a MinIO address.
settings-f-chat-s3-region = Region
settings-f-chat-s3-region-help = Region name.
settings-f-chat-s3-bucket = Bucket
settings-f-chat-s3-bucket-help = Bucket holding the attachments.
settings-f-chat-s3-key_prefix = Key prefix
settings-f-chat-s3-key_prefix-help = Prefix every object key is written under.
settings-f-chat-s3-access_key = Access key ID
settings-f-chat-s3-access_key-help = Identifier of the access key used to reach the bucket.
settings-f-chat-s3-secret_key = Secret access key
settings-f-chat-s3-secret_key-help = Secret half of that access key. Stored encrypted.

settings-f-sandbox-enabled = Enable the sandbox tools
settings-f-sandbox-enabled-help = Register the tools that let the model run code.
settings-f-sandbox-runner_url = Runner URL
settings-f-sandbox-runner_url-help = Base URL of the sandbox-runner service. It executes arbitrary code, so it must be reachable only from the gateway.
settings-f-sandbox-timeout_secs = Timeout
settings-f-sandbox-timeout_secs-help = HTTP deadline for one run, in seconds.
settings-f-sandbox-max_artifact_bytes = Maximum artifact size
settings-f-sandbox-max_artifact_bytes-help = Largest single file accepted back from a run, in bytes.

settings-f-comfyui-enabled = Enable the image & video tools
settings-f-comfyui-enabled-help = Register the comfyui_* tools.
settings-f-comfyui-base_url = ComfyUI URL
settings-f-comfyui-base_url-help = Base URL of the ComfyUI instance. It has no authentication, so it must be reachable only from the gateway.
settings-f-comfyui-content_dir = Workflow directory
settings-f-comfyui-content_dir-help = Holds one subdirectory per workflow. Use the reload button on /admin/comfyui to re-scan it without a restart.
settings-f-comfyui-timeout_secs = Timeout
settings-f-comfyui-timeout_secs-help = Deadline for one workflow run, in seconds.
settings-f-comfyui-queue_poll_interval_ms = Queue poll interval
settings-f-comfyui-queue_poll_interval_ms-help = How often the gateway asks ComfyUI about a running job, in milliseconds.
settings-f-comfyui-max_concurrent_jobs = Concurrent jobs
settings-f-comfyui-max_concurrent_jobs-help = Workflows the model may have running at once.

settings-f-rag-enabled = Run the indexer
settings-f-rag-enabled-help = Master switch for RAG indexing and retrieval.
settings-f-rag-data_dir = Index directory
settings-f-rag-data_dir-help = Where indexes are stored. Must be on the persistent volume, or every restart reindexes. Existing indexes do not move with it — point this somewhere new and everything is reindexed from scratch.
settings-f-rag-clone_concurrency = Parallel index jobs
settings-f-rag-clone_concurrency-help = How many git clones and indexing jobs run at once.

settings-f-skills-enabled = Load skill bundles
settings-f-skills-enabled-help = Master switch for the skills managed at /admin/skills.
settings-f-skills-dir = Bundle directory
settings-f-skills-dir-help = Directory holding the skill bundles.

settings-f-typst-enabled = Load Typst templates
settings-f-typst-enabled-help = Master switch for PDF export and the document tools.
settings-f-typst-templates_dir = Template directory
settings-f-typst-templates_dir-help = Directory holding the templates. Re-scanned on save, so adding one needs no restart.

settings-f-geoip-enabled = Enable GeoIP lookups
settings-f-geoip-enabled-help = Master switch for the get_user_location tool.
settings-f-geoip-db_path = Database file
settings-f-geoip-db_path-help = Path to the IP2Location BIN database.
settings-f-geoip-update_token = Download token
settings-f-geoip-update_token-help = IP2Location token used to refresh the database. Stored encrypted.

settings-f-usage-enabled = Record usage
settings-f-usage-enabled-help = Per-request accounting behind /usage.
settings-f-usage-retention_days = Retention
settings-f-usage-retention_days-help = How many days records are kept.
settings-f-usage-currency = Currency
settings-f-usage-currency-help = Currency that costs are reported in.

settings-f-limits-enabled = Enforce limits and quotas
settings-f-limits-enabled-help = Off means the rules at /admin/limits are ignored.
settings-f-agents-max_active_per_user = Active personal agents per user
settings-f-agents-max_active_per_user-help = Maximum number of active personal agents a user may own. Default: 4.

settings-f-feedback-enabled = Offer the feedback widget
settings-f-feedback-enabled-help = Master switch for the in-app feedback button.
settings-f-feedback-provider = Issue tracker
settings-f-feedback-provider-help = Which tracker receives the reports. The fields below are grouped by tracker; only the selected one is used.
settings-f-feedback-provider-opt-github = GitHub
settings-f-feedback-provider-opt-gitlab = GitLab
settings-f-feedback-github_owner = Repository owner
settings-f-feedback-github_owner-help = GitHub user or organisation that owns the issue tracker.
settings-f-feedback-github_repo = Repository
settings-f-feedback-github_repo-help = Repository name issues are filed in.
settings-f-feedback-github_token = GitHub token
settings-f-feedback-github_token-help = Needs issues:write, plus contents:write if screenshots are attached. Stored encrypted.
settings-f-feedback-github_api_base = API base URL
settings-f-feedback-github_api_base-help = REST API base URL. Change it for GitHub Enterprise.
settings-f-feedback-labels = Issue labels
settings-f-feedback-labels-help = Labels applied to every issue filed.
settings-f-feedback-assets_branch = Screenshot branch
settings-f-feedback-assets_branch-help = Orphan branch that screenshots are committed to.
settings-f-feedback-gitlab_url = GitLab URL
settings-f-feedback-gitlab_url-help = Instance base URL, e.g. https://gitlab.com — without the /api/v4 path.
settings-f-feedback-gitlab_project_id = GitLab project
settings-f-feedback-gitlab_project_id-help = Numeric project ID, or the full path such as group/subgroup/project.
settings-f-feedback-gitlab_token = GitLab token
settings-f-feedback-gitlab_token-help = Access token with the api scope (creates issues and uploads screenshots). Stored encrypted.
settings-f-feedback-extraction_model = Extraction model
settings-f-feedback-extraction_model-help = Chat model that turns a voice note into the form fields.
settings-f-feedback-voice_model = Transcription model
settings-f-feedback-voice_model-help = Model that turns the voice note into text.

settings-f-push-enabled = Send push notifications
settings-f-push-enabled-help = Serve the push endpoints and notify when a turn finishes.
settings-f-push-contact = Operator contact
settings-f-push-contact-help = A mailto: or https: URI the push service can use to reach you.

settings-f-gateway-token_ttl_days = API token lifetime
settings-f-gateway-token_ttl_days-help = How many days a freshly minted gwk_… token stays valid.
settings-f-gateway-session_ttl_days = Session idle timeout
settings-f-gateway-session_ttl_days-help = Sliding idle timeout for a browser login, in days: every request pushes it forward, so it is how long someone may stay away before signing in again.
settings-f-gateway-session_absolute_max_days = Maximum session age
settings-f-gateway-session_absolute_max_days-help = Hard cap in days on a browser login since sign-in, which no amount of activity extends. It also forces a periodic trip through the identity provider, the only point at which group claims are re-read.
settings-f-gateway-allow_impersonation = Allow impersonation
settings-f-gateway-allow_impersonation-help = Let admins act as another user for debugging. Every impersonation is audited and shows a persistent banner; off hides the buttons and the endpoint refuses.

settings-f-metrics-enabled = Serve /metrics
settings-f-metrics-enabled-help = Off answers 404. On without a token and without allowed IPs also answers 404 — the endpoint is never open unguarded.
settings-f-metrics-token = Scrape token
settings-f-metrics-token-help = Prometheus sends it as Authorization: Bearer <token>. Stored encrypted.
settings-f-metrics-allowed_ips = Allowed IPs
settings-f-metrics-allowed_ips-help = Addresses or CIDR networks, e.g. 10.0.0.0/8, 2001:db8::/32. Behind a reverse proxy this only works when AIPLANE_TRUSTED_PROXIES names the proxy; otherwise every scrape comes from the proxy address.

# Confirmation before clearing one stored setting (SPA settings editor).
settings-clear-confirm = Reset { $key } to its built-in default?
settings-heading = Settings
settings-intro = Operator settings for this gateway. They live in the database — each field also shows its setting key.
settings-saved-restart = Saved. Some fields in this section only take effect after a restart.
settings-save-failed = Could not save those settings.
settings-cleared = Cleared. The built-in default applies again.
settings-restart-badge = restart
settings-no-backend-heading = No model backend yet
settings-no-backend-body = Setup configured sign-in, but this gateway serves no models until you add a backend. Chat and the /v1 API will refuse requests until then.
settings-no-backend-cta = Add a backend under Models & routing →
settings-show-fields = Show { $count } more settings
settings-model-automatic = Automatic — use the first available model
settings-model-none-configured = No model of this kind is configured yet. Add a pool under Models & routing and it will appear here.
settings-model-unavailable = { $model } (configured, but not currently available)
settings-restart-pending-heading = Restart pending

# A refused settings save: the summary, and the line under each field the
# server could not use (FieldSpec::check).
settings-invalid = Nothing was saved: correct the marked fields and save again.
settings-invalid-whole-number = Enter a whole number.
settings-invalid-negative = Enter a number of 0 or more.
settings-invalid-number = Enter a number, such as 0.7.
settings-invalid-choice = Choose one of the offered options.
settings-invalid-ip-network = “{ $entry }” is not an IP address or CIDR network, such as 10.0.0.0/8 or 2001:db8::/32: { $reason }.
settings-invalid-ip-network-prefix-not-number = the prefix length is not a number
settings-invalid-ip-network-address = the address does not parse
settings-invalid-ip-network-mapped-prefix = an IPv4-mapped network needs a prefix of 96 or more
settings-invalid-ip-network-prefix-too-long = the prefix length exceeds { $max }

settings-f-gateway-priority_enabled = Enable request prioritization
settings-f-gateway-priority_enabled-help = Strict priority: high requests take the next available slot before medium and low. Within a priority, arrival order applies. Lower priorities may time out under sustained higher-priority load.
settings-f-gateway-default_priority = Default request priority
settings-f-gateway-default_priority-help = Used when neither a token nor a group has an explicit priority.
settings-f-gateway-default_priority-opt-low = Low
settings-f-gateway-default_priority-opt-medium = Medium
settings-f-gateway-default_priority-opt-high = High

settings-f-agents-max_active_per_organization = Active agents per organization
settings-f-agents-max_active_per_organization-help = Maximum number of active agents an organization may own, including idle agents. Default: 4.

# Branding — the operator-uploaded company logo. A hand-rolled admin card,
# not a settings.rs section, so its keys are plain names.
settings-tab-branding = Branding
branding-heading = Branding
branding-intro = The company logo shown in the app header, on the login screen and as the browser tab icon. Upload a light variant and, optionally, a dark one.
branding-slot-light = Light theme
branding-slot-dark = Dark theme
branding-upload = Upload
branding-remove = Remove
branding-format-hint = SVG or PNG, up to 512 KB. Larger images are resized for you.
branding-saved = Logo saved.
branding-removed = Logo removed.
branding-error-format = That file is not a usable logo — upload an SVG or a raster image (PNG, JPEG).
branding-error-too-large = The logo is still over 512 KB after resizing — upload a smaller image.
branding-error-slot = Unknown theme slot — the logo is set for the light or the dark theme.
branding-error-empty = The upload carried no file.
branding-error-multipart = The upload was not a readable form submission — try uploading the file again.
branding-error-store = The logo could not be stored — try again.

settings-s-chat-welcome = Welcome text
settings-s-chat-welcome-blurb = Shown on an empty conversation before the first message.
settings-f-chat-welcome-text = Welcome text
settings-f-chat-welcome-text-help = Centred on a card in an empty chat. Markdown is supported; empty means no welcome text. Placeholders: { "{" }name{ "}" } (display name), { "{" }first_name{ "}" }, { "{" }email{ "}" }, { "{" }language{ "}" } (interface language). Text wrapped in <prompt>…</prompt> becomes a button that sends it.

settings-s-classification = Data classification
settings-s-classification-blurb = The levels that rank how sensitive a conversation is, and how the gateway reacts when content needs a higher one. Levels, data classes and pool assignments are defined in the database.
settings-f-classification-default_level = Default level
settings-f-classification-default_level-help = The level id a new conversation starts at. Empty starts at the lowest level.
settings-f-classification-lowering = Lowering a level
settings-f-classification-lowering-help = Whether a conversation's level may be lowered again after it was raised.
settings-f-classification-lowering-opt-never = Never
settings-f-classification-lowering-opt-with_reason = With a reason, audited
settings-f-classification-gate_on_match = When a gate matches
settings-f-classification-gate_on_match-help = What happens when content matches a class that needs a higher level than the target offers.
settings-f-classification-gate_on_match-opt-ask = Ask the user
settings-f-classification-gate_on_match-opt-block = Block
settings-f-classification-link_share_max_level = Highest level for link sharing
settings-f-classification-link_share_max_level-help = The highest level id a conversation may have and still be shared by link. Empty applies no limit.
settings-f-classification-unknown_label_level = Level of unknown labels
settings-f-classification-unknown_label_level-help = The level id given to content carrying a label that no marking maps.
settings-f-classification-unreadable_level = Level of unreadable files
settings-f-classification-unreadable_level-help = The level id given to files whose classification cannot be read.
settings-f-classification-system_one_model = Detector model
settings-f-classification-system_one_model-help = System One model that answers the data class questions. Empty disables detection.
settings-f-classification-mode = Detection mode
settings-f-classification-mode-help = Monitor logs what the detector finds and changes nothing; enforce raises conversations and applies the class action.
settings-f-classification-mode-opt-monitor = Monitor
settings-f-classification-mode-opt-enforce = Enforce
settings-f-classification-output-visible = Visible marking on output
settings-f-classification-output-visible-help = Stamp the level's marking text onto generated documents.
settings-f-classification-output-metadata = Metadata marking on output
settings-f-classification-output-metadata-help = Write the level into the metadata of generated documents.
settings-f-classification-output-filename = Level in file names
settings-f-classification-output-filename-help = Add the level's file name tag to generated files.
