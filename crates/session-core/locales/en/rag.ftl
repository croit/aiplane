# Strings owned by `gateway/src/rama_server/pages/rag.rs` — the
# admin-only `/rag` page: create/list/edit/delete RAG collections, their
# per-source refs, the live status poll, and the indexing log viewer.

rag-heading = RAG collections

# Toasts — collection CRUD
rag-toast-vanished = Collection vanished after save.

# Toasts — refs / sources
rag-toast-bulk-queued-skipped = Queued { $added } source(s); skipped { $skipped } duplicate(s).
rag-toast-bulk-queued = Queued indexing of { $added } source(s).
rag-toast-reindex-queued-ref = Queued re-index of `{ $ref }`.

# Status badges
rag-status-pending = pending
rag-status-cloning = cloning
rag-status-indexing = indexing
rag-status-ready = ready
rag-status-error = error

# Collection row
rag-button-edit = Edit
rag-button-add-source = Add source
rag-button-add-bulk = Add sources (bulk)

# Ref / source row
rag-badge-primary = primary
rag-button-reindex = Re-index
rag-button-set-primary = Set primary
rag-button-remove = Remove

# Inline per-source editor
rag-label-branch-tag = Branch / tag
rag-button-cancel = Cancel

# Create-collection form
rag-label-name = Name
rag-label-chunk-size = Chunk size
rag-label-chunk-overlap = Chunk overlap

# Edit-collection form
rag-label-description = Description

# Embedding model field
rag-label-embedding-model = Embedding model

# Source kind picker + provider credential fields (rag_source.rs). The
# per-provider field labels come from the provider itself and are not
# translated: they are written where the provider is defined.
rag-label-source-kind = Source
rag-source-unknown-kind = Unknown source kind.
rag-source-test-button = Test connection
rag-source-test-ok = Connected as `{ $account }`. { $entries } item(s) under the configured folder.
rag-source-test-ok-plain = Connected. { $entries } item(s) under the configured folder.
rag-source-test-failed = Could not reach the source: { $error }
rag-source-detected = Detected: { $server }

rag-label-profile = Document fields
rag-option-profile-none = None — index text only

# Sync hook — an inbound trigger that re-syncs one collection.
rag-button-sync-token = Sync URL
rag-badge-sync-hook = sync hook

# Browser consent for an OAuth source (Google Drive).
rag-source-consent-save-first = Save the collection with its client ID and secret first, then connect it to grant access.
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

# SPA collection manager: the create form, the sync-URL card, the per-source
# rows, and the confirmations the legacy page did not need.
rag-button-new-collection = New collection
rag-label-git-url = Git URL
rag-source-testing = Testing…
rag-button-create = Create
rag-button-rebuild = Rebuild
rag-sync-url-heading = Sync URL — shown once
rag-sync-token-confirm = Mint a new sync URL? The old one stops working.
rag-delete-collection-confirm = Delete collection { $name } and its index?
rag-remove-source-confirm = Remove source { $source }?
rag-toast-rebuild-queued = Full rebuild requested.
rag-ref-indexed-at = indexed { $date }
rag-no-sources = No sources — this collection indexes nothing until one is added.
rag-add-sources-hint = One source per line; add { $at } to override this collection's { $ref }.
