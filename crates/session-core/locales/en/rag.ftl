# Strings owned by `gateway/src/rama_server/pages/rag.rs` — the
# admin-only `/rag` page: create/list/edit/delete RAG collections, their
# per-source refs, the live status poll, and the indexing log viewer.

rag-heading = RAG collections
rag-description-prefix = Codebases the gateway has indexed. The
rag-description-suffix = tool reaches into these collections to answer questions about the code.
rag-collections-heading = Configured collections
rag-empty-list = No collections yet. Create one above.

# Toasts — collection CRUD
rag-toast-indexing-queued = Indexing `{ $name }` @ `{ $ref }` was queued.
rag-toast-created-aggregate = Created `{ $name }` (aggregate). Add source repos below to index them.
rag-toast-collection-saved = Saved `{ $name }`.
rag-toast-vanished = Collection vanished after save.
# Toasts — refs / sources
rag-toast-bulk-queued-skipped = Queued { $added } source(s); skipped { $skipped } duplicate(s).
rag-toast-bulk-queued = Queued indexing of { $added } source(s).
rag-toast-source-updated = Source updated.

# Status badges
rag-status-pending = pending
rag-status-cloning = cloning
rag-status-indexing = indexing
rag-status-ready = ready
rag-status-error = error

# Collection row
rag-pat-set = PAT set
rag-pat-none = no PAT
rag-meta-aggregate = { $count } source(s) · { $hint }
rag-meta-versioned = { $url } · { $hint }
rag-badge-aggregate = aggregate
rag-embed-prefix = embed:
rag-button-edit = Edit
rag-button-delete-collection = Delete collection
rag-placeholder-source-git-url = https://github.com/org/repo.git
rag-button-add-source = Add source
rag-placeholder-branch-tag-commit = branch, tag, or commit
rag-button-add-ref = Add ref
rag-placeholder-bulk-sources = Bulk add — one repo per line, optional @ref:
    https://github.com/proxmox/pve-manager.git
    https://github.com/proxmox/qemu-server.git @master
rag-button-add-bulk = Add sources (bulk)

# Ref / source row
rag-badge-primary = primary
rag-ref-indexed-line = indexed { $date } · { $commit }
rag-never = never
rag-button-log = Log
rag-button-reindex = Re-index
rag-button-set-primary = Set primary
rag-button-remove = Remove

# Indexing log
rag-log-info = info
rag-log-warn = warn
rag-log-error = error
rag-log-heading = Indexing log
rag-log-empty = No indexing events recorded yet. The first run logs here once the indexer picks this ref up.

# Inline per-source editor
rag-label-git-url-source = Git URL (this source)
rag-label-git-url-inherit = Git URL (blank = inherit collection)
rag-placeholder-git-url = https://example.com/org/repo.git
rag-label-branch-tag = Branch / tag
rag-button-save-source = Save source
rag-button-cancel = Cancel

# Create-collection form
rag-create-heading = Index a new collection
rag-create-description = The indexer clones the repo, chunks each file, and embeds it through the configured embedding model. PATs are stored verbatim (the gateway runs on trusted infra).
rag-new-page-title = Index a new collection
rag-edit-page-title = Edit collection
rag-not-found = No collection with that id. It may have been deleted.
rag-back-to-collections = RAG collections
rag-edit-source-heading = Edit source
rag-add-source-heading = Add a source
rag-label-name = Name
rag-placeholder-name = e.g. gateway-repo
rag-label-description-optional = Description (optional)
rag-placeholder-description = short, human-readable
rag-label-git-url-versioned = Git URL (versioned only)
rag-label-pat-optional = Personal access token (optional)
rag-placeholder-pat = for private repos
rag-label-include-globs-full = Include globs (comma- or newline-separated)
rag-placeholder-include-globs = *.rs, *.md
rag-label-exclude-globs = Exclude globs
rag-placeholder-exclude-globs = target/, node_modules/
rag-label-chunk-size = Chunk size
rag-label-chunk-overlap = Chunk overlap
rag-label-allowed-groups = Allowed groups
rag-hint-allowed-groups = Comma-separated gateway groups allowed to list + search this collection. Blank = everyone with the RAG tools. Admins always have access.
rag-create-aggregate-help = Aggregate (multi-source): search across many repos as one corpus. Leave the Git URL empty and add each source repo after creating. Branch / tag becomes the default ref for added sources.
rag-button-queue-indexing = Queue indexing

# Edit-collection form
rag-edit-heading = Editing { $name }
rag-label-description = Description
rag-label-pat = Personal access token
rag-placeholder-pat-keep = leave blank to keep existing
rag-label-clear-pat = Remove the stored PAT (no longer authenticate)
rag-label-include-globs = Include globs
rag-button-save-changes = Save changes

# Embedding model field
rag-label-embedding-model = Embedding model
rag-placeholder-embedding-model-none = no embedding pools configured — type a model id
rag-option-choose-embedding-model = Choose an embedding model…
rag-suffix-not-advertised = (no longer advertised)

# Source kind picker + provider credential fields (rag_source.rs). The
# per-provider field labels come from the provider itself and are not
# translated: they are written where the provider is defined.
rag-label-source-kind = Source
rag-source-git-help = Clones a repository and indexes its files. The original behaviour.
rag-source-secret-placeholder = leave empty to keep the stored value
rag-source-unknown-kind = Unknown source kind.
rag-source-test-button = Test connection
rag-source-test-ok = Connected as `{ $account }`. { $entries } item(s) under the configured folder.
rag-source-test-ok-plain = Connected. { $entries } item(s) under the configured folder.
rag-source-test-failed = Could not reach the source: { $error }
rag-source-test-git = Pick a remote source to test. Git repositories are checked when indexing runs.
rag-source-detected = Detected: { $server }

rag-label-profile = Document fields
rag-option-profile-none = None — index text only
rag-profile-help = Extracts fields (vendor, date, amount, project) from each document so they can be filtered, sorted and totalled. Costs one model call per document; leave as None for code or plain-text collections.

# Extraction-profile editor (/rag/profiles, rag_profiles.rs)
rag-profile-heading = Extraction profiles
rag-profile-description = What gets pulled out of every document in a collection: the fields that make "the most recent invoice from X" or "how much did we spend" answerable. Attach a profile to a collection on the RAG page.
rag-profile-create-heading = New profile
rag-profile-list-heading = Profiles
rag-profile-empty = No profiles yet.
rag-profile-builtin = built-in
rag-profile-version = v{ $version }
rag-profile-summary = { $count } field(s)
rag-profile-label-name = Name
rag-profile-label-description = Description
rag-profile-label-prompt = Extraction instructions
rag-profile-label-fields = Fields (JSON)
rag-profile-prompt-placeholder = Describe what the model is reading and how to normalise dates and amounts.
rag-profile-fields-help = One object per field: key, label, type (text | number | date | enum), description, and optional filterable / sortable. An enum also needs "values". The description is shown to the model, so be precise.
rag-profile-edit-warning = Saving bumps this profile's version, which clears its cached extractions. Collections using it must be re-indexed to pick up the new fields.
rag-profile-button-create = Create profile
rag-profile-button-save = Save
rag-profile-button-delete = Delete
rag-profile-delete-confirm = Delete profile { $name }?
rag-profile-example-counterparty-label = Counterparty
rag-profile-example-counterparty-description = The other party.
rag-profile-example-date-label = Date
rag-profile-example-date-description = The document date.
rag-profile-example-amount-label = Amount
rag-profile-example-amount-description = The total amount.
rag-profile-link = Edit extraction profiles
rag-profile-toast-created = Created profile `{ $name }`.
rag-profile-toast-saved = Saved `{ $name }`.
rag-profile-toast-saved-reindex = Saved `{ $name }`. Re-index to apply it: { $collections }.
rag-profile-toast-deleted = Profile deleted.
# Sync hook — an inbound trigger that re-syncs one collection.
rag-toast-sync-token = Sync URL (shown once, it is not stored): { $url }
rag-toast-sync-token-cleared = Sync URL disabled.
rag-button-sync-token = Sync URL
rag-button-sync-token-rotate = New sync URL
rag-button-sync-token-clear = Disable sync URL
rag-badge-sync-hook = sync hook

# Browser consent for an OAuth source (Google Drive).
rag-source-consent-save-first = Save the collection with its client ID and secret first, then connect it to grant access.
rag-source-consent-connected = connected
rag-source-consent-connect = Connect
rag-oauth-lookup-failed = Could not read the collection.
rag-oauth-not-oauth = This source kind is not connected in the browser.
rag-oauth-no-client = Save the OAuth client ID and secret on the collection first.
rag-oauth-bad-authorize-url = Could not build the provider's authorization URL.
rag-oauth-start-failed = Could not start the authorization.
rag-oauth-callback-missing = The provider's answer was missing its code or state.
rag-oauth-expired = That authorization has expired or was already used. Start again.
rag-oauth-provider-refused = The provider refused the authorization: { $error }
rag-oauth-exchange-failed = Exchanging the authorization code failed: { $error }
rag-oauth-no-refresh-token = The provider returned no refresh token, so the gateway could not keep indexing unattended. Remove the gateway's access in your provider account and connect again.
rag-oauth-store-failed = Could not store the credentials.
rag-badge-no-files = no files indexed
rag-ref-files = { $files } files
rag-label-git-url = Git URL
rag-source-testing = Testing…
rag-sync-url-heading = Sync URL — shown once
rag-sync-token-confirm = Mint a new sync URL? The old one stops working.
rag-delete-collection-confirm = Delete collection { $name } and its index?
rag-remove-source-confirm = Remove source { $source }?
rag-toast-rebuild-queued = Full rebuild requested.
rag-no-sources = No sources — this collection indexes nothing until one is added.
rag-add-sources-hint = One source per line; add { $at } to override this collection's { $ref }.
