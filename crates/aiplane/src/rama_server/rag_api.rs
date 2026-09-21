// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! `/api/v0/rag/*` — session-authenticated admin API for the RAG
//! collection registry.
//!
//! Wire shapes are kept inline rather than in `shared::api` because they
//! are admin-only (the CLI doesn't speak them) and likely to evolve as
//! the indexer gains knobs. The PAT field is treated as a one-way
//! secret: it can be *set* on create/update, but every response
//! surfaces `pat_set: bool` instead of the plaintext.

use std::sync::Arc;

use jiff::Timestamp;
use rama::http::service::web::extract::{Path, State};
use rama::http::service::web::response::IntoResponse;
use rama::http::{Request, Response, StatusCode, header};
use serde::{Deserialize, Serialize};
use serde_json::json;

use aiplane_core::rama_server::session::Session;
use aiplane_core::server::db::rag as rag_db;
use aiplane_core::server::db::rag_documents;
use aiplane_core::server::db::users;
use aiplane_runtime::rama_server::state::RamaState;

/// Wire shape returned from every list / get / update response.
#[derive(Serialize)]
struct CollectionView {
    id: i64,
    name: String,
    description: Option<String>,
    git_url: String,
    git_ref: String,
    pat_set: bool,
    /// Which provider reaches this collection's files: `git`, or a
    /// registered remote source. See `GET /api/v0/rag/providers`.
    source_kind: String,
    /// The provider's non-secret settings. Secrets are never returned;
    /// `source_secrets_set` says whether any are stored.
    source_config: std::collections::BTreeMap<String, String>,
    source_secrets_set: bool,
    sync_hook_set: bool,
    connected_account: Option<String>,
    connected_by: Option<String>,
    connected_at: Option<String>,
    /// Extraction profile id, or null. Names are resolved on write; the id
    /// is what the row stores.
    profile_id: Option<i64>,
    extraction_model: Option<String>,
    embedding_model: String,
    include_globs: Vec<String>,
    exclude_globs: Vec<String>,
    chunk_size: i64,
    chunk_overlap: i64,
    search_mode: String,
    /// Minutes between automatic re-syncs; `0` = only on demand.
    refresh_interval_mins: i64,
    allowed_groups: Vec<String>,
    status: String,
    last_indexed_at: Option<String>,
    last_indexed_commit: Option<String>,
    last_error: Option<String>,
    created_at: String,
    updated_at: String,
}

impl CollectionView {
    fn from_collection(c: rag_db::Collection, search_ref: Option<rag_db::CollectionRef>) -> Self {
        let status = search_ref
            .as_ref()
            .map(|r| r.status.as_str().to_string())
            .unwrap_or_else(|| "unconfigured".to_string());
        let last_indexed_at = search_ref
            .as_ref()
            .and_then(|r| r.last_indexed_at)
            .map(|t| t.to_string());
        let last_indexed_commit = search_ref
            .as_ref()
            .and_then(|r| r.last_indexed_commit.clone());
        let last_error = search_ref.and_then(|r| r.last_error);
        CollectionView {
            id: c.id,
            name: c.name,
            description: c.description,
            git_url: c.git_url,
            git_ref: c.git_ref,
            pat_set: c.pat.is_some(),
            source_kind: c.source.kind,
            source_config: c.source.config,
            source_secrets_set: c.source.secrets.is_some(),
            sync_hook_set: c.sync_hook_set,
            connected_account: c.connected_account,
            connected_by: c.connected_by,
            connected_at: c.connected_at,
            profile_id: c.profile_id,
            extraction_model: c.extraction_model,
            embedding_model: c.embedding_model,
            include_globs: c.include_globs,
            exclude_globs: c.exclude_globs,
            chunk_size: c.chunk_size,
            chunk_overlap: c.chunk_overlap,
            search_mode: c.search_mode.as_str().to_string(),
            refresh_interval_mins: c.refresh_interval_mins,
            allowed_groups: c.allowed_groups,
            status,
            last_indexed_at,
            last_indexed_commit,
            last_error,
            created_at: c.created_at.to_string(),
            updated_at: c.updated_at.to_string(),
        }
    }
}

async fn collection_view(
    pool: &aiplane_core::server::db::Pool,
    collection: rag_db::Collection,
) -> Result<CollectionView, aiplane_core::server::db::DbError> {
    let search_ref = rag_db::primary_ref(pool, collection.id).await?;
    Ok(CollectionView::from_collection(collection, search_ref))
}

#[derive(Deserialize)]
struct CreateRequest {
    name: String,
    #[serde(default)]
    description: Option<String>,
    /// Only meaningful for `source_kind: "git"`, and only required there —
    /// a Drive or WebDAV collection has no git URL, and demanding one made
    /// every non-git caller send `"git_url": ""` to get past the parser.
    #[serde(default)]
    git_url: String,
    #[serde(default = "default_ref")]
    git_ref: String,
    #[serde(default)]
    pat: Option<String>,
    /// `git` (the default) or a registered provider kind.
    #[serde(default = "default_source_kind")]
    source_kind: String,
    /// Flat settings map for the chosen provider, secrets included. Which
    /// keys are secret is the provider's call (`config_fields`), so the
    /// caller sends one map and the server splits and seals it.
    #[serde(default)]
    source_config: std::collections::BTreeMap<String, String>,
    /// Extraction profile to apply to each document, by name. Absent = no
    /// extraction, which is right for a code collection.
    #[serde(default)]
    profile: Option<String>,
    #[serde(default)]
    extraction_model: Option<String>,
    embedding_model: String,
    #[serde(default)]
    include_globs: Vec<String>,
    #[serde(default)]
    exclude_globs: Vec<String>,
    #[serde(default = "default_chunk_size")]
    chunk_size: i64,
    #[serde(default = "default_chunk_overlap")]
    chunk_overlap: i64,
    #[serde(default = "default_search_mode")]
    search_mode: String,
    /// Minutes between automatic re-syncs; `0` (the default) leaves the
    /// collection on manual re-index and the sync hook, which is what every
    /// caller written before this field expects.
    #[serde(default)]
    refresh_interval_mins: i64,
}

fn default_ref() -> String {
    "main".into()
}
fn default_source_kind() -> String {
    "git".into()
}
fn default_chunk_size() -> i64 {
    800
}
fn default_chunk_overlap() -> i64 {
    100
}
fn default_search_mode() -> String {
    "versioned".into()
}

#[derive(Deserialize, Default)]
struct UpdateRequest {
    #[serde(default)]
    description: Option<Option<String>>,
    #[serde(default)]
    git_ref: Option<String>,
    /// `Some(Some(token))` → set; `Some(None)` → clear; missing → leave.
    /// Using `Option<Option<String>>` is the canonical "tri-state PATCH"
    /// idiom for fields that are themselves nullable in the model.
    #[serde(default, deserialize_with = "deserialize_option_option")]
    pat: Option<Option<String>>,
    #[serde(default)]
    embedding_model: Option<String>,
    /// Replace the source. Both must be sent together: a settings map means
    /// nothing without the kind that defines its schema.
    #[serde(default)]
    source_kind: Option<String>,
    #[serde(default)]
    source_config: Option<std::collections::BTreeMap<String, String>>,
    #[serde(default)]
    include_globs: Option<Vec<String>>,
    #[serde(default)]
    exclude_globs: Option<Vec<String>>,
    #[serde(default)]
    chunk_size: Option<i64>,
    #[serde(default)]
    chunk_overlap: Option<i64>,
    #[serde(default)]
    git_url: Option<String>,
    #[serde(default, deserialize_with = "deserialize_option_option")]
    profile: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_option_option")]
    extraction_model: Option<Option<String>>,
    #[serde(default)]
    search_mode: Option<String>,
    #[serde(default)]
    refresh_interval_mins: Option<i64>,
    #[serde(default)]
    allowed_groups: Option<Vec<String>>,
}

// Distinguish "field omitted" from "field set to null" for the PAT.
fn deserialize_option_option<'de, D>(de: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    Ok(Some(Option::<String>::deserialize(de)?))
}

pub async fn list_collections(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    if let Err(resp) = require_admin(&state, &req).await {
        return resp;
    }
    let rows = match rag_db::list_collections(&state.db).await {
        Ok(r) => r,
        Err(err) => {
            tracing::warn!(error = %err, "listing rag collections");
            return internal_error("listing collections failed");
        }
    };
    let mut view = Vec::with_capacity(rows.len());
    for collection in rows {
        match collection_view(&state.db, collection).await {
            Ok(collection) => view.push(collection),
            Err(err) => {
                tracing::warn!(error = %err, "deriving rag collection status");
                return internal_error("deriving collection status failed");
            }
        }
    }
    json_ok(&json!({ "data": view }))
}

pub async fn get_collection(
    State(state): State<Arc<RamaState>>,
    Path(id): Path<i64>,
    req: Request,
) -> Response {
    if let Err(resp) = require_admin(&state, &req).await {
        return resp;
    }
    match rag_db::find_collection_by_id(&state.db, id).await {
        Ok(Some(c)) => match collection_view(&state.db, c).await {
            Ok(view) => json_ok(&view),
            Err(err) => {
                tracing::warn!(error = %err, %id, "deriving rag collection status");
                internal_error("deriving collection status failed")
            }
        },
        Ok(None) => not_found(&format!("no collection with id {id}")),
        Err(err) => {
            tracing::warn!(error = %err, %id, "get rag collection");
            internal_error("collection lookup failed")
        }
    }
}

pub async fn create_collection(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    if let Err(resp) = require_admin(&state, &req).await {
        return resp;
    }
    let body = match read_json::<CreateRequest>(req).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    if body.name.trim().is_empty() || body.name.len() > 64 {
        return invalid_request("`name` must be 1..=64 characters");
    }
    let source = match build_source(&state, &body.source_kind, &body.source_config, None) {
        Ok(spec) => spec,
        Err(msg) => return invalid_request(&msg),
    };
    // A profile is named rather than numbered on the wire: ids are an
    // implementation detail of this gateway's DB, names are what an operator
    // writes in a script.
    let profile_id = match body.profile.as_deref().filter(|p| !p.is_empty()) {
        None => None,
        Some(name) => match rag_documents::find_profile_by_name(&state.db, name).await {
            Ok(Some(p)) => Some(p.id),
            Ok(None) => {
                return invalid_request(&format!(
                    "no extraction profile named `{name}` — see GET /api/v0/rag/profiles"
                ));
            }
            Err(err) => {
                tracing::warn!(error = %err, "looking up extraction profile");
                return internal_error("profile lookup failed");
            }
        },
    };
    // A remote source keeps its location in the provider config, so only a
    // git collection needs a repository URL.
    if source.is_git() && body.git_url.trim().is_empty() {
        return invalid_request("`git_url` must not be empty");
    }
    if body.embedding_model.trim().is_empty() {
        return invalid_request("`embedding_model` must not be empty");
    }
    if let Some(msg) = chunk_pair_error(body.chunk_size, body.chunk_overlap) {
        return invalid_request(msg);
    }
    if let Some(msg) = refresh_interval_error(body.refresh_interval_mins) {
        return invalid_request(msg);
    }
    let new = rag_db::NewCollection {
        name: body.name.trim().to_string(),
        description: body.description.map(|s| s.trim().to_string()),
        git_url: body.git_url.trim().to_string(),
        git_ref: body.git_ref,
        pat: body.pat.filter(|s| !s.is_empty()),
        source,
        profile_id,
        extraction_model: body.extraction_model.filter(|s| !s.is_empty()),
        embedding_model: body.embedding_model.trim().to_string(),
        include_globs: body.include_globs,
        exclude_globs: body.exclude_globs,
        chunk_size: body.chunk_size,
        chunk_overlap: body.chunk_overlap,
        // The JSON API creates single-repo (versioned) collections; aggregate
        // multi-source collections are driven through the /rag admin UI.
        search_mode: match body.search_mode.as_str() {
            "versioned" => rag_db::SearchMode::Versioned,
            "aggregate" => rag_db::SearchMode::Aggregate,
            _ => return invalid_request("`search_mode` must be `versioned` or `aggregate`"),
        },
        refresh_interval_mins: body.refresh_interval_mins,
    };
    match rag_db::create_collection(&state.db, &new).await {
        Ok(c) => match collection_view(&state.db, c).await {
            Ok(view) => (
                StatusCode::CREATED,
                [(header::CONTENT_TYPE, "application/json")],
                serde_json::to_string(&view).unwrap_or_default(),
            )
                .into_response(),
            Err(err) => {
                tracing::warn!(error = %err, "deriving created rag collection status");
                internal_error("deriving collection status failed")
            }
        },
        // sqlx wraps the underlying sqlite error inside `DbError::Query`;
        // pull it out so the operator gets "name already exists" instead
        // of a vague 500.
        Err(err) => {
            if is_unique_violation(&err) {
                return invalid_request(&format!(
                    "a collection named `{}` already exists",
                    new.name
                ));
            }
            tracing::warn!(error = %err, "creating rag collection");
            internal_error("creating collection failed")
        }
    }
}

/// Validate a `source_kind` + settings map against its provider and seal the
/// secret half.
///
/// The split between config and secrets is the provider's to make
/// (`ConfigField::kind`), so callers send one flat map and never have to know
/// which keys are sensitive — the same contract the admin form uses.
/// Turn a wire `source_kind` + flat `source_config` into a storable spec.
///
/// `existing` is the currently stored spec on a PATCH. It matters for two
/// reasons that only show up with an OAuth source: a secret the caller did
/// not resend must keep its stored value rather than be dropped, and the
/// refresh token minted by the consent callback is a secret **no caller ever
/// sends** — rebuilding the blob from the request alone would disconnect the
/// collection every time someone edited an unrelated field.
/// The chunking invariant, in one place.
///
/// Create and PATCH both enforce it and both spelled out the same two
/// messages; the web form has its own copy again with i18n keys. Two had
/// already drifted — the PATCH path only checked the size bound when
/// `chunk_size` was being set.
fn chunk_pair_error(size: i64, overlap: i64) -> Option<&'static str> {
    if size <= 0 || size > 8000 {
        return Some("`chunk_size` must be in (0, 8000]");
    }
    if overlap < 0 || overlap >= size {
        return Some("`chunk_overlap` must satisfy 0 <= overlap < chunk_size");
    }
    None
}

/// One rule for both write surfaces.
///
/// `0` is "never"; anything positive is a schedule, and a schedule shorter
/// than five minutes is a typo with consequences — the poll loop would then
/// re-walk (and for a git source, re-clone) every collection continuously.
/// Refused rather than clamped, so the stored value is the one that was asked
/// for.
fn refresh_interval_error(mins: i64) -> Option<&'static str> {
    if mins == 0 || (5..=525_600).contains(&mins) {
        return None;
    }
    Some("`refresh_interval_mins` must be 0 (never) or between 5 and 525600")
}

fn build_source(
    state: &RamaState,
    kind: &str,
    config: &std::collections::BTreeMap<String, String>,
    existing: Option<&rag_db::SourceSpec>,
) -> Result<rag_db::SourceSpec, String> {
    use aiplane_features::server::rag::source::ProviderConfig;

    if kind == "git" {
        return Ok(rag_db::SourceSpec::default());
    }
    let registry = source_registry(state);
    let factory = registry.get(kind).ok_or_else(|| {
        let known: Vec<&str> = std::iter::once("git")
            .chain(registry.factories().iter().map(|f| f.kind()))
            .collect();
        format!(
            "unknown `source_kind` `{kind}` (known: {})",
            known.join(", ")
        )
    })?;
    let secret_keys = factory.secret_keys();
    let mut values = std::collections::BTreeMap::new();
    let mut secrets = existing
        .filter(|s| s.kind == kind)
        .map(|s| s.open_secrets(&state.crypto))
        .unwrap_or_default();
    for (k, v) in config {
        if v.is_empty() {
            continue;
        }
        if secret_keys.contains(&k.as_str()) {
            secrets.insert(k.clone(), v.clone());
        } else {
            values.insert(k.clone(), v.clone());
        }
    }
    let cfg = ProviderConfig::new(values.clone(), secrets.clone());
    factory.validate(&cfg).map_err(|e| e.to_string())?;
    // Build once so a malformed URL is a 400 here rather than a failed build
    // discovered later on the indexing timeline.
    //
    // Except before consent: an OAuth source cannot be built until someone
    // has clicked through the provider's consent screen, and that needs a
    // saved collection to hang the flow on. Same rule the web form applies —
    // without it, creating a Drive collection over the API is impossible.
    if !aiplane_features::server::rag::source::awaiting_consent(factory.as_ref(), &secrets) {
        factory
            .build(
                &cfg,
                &aiplane_features::server::rag::source::ProviderContext::new(state.http.clone()),
            )
            .map_err(|e| e.to_string())?;
    }

    let sealed = if secrets.is_empty() {
        None
    } else {
        let json = serde_json::to_string(&secrets).map_err(|e| e.to_string())?;
        Some(state.crypto.seal_str(&json).map_err(|e| e.to_string())?)
    };
    Ok(rag_db::SourceSpec {
        kind: kind.to_string(),
        config: values,
        secrets: sealed,
    })
}

/// The registry to validate against, falling back to the built-ins when no
/// indexer is wired so the endpoint still describes what exists.
fn source_registry(state: &RamaState) -> &aiplane_features::server::rag::source::ProviderRegistry {
    state.provider_registry()
}

/// Re-queue one ref for indexing, through the indexer when there is one and
/// straight into the queue table when there is not.
async fn requeue_ref(
    state: &RamaState,
    ref_id: i64,
) -> Result<(), aiplane_core::server::db::DbError> {
    if let Some(indexer) = state.indexer.as_ref() {
        indexer.request_reindex(ref_id).await
    } else {
        rag_db::request_ref_reindex(&state.db, ref_id).await
    }
}

async fn request_full_rebuild(
    state: &RamaState,
    ref_id: i64,
) -> Result<(), aiplane_core::server::db::DbError> {
    if let Some(indexer) = state.indexer.as_ref() {
        indexer.request_full_rebuild(ref_id).await
    } else {
        rag_db::request_full_rebuild(&state.db, ref_id).await
    }
}

fn index_targets(
    collection: &rag_db::Collection,
    refs: Vec<rag_db::CollectionRef>,
) -> Vec<rag_db::CollectionRef> {
    if collection.search_mode == rag_db::SearchMode::Aggregate {
        refs.into_iter().filter(|r| r.is_primary).collect()
    } else {
        refs
    }
}

/// After a source is added to or removed from an AGGREGATE collection,
/// re-queue its primary ref.
///
/// An aggregate collection keeps ONE unified index, built from every source
/// and hung off the primary ref. Adding or dropping a source therefore has no
/// effect at all until that index rebuilds — a removed source keeps answering
/// searches and a new one is invisible. Versioned collections index their refs
/// independently, so this is a no-op for them.
async fn requeue_unified_if_aggregate(state: &RamaState, collection_id: i64) {
    let Ok(Some(collection)) = rag_db::find_collection_by_id(&state.db, collection_id).await else {
        return;
    };
    if collection.search_mode != rag_db::SearchMode::Aggregate {
        return;
    }
    if let Ok(Some(primary)) = rag_db::primary_ref(&state.db, collection_id).await {
        let _ = requeue_ref(state, primary.id).await;
    }
}

#[derive(Deserialize)]
pub struct AddRefsRequest {
    /// One or more sources. Each entry is a URL plus an optional ref; an
    /// entry without one inherits the collection's `git_ref`, which is what
    /// makes pasting a list of repositories work.
    pub sources: Vec<AddRefEntry>,
}

#[derive(Deserialize)]
pub struct AddRefEntry {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub git_ref: Option<String>,
}

/// POST /api/v0/rag/collections/{id}/refs — add sources to a collection.
///
/// Takes a list rather than a single entry because the collections this
/// exists for are aggregates of tens of repositories; adding one is the
/// one-element case. A duplicate (same url+ref already present) is skipped
/// rather than fatal, so re-submitting a list is idempotent.
pub async fn add_refs(
    State(state): State<Arc<RamaState>>,
    Path(id): Path<i64>,
    req: Request,
) -> Response {
    if let Err(resp) = require_admin(&state, &req).await {
        return resp;
    }
    let collection = match rag_db::find_collection_by_id(&state.db, id).await {
        Ok(Some(c)) => c,
        Ok(None) => return not_found(&format!("no collection {id}")),
        Err(err) => {
            tracing::warn!(error = %err, %id, "adding refs: collection lookup");
            return internal_error("collection lookup failed");
        }
    };
    let body = match read_json::<AddRefsRequest>(req).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    let entries: Vec<(String, String)> = body
        .sources
        .into_iter()
        .filter_map(|e| {
            let url = if collection.search_mode == rag_db::SearchMode::Aggregate {
                e.url.trim().to_string()
            } else {
                collection.git_url.clone()
            };
            if url.is_empty() {
                return None;
            }
            let git_ref = e
                .git_ref
                .map(|r| r.trim().trim_start_matches('@').to_string())
                .filter(|r| !r.is_empty())
                .unwrap_or_else(|| collection.git_ref.clone());
            Some((url, git_ref))
        })
        .collect();
    if entries.is_empty() {
        return invalid_request("`sources` must contain at least one url");
    }

    // Default to "the collection already has refs" when the lookup fails.
    // The error path must not be the one that mints a second primary: that
    // breaks the one-primary invariant the UI and aggregate search read, and
    // nothing downstream repairs it. Not knowing means not promoting.
    let had_refs = rag_db::list_refs(&state.db, id)
        .await
        .map(|r| !r.is_empty())
        .unwrap_or(true);
    let mut added = Vec::new();
    let mut skipped = 0usize;
    for (i, (url, git_ref)) in entries.iter().enumerate() {
        // The first source of an empty collection becomes primary — harmless
        // in aggregate mode (search ignores primacy there) but it keeps the
        // one-primary invariant the UI reads.
        let is_primary = !had_refs && i == 0;
        let stored_url =
            (collection.search_mode == rag_db::SearchMode::Aggregate).then_some(url.as_str());
        match rag_db::add_ref(&state.db, id, git_ref, stored_url, is_primary).await {
            Ok(r) => {
                let _ = requeue_ref(&state, r.id).await;
                added.push(json!({ "id": r.id, "git_url": r.git_url, "git_ref": r.git_ref }));
            }
            Err(_) => skipped += 1,
        }
    }
    if !added.is_empty() {
        requeue_unified_if_aggregate(&state, id).await;
    }
    json_ok(&json!({ "added": added, "skipped": skipped }))
}

/// POST /api/v0/rag/collections/{id}/refs/{ref_id}/primary — make one ref the
/// collection's search default.
pub async fn set_primary_ref(
    State(state): State<Arc<RamaState>>,
    Path(RagRefPath { id, ref_id }): Path<RagRefPath>,
    req: Request,
) -> Response {
    if let Err(resp) = require_admin(&state, &req).await {
        return resp;
    }
    // Scope the ref to the collection in the path: an id belonging to another
    // collection would otherwise repoint *its* primary through this route.
    match rag_db::find_ref_by_id(&state.db, ref_id).await {
        Ok(Some(r)) if r.collection_id == id => {}
        Ok(_) => return not_found(&format!("no ref {ref_id} in collection {id}")),
        Err(err) => {
            tracing::warn!(error = %err, ref_id, "primary: ref lookup");
            return internal_error("ref lookup failed");
        }
    }
    match rag_db::set_primary(&state.db, ref_id).await {
        Ok(()) => json_ok(&json!({ "primary": ref_id, "collection": id })),
        Err(err) => {
            tracing::warn!(error = %err, ref_id, "setting primary ref");
            internal_error("setting the primary ref failed")
        }
    }
}

#[derive(Deserialize)]
pub struct ProfileRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub prompt: String,
    #[serde(default)]
    pub fields: Vec<rag_documents::ProfileField>,
}

impl ProfileRequest {
    /// Validate and lower into the DB input. The rules match the web form's:
    /// a profile needs a name and a prompt, and every field needs a key the
    /// query tool can filter on.
    fn into_input(self) -> Result<rag_documents::ProfileInput, String> {
        let name = self.name.trim().to_string();
        if name.is_empty() || name.len() > 64 {
            return Err("`name` must be 1..=64 characters".into());
        }
        // The name addresses the profile in `PUT`/`DELETE /rag/profiles/{name}`,
        // and rama lowercases path segments before matching. A name carrying
        // anything outside this set — an uppercase letter included — creates a
        // row that can never be found again through the API.
        if !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err("`name` may only contain a-z, 0-9, `_` and `-`".into());
        }
        let prompt = self.prompt.trim().to_string();
        if prompt.is_empty() {
            return Err("`prompt` must not be empty".into());
        }
        if self.fields.is_empty() {
            return Err("`fields` must contain at least one field".into());
        }
        let mut seen = std::collections::HashSet::new();
        for f in &self.fields {
            if f.key.trim().is_empty() {
                return Err("every field needs a `key`".into());
            }
            // Keys are the EAV table's primary key; a duplicate silently
            // shadows the other rather than failing.
            if !seen.insert(f.key.clone()) {
                return Err(format!("duplicate field key `{}`", f.key));
            }
            if f.field_type == rag_documents::FieldType::Enum && f.values.is_empty() {
                return Err(format!(
                    "field `{}` is an enum, so it needs at least one value",
                    f.key
                ));
            }
        }
        Ok(rag_documents::ProfileInput {
            name,
            description: self
                .description
                .map(|d| d.trim().to_string())
                .filter(|d| !d.is_empty()),
            prompt,
            fields: self.fields,
        })
    }
}

/// POST /api/v0/rag/profiles — create an extraction profile.
pub async fn create_profile(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    if let Err(resp) = require_admin(&state, &req).await {
        return resp;
    }
    let body = match read_json::<ProfileRequest>(req).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    let input = match body.into_input() {
        Ok(i) => i,
        Err(msg) => return invalid_request(&msg),
    };
    if let Ok(Some(_)) = rag_documents::find_profile_by_name(&state.db, &input.name).await {
        return invalid_request(&format!("a profile named `{}` already exists", input.name));
    }
    match rag_documents::create_profile(&state.db, &input).await {
        Ok(_) => json_ok(&json!({ "name": input.name })),
        Err(err) => {
            tracing::warn!(error = %err, "creating extraction profile");
            internal_error("creating the profile failed")
        }
    }
}

/// PUT /api/v0/rag/profiles/{name} — replace a profile's prompt and fields.
///
/// The write bumps the profile version, which invalidates every extraction
/// cached under it: the collections listed in the response have to re-index
/// before they answer with the new shape. Saying which ones beats an operator
/// wondering why nothing changed.
pub async fn update_profile(
    Path(name): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    if let Err(resp) = require_admin(&state, &req).await {
        return resp;
    }
    let existing = match rag_documents::find_profile_by_name(&state.db, &name).await {
        Ok(Some(p)) => p,
        Ok(None) => return not_found("no such profile"),
        Err(err) => {
            tracing::warn!(error = %err, "looking up extraction profile");
            return internal_error("profile lookup failed");
        }
    };
    let body = match read_json::<ProfileRequest>(req).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    let input = match body.into_input() {
        Ok(i) => i,
        Err(msg) => return invalid_request(&msg),
    };
    if let Err(err) = rag_documents::update_profile(&state.db, existing.id, &input).await {
        tracing::warn!(error = %err, id = existing.id, "updating extraction profile");
        return internal_error("saving the profile failed");
    }
    let affected = rag_documents::collections_using_profile(&state.db, existing.id)
        .await
        .unwrap_or_default();
    json_ok(&json!({ "name": input.name, "reindex_required_by": affected }))
}

/// DELETE /api/v0/rag/profiles/{name} — remove a profile.
///
/// Refused while a collection still points at it: a collection whose profile
/// vanished indexes without fields, which reads as a puzzle rather than an
/// error. Built-in profiles are not deletable at all.
pub async fn delete_profile(
    Path(name): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    if let Err(resp) = require_admin(&state, &req).await {
        return resp;
    }
    let existing = match rag_documents::find_profile_by_name(&state.db, &name).await {
        Ok(Some(p)) => p,
        Ok(None) => return not_found("no such profile"),
        Err(err) => {
            tracing::warn!(error = %err, "looking up extraction profile");
            return internal_error("profile lookup failed");
        }
    };
    if existing.builtin {
        return invalid_request("a built-in profile cannot be deleted");
    }
    let users = rag_documents::collections_using_profile(&state.db, existing.id)
        .await
        .unwrap_or_default();
    if !users.is_empty() {
        return invalid_request(&format!(
            "still used by: {} — point them at another profile first",
            users.join(", ")
        ));
    }
    match rag_documents::delete_profile(&state.db, existing.id).await {
        Ok(true) => json_ok(&json!({ "deleted": true })),
        Ok(false) => not_found("no such profile"),
        Err(err) => {
            tracing::warn!(error = %err, "deleting extraction profile");
            internal_error("deleting the profile failed")
        }
    }
}

#[derive(Deserialize)]
pub struct TestSourceRequest {
    pub source_kind: String,
    #[serde(default)]
    pub source_config: std::collections::BTreeMap<String, String>,
    /// An existing collection whose stored secret may stand in for a blank
    /// password field, so testing an edit does not require retyping it.
    #[serde(default)]
    pub collection_id: Option<i64>,
}

/// POST /api/v0/rag/test-source — reach the configured source and report what
/// came back, before anything is saved.
///
/// The point is to fail on the operator's screen rather than silently on the
/// indexing timeline hours later: wrong host, wrong credentials and wrong
/// folder all look identical in a collection that simply never fills up.
///
/// `git` has nothing to probe (the clone is the test), so it is refused
/// explicitly rather than answering a meaningless success.
pub async fn test_source(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    use aiplane_features::server::rag::source::ProviderConfig;

    if let Err(resp) = require_admin(&state, &req).await {
        return resp;
    }
    let body = match read_json::<TestSourceRequest>(req).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    if body.source_kind == "git" {
        return invalid_request("a git source is tested by indexing it, not by probing");
    }
    // Only the collection's *own* stored secret may stand in, and only for the
    // settings it was stored against — otherwise this probe would present a
    // saved credential to whatever host the caller named. Matching on `kind`
    // alone is NOT enough for that: `build_source` takes `values` (the URL
    // included) straight from the request body while seeding `secrets` from
    // storage, so a body of `{"collection_id": 7, "source_config": {"url":
    // "https://attacker.example/"}}` would authenticate against the caller's
    // host using collection 7's sealed credential — one this API deliberately
    // never returns (`source_secrets_set` is a bool).
    //
    // So the whole non-secret config must be byte-for-byte what is stored.
    // Comparing everything rather than naming the "destination" fields is
    // deliberate: which settings decide where the bytes go is provider
    // knowledge, and anything that guessed would be wrong for the first
    // provider that puts its host somewhere unexpected. The cost is that
    // after editing a setting the secret has to be retyped to test — which
    // the error below says.
    let stored = match body.collection_id {
        Some(id) => rag_db::find_collection_by_id(&state.db, id)
            .await
            .ok()
            .flatten()
            .map(|c| c.source)
            .filter(|spec| spec.kind == body.source_kind),
        None => None,
    };
    let existing = match stored {
        Some(spec) => {
            let Some(factory) = source_registry(&state).get(&body.source_kind) else {
                return invalid_request(&format!("unknown `source_kind` `{}`", body.source_kind));
            };
            let secret_keys = factory.secret_keys();
            let submitted_public: std::collections::BTreeMap<&str, &str> = body
                .source_config
                .iter()
                .filter(|(k, _)| !secret_keys.contains(&k.as_str()))
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            let stored_public: std::collections::BTreeMap<&str, &str> = spec
                .config
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            if submitted_public != stored_public {
                return invalid_request(
                    "the stored credentials can only be reused when the rest of the settings \
                     are unchanged — re-enter the secret to test these ones",
                );
            }
            Some(spec)
        }
        None => None,
    };
    let spec = match build_source(
        &state,
        &body.source_kind,
        &body.source_config,
        existing.as_ref(),
    ) {
        Ok(spec) => spec,
        Err(msg) => return invalid_request(&msg),
    };
    let registry = source_registry(&state);
    let secrets = spec.open_secrets(&state.crypto);
    // No cache directory: "Test connection" must not leave bytes behind for a
    // collection that may never be saved.
    let provider = match registry.build(
        &spec.kind,
        &ProviderConfig::new(spec.config, secrets),
        &aiplane_features::server::rag::source::ProviderContext::new(state.http.clone()),
    ) {
        Ok(p) => p,
        Err(err) => return invalid_request(&err.to_string()),
    };
    match provider.probe().await {
        Ok(report) => json_ok(&json!({
            "ok": true,
            "account": report.account,
            "root_entries": report.root_entries,
            "server": report.server,
        })),
        // A failed probe is the endpoint working: the operator asked whether
        // this source is reachable and the answer is no, with the reason.
        Err(err) => json_ok(&json!({
            "ok": false,
            "error": err.to_string(),
        })),
    }
}

/// GET /api/v0/rag/providers — the source kinds this gateway can index, and
/// the settings each one takes.
///
/// Exists so a client can build a form (or a config file) for a provider it
/// has no compiled-in knowledge of — the same descriptors the admin UI
/// renders from.
pub async fn list_providers(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    if let Err(resp) = require_admin(&state, &req).await {
        return resp;
    }
    use aiplane_features::server::rag::source::FieldKind;
    let mut providers = vec![json!({
        "kind": "git",
        "label": "Git repository",
        "description": "Clones a repository and indexes its files.",
        "auth": {"kind": "fields"},
        "fields": [],
    })];
    providers.extend(source_registry(&state).factories().iter().map(|f| {
        let fields: Vec<serde_json::Value> = f
            .config_fields()
            .iter()
            .map(|field| {
                json!({
                    "key": field.key,
                    "label": field.label,
                    "help": field.help,
                    "required": field.required,
                    "kind": field.kind.as_str(),
                    "secret": field.kind == FieldKind::Secret,
                    "default": field.default,
                })
            })
            .collect();
        // How the provider is authorised, so a client can tell "fill in these
        // fields and you are done" from "fill these in, save, then send a
        // human through a browser". Without this a caller cannot explain why
        // its freshly created collection is not indexing.
        let auth = match f.auth() {
            aiplane_features::server::rag::source::AuthKind::Fields => json!({"kind": "fields"}),
            aiplane_features::server::rag::source::AuthKind::OAuth2 { scopes, .. } => json!({
                "kind": "oauth2",
                "scopes": scopes,
                "connect_path": "/rag/{collection_id}/connect",
            }),
        };
        json!({
            "kind": f.kind(),
            "label": f.label(),
            "description": f.description(),
            "auth": auth,
            "fields": fields,
        })
    }));
    let mut embedding_models = state
        .upstreams
        .models_for_kind(aiplane_core::server::upstreams::config::PoolKind::Embedding);
    embedding_models.sort();
    let default_embedding = aiplane_core::server::feature_defaults::get(
        &state.db,
        aiplane_core::server::feature_defaults::Feature::Embedding,
    )
    .await
    .filter(|model| embedding_models.contains(model));
    json_ok(&json!({
        "data": providers,
        "embedding_models": embedding_models,
        "default_embedding": default_embedding,
        // The collection editor's access picker renders from this; a group
        // name only restricts anything when it matches a group exactly.
        "groups": aiplane_core::server::db::gateway_groups::list_group_names(&state.db)
            .await
            .unwrap_or_default(),
    }))
}

/// GET /api/v0/rag/profiles — the extraction profiles this gateway knows,
/// and the fields each one pulls out of a document.
///
/// A collection references a profile **by name** on create/PATCH; this is how
/// a caller discovers which names exist and what querying against one will
/// give them.
pub async fn list_profiles(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    if let Err(resp) = require_admin(&state, &req).await {
        return resp;
    }
    let profiles = match rag_documents::list_profiles(&state.db).await {
        Ok(p) => p,
        Err(err) => {
            tracing::warn!(error = %err, "listing extraction profiles");
            return internal_error("listing profiles failed");
        }
    };
    let data: Vec<serde_json::Value> = profiles
        .iter()
        .map(|p| {
            json!({
                "id": p.id,
                "name": p.name,
                "description": p.description,
                "prompt": p.prompt,
                "version": p.version,
                "builtin": p.builtin,
                "fields": p.fields.iter().map(|f| json!({
                    "key": f.key,
                    "label": f.label,
                    "type": f.field_type,
                    "description": f.description,
                    "values": f.values,
                    "filterable": f.filterable,
                    "sortable": f.sortable,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    json_ok(&json!({ "data": data }))
}

/// True when `err` is a SQLite UNIQUE-constraint violation; reaches
/// through the `DbError::Query(sqlx::Error::Database(...))` envelope
/// because `DbError`'s `Display` is intentionally terse (`"query"`).
fn is_unique_violation(err: &aiplane_core::server::db::DbError) -> bool {
    use aiplane_core::server::db::DbError;
    let DbError::Query(sqlx::Error::Database(db_err)) = err else {
        return false;
    };
    // SQLite uses code "2067" for UNIQUE constraint failures; the
    // string form ("SQLITE_CONSTRAINT_UNIQUE") shows up via `.code()`
    // depending on sqlx version, so check both.
    db_err.code().as_deref() == Some("2067") || db_err.message().contains("UNIQUE")
}

pub async fn update_collection(
    State(state): State<Arc<RamaState>>,
    Path(id): Path<i64>,
    req: Request,
) -> Response {
    if let Err(resp) = require_admin(&state, &req).await {
        return resp;
    }
    let lang = session_core::i18n::Lang::from_request(req.headers());
    let body = match read_json::<UpdateRequest>(req).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    let before = match rag_db::find_collection_by_id(&state.db, id).await {
        Ok(Some(c)) => c,
        Ok(None) => return not_found(&format!("no collection with id {id}")),
        Err(err) => {
            tracing::warn!(error = %err, %id, "pre-update lookup");
            return internal_error("collection lookup failed");
        }
    };
    let allowed_groups = body.allowed_groups.clone();
    // Same gate as the pool and connector saves: a group name matching nothing
    // makes the collection invisible and unsearchable rather than restricted.
    if let Some(groups) = allowed_groups.as_ref() {
        match aiplane_core::server::db::gateway_groups::unknown_added_groups_message(
            &state.db,
            lang,
            groups,
            &before.allowed_groups,
        )
        .await
        {
            Ok(Some(message)) => return invalid_request(&message),
            Ok(None) => {}
            Err(err) => {
                tracing::warn!(error = %err, %id, "validating rag collection access");
                return internal_error("validating collection access failed");
            }
        }
    }
    let mut sets: Vec<&'static str> = Vec::new();
    let mut bindings: Vec<UpdateBinding> = Vec::new();
    if let Some(desc) = body.description {
        sets.push("description = ?");
        bindings.push(UpdateBinding::OptStr(desc));
    }
    if let Some(git_ref) = body.git_ref {
        if git_ref.trim().is_empty() {
            return invalid_request("`git_ref` must not be empty");
        }
        sets.push("git_ref = ?");
        bindings.push(UpdateBinding::Str(git_ref));
    }
    if let Some(git_url) = body.git_url {
        if before.source.is_git() && git_url.trim().is_empty() {
            return invalid_request("`git_url` must not be empty");
        }
        sets.push("git_url = ?");
        bindings.push(UpdateBinding::Str(git_url.trim().to_string()));
    }
    if let Some(pat) = body.pat {
        sets.push("pat = ?");
        bindings.push(UpdateBinding::OptStr(pat.filter(|s| !s.is_empty())));
    }
    match (body.source_kind, body.source_config) {
        (Some(kind), Some(config)) => {
            let source = match build_source(&state, &kind, &config, Some(&before.source)) {
                Ok(spec) => spec,
                Err(msg) => return invalid_request(&msg),
            };
            let config_json = serde_json::to_string(&source.config).unwrap_or_else(|_| "{}".into());
            sets.push("source_kind = ?");
            bindings.push(UpdateBinding::Str(source.kind));
            sets.push("source_config_json = ?");
            bindings.push(UpdateBinding::Str(config_json));
            sets.push("source_secrets_ct = ?");
            bindings.push(UpdateBinding::OptBlob(
                source.secrets.as_ref().map(|s| s.ciphertext.clone()),
            ));
            sets.push("source_secrets_nonce = ?");
            bindings.push(UpdateBinding::OptBlob(
                source.secrets.as_ref().map(|s| s.nonce.clone()),
            ));
        }
        (None, None) => {}
        _ => {
            return invalid_request(
                "`source_kind` and `source_config` must be sent together — a settings map \
                 has no meaning without the kind whose schema defines it",
            );
        }
    }
    if let Some(model) = body.embedding_model {
        if model.trim().is_empty() {
            return invalid_request("`embedding_model` must not be empty");
        }
        sets.push("embedding_model = ?");
        bindings.push(UpdateBinding::Str(model));
    }
    if let Some(profile) = body.profile {
        let profile_id = match profile.as_deref().filter(|name| !name.is_empty()) {
            None => None,
            Some(name) => match rag_documents::find_profile_by_name(&state.db, name).await {
                Ok(Some(profile)) => Some(profile.id),
                Ok(None) => {
                    return invalid_request(&format!("no extraction profile named `{name}`"));
                }
                Err(err) => {
                    tracing::warn!(error = %err, "looking up extraction profile");
                    return internal_error("profile lookup failed");
                }
            },
        };
        sets.push("profile_id = ?");
        bindings.push(UpdateBinding::OptInt(profile_id));
    }
    if let Some(model) = body.extraction_model {
        sets.push("extraction_model = ?");
        bindings.push(UpdateBinding::OptStr(
            model
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
        ));
    }
    if let Some(search_mode) = body.search_mode {
        match search_mode.as_str() {
            "versioned" | "aggregate" => {
                sets.push("search_mode = ?");
                bindings.push(UpdateBinding::Str(search_mode));
            }
            _ => return invalid_request("`search_mode` must be `versioned` or `aggregate`"),
        }
    }
    if let Some(globs) = body.include_globs {
        let s = match serde_json::to_string(&globs) {
            Ok(s) => s,
            Err(_) => return invalid_request("could not encode include_globs"),
        };
        sets.push("include_globs_json = ?");
        bindings.push(UpdateBinding::Str(s));
    }
    if let Some(globs) = body.exclude_globs {
        let s = match serde_json::to_string(&globs) {
            Ok(s) => s,
            Err(_) => return invalid_request("could not encode exclude_globs"),
        };
        sets.push("exclude_globs_json = ?");
        bindings.push(UpdateBinding::Str(s));
    }
    // Validated as a *pair*, against whatever the other half will be after
    // this edit. Checking each field alone let either one be moved past the
    // other: the chunker clamps the overlap so nothing breaks, but the stored
    // numbers then disagree with the ones actually used, and the web form —
    // which does check the pair — refuses to save the collection at all,
    // leaving it uneditable in the UI.
    if body.chunk_size.is_some() || body.chunk_overlap.is_some() {
        let size = body.chunk_size.unwrap_or(before.chunk_size);
        let overlap = body.chunk_overlap.unwrap_or(before.chunk_overlap);
        if let Some(msg) = chunk_pair_error(size, overlap) {
            return invalid_request(msg);
        }
    }
    if let Some(cs) = body.chunk_size {
        sets.push("chunk_size = ?");
        bindings.push(UpdateBinding::Int(cs));
    }
    if let Some(co) = body.chunk_overlap {
        sets.push("chunk_overlap = ?");
        bindings.push(UpdateBinding::Int(co));
    }
    if let Some(mins) = body.refresh_interval_mins {
        if let Some(msg) = refresh_interval_error(mins) {
            return invalid_request(msg);
        }
        sets.push("refresh_interval_mins = ?");
        bindings.push(UpdateBinding::Int(mins));
    }
    if sets.is_empty() && allowed_groups.is_none() {
        // Nothing to do — still surface the current row so the caller
        // can write a UI that doesn't special-case the empty diff.
        return match rag_db::find_collection_by_id(&state.db, id).await {
            Ok(Some(c)) => match collection_view(&state.db, c).await {
                Ok(view) => json_ok(&view),
                Err(err) => {
                    tracing::warn!(error = %err, %id, "deriving rag collection status");
                    internal_error("deriving collection status failed")
                }
            },
            Ok(None) => not_found(&format!("no collection with id {id}")),
            Err(err) => {
                tracing::warn!(error = %err, %id, "lookup rag collection");
                internal_error("collection lookup failed")
            }
        };
    }
    if !sets.is_empty() {
        let now = Timestamp::now().to_string();
        sets.push("updated_at = ?");
        bindings.push(UpdateBinding::Str(now));
        let sql = format!(
            "UPDATE rag_collections SET {} WHERE id = ?",
            sets.join(", ")
        );
        let mut q = sqlx::query(&sql);
        for b in &bindings {
            q = match b {
                UpdateBinding::OptStr(s) => q.bind(s),
                UpdateBinding::Str(s) => q.bind(s),
                UpdateBinding::Int(i) => q.bind(i),
                UpdateBinding::OptInt(i) => q.bind(i),
                UpdateBinding::OptBlob(b) => q.bind(b),
            };
        }
        q = q.bind(id);
        if let Err(err) = q.execute(&state.db).await {
            tracing::warn!(error = %err, %id, "updating rag collection");
            return internal_error("updating collection failed");
        }
    }
    if let Some(groups) = allowed_groups
        && let Err(err) = rag_db::set_allowed_groups(&state.db, id, &groups).await
    {
        tracing::warn!(error = %err, %id, "updating rag collection access");
        return internal_error("updating collection access failed");
    }
    let after = match rag_db::find_collection_by_id(&state.db, id).await {
        Ok(Some(c)) => c,
        Ok(None) => return not_found(&format!("no collection with id {id}")),
        Err(err) => {
            tracing::warn!(error = %err, %id, "post-update lookup");
            return internal_error("collection lookup failed");
        }
    };
    // Same rule as the web editor, from the same predicate: swapping the
    // source, the profile or the embedding model over the API has to re-queue
    // too, or the corpus keeps answering out of a store that no longer matches
    // its own settings.
    if rag_db::index_shape_changed(&before, &after) {
        for r in index_targets(
            &after,
            rag_db::list_refs(&state.db, id).await.unwrap_or_default(),
        ) {
            let _ = request_full_rebuild(&state, r.id).await;
        }
    }
    match collection_view(&state.db, after).await {
        Ok(view) => json_ok(&view),
        Err(err) => {
            tracing::warn!(error = %err, %id, "deriving rag collection status");
            internal_error("deriving collection status failed")
        }
    }
}

enum UpdateBinding {
    OptStr(Option<String>),
    Str(String),
    Int(i64),
    OptInt(Option<i64>),
    /// Sealed ciphertext / nonce, which are BLOB columns.
    OptBlob(Option<Vec<u8>>),
}

pub async fn delete_collection(
    State(state): State<Arc<RamaState>>,
    Path(id): Path<i64>,
    req: Request,
) -> Response {
    if let Err(resp) = require_admin(&state, &req).await {
        return resp;
    }
    // Capture every ref's store folder before the cascade delete so we can
    // reap them all (each ref has its own <data_dir>/<uuid>/).
    let refs = rag_db::list_refs(&state.db, id).await.unwrap_or_default();
    match rag_db::delete_collection(&state.db, id).await {
        Ok(true) => {
            if let Some(indexer) = state.indexer.as_ref() {
                for r in &refs {
                    indexer.drop_ref_storage(r.id, &r.data_uuid);
                }
            }
            json_ok(&json!({ "deleted": true }))
        }
        Ok(false) => not_found(&format!("no collection with id {id}")),
        Err(err) => {
            tracing::warn!(error = %err, %id, "delete rag collection");
            internal_error("delete failed")
        }
    }
}

/// POST /api/v0/rag/collections/{id}/reindex — re-queue every index that
/// contributes to this collection's search result.
pub async fn reindex_collection(
    State(state): State<Arc<RamaState>>,
    Path(id): Path<i64>,
    req: Request,
) -> Response {
    if let Err(resp) = require_admin(&state, &req).await {
        return resp;
    }
    match rag_db::find_collection_by_id(&state.db, id).await {
        Ok(Some(c)) => {
            let refs = match rag_db::list_refs(&state.db, id).await {
                Ok(refs) => index_targets(&c, refs),
                Err(err) => {
                    tracing::warn!(error = %err, %id, "listing collection refs for reindex");
                    return internal_error("listing collection sources failed");
                }
            };
            for source in refs {
                if let Err(err) = requeue_ref(&state, source.id).await {
                    tracing::warn!(error = %err, %id, ref_id = source.id, "reindex request");
                    return internal_error("queueing collection source failed");
                }
            }
            match collection_view(&state.db, c).await {
                Ok(view) => json_ok(&view),
                Err(err) => {
                    tracing::warn!(error = %err, %id, "deriving rag collection status");
                    internal_error("deriving collection status failed")
                }
            }
        }
        Ok(None) => not_found(&format!("no collection with id {id}")),
        Err(err) => {
            tracing::warn!(error = %err, %id, "post-reindex lookup");
            internal_error("collection lookup failed")
        }
    }
}

// ----- helpers ------------------------------------------------------------

/// Gate for every `/api/v0/rag/*` handler. The RAG collection registry
/// is an operator-global resource (no per-row owner — see
/// `migrations/0013_rag.sql`), so these endpoints are admin-only, exactly
/// like the HTML surface in `pages::rag_*` which gates on
/// `require_admin_or_403`. Anonymous → 401 JSON; an authenticated
/// non-admin → 403 JSON. Returns the session on success.
async fn require_admin(state: &RamaState, req: &Request) -> Result<Session, Response> {
    let session = match state.sessions.lookup_from_headers(req.headers()).await {
        Ok(Some(s)) => s,
        Ok(None) => return Err(unauthorized("no active session — sign in at /auth/login")),
        Err(err) => {
            tracing::warn!(error = %err, "session lookup");
            return Err(internal_error("session lookup failed"));
        }
    };
    let user = match users::find_by_id(&state.db, &session.user_id).await {
        Ok(Some(u)) => u,
        Ok(None) => return Err(unauthorized("session references a missing user")),
        Err(err) => {
            tracing::warn!(error = %err, "user lookup");
            return Err(internal_error("user lookup failed"));
        }
    };
    let role_ids = state.rbac.role_ids_for(&user.roles);
    if !state.rbac.is_admin(&role_ids) {
        return Err(forbidden("admin role required"));
    }
    Ok(session)
}

async fn read_json<T: for<'de> Deserialize<'de>>(req: Request) -> Result<T, Response> {
    let (_, body) = req.into_parts();
    let bytes = match body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return Err(invalid_request(&msg)),
    };
    serde_json::from_slice(&bytes).map_err(|err| invalid_request(&format!("invalid body: {err}")))
}

async fn body_to_bytes(body: rama::http::Body) -> Result<rama::bytes::Bytes, String> {
    use rama::http::body::util::BodyExt;
    body.collect()
        .await
        .map(|c| c.to_bytes())
        .map_err(|e| format!("reading request body: {e}"))
}

fn json_ok<T: Serialize>(value: &T) -> Response {
    let body = match serde_json::to_string(value) {
        Ok(s) => s,
        Err(err) => return internal_error(&format!("serialising response: {err}")),
    };
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        body,
    )
        .into_response()
}

fn not_found(message: &str) -> Response {
    error_envelope(StatusCode::NOT_FOUND, "not_found", message)
}
fn invalid_request(message: &str) -> Response {
    error_envelope(StatusCode::BAD_REQUEST, "invalid_request", message)
}
fn unauthorized(message: &str) -> Response {
    error_envelope(StatusCode::UNAUTHORIZED, "unauthorized", message)
}
fn forbidden(message: &str) -> Response {
    error_envelope(StatusCode::FORBIDDEN, "forbidden", message)
}
fn internal_error(message: &str) -> Response {
    error_envelope(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", message)
}
fn error_envelope(status: StatusCode, code: &str, message: &str) -> Response {
    let body = json!({
        "error": {
            "message": message,
            "type": code,
            "code": code,
        }
    });
    (
        status,
        [(header::CONTENT_TYPE, "application/json")],
        body.to_string(),
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// Refs + sync tokens (issue #22 P5 — the SPA's collection browser)

#[derive(Serialize)]
struct RefView {
    id: i64,
    collection_id: i64,
    git_ref: String,
    git_url: Option<String>,
    is_primary: bool,
    status: String,
    last_indexed_at: Option<String>,
    last_indexed_commit: Option<String>,
    last_error: Option<String>,
    chunk_count: i64,
    document_count: i64,
}

/// GET /api/v0/rag/collections/{id}/refs — the collection's sources/refs
/// with index status and sizes.
pub async fn list_refs(
    State(state): State<Arc<RamaState>>,
    Path(id): Path<i64>,
    req: Request,
) -> Response {
    if let Err(resp) = require_admin(&state, &req).await {
        return resp;
    }
    let refs = match rag_db::list_refs(&state.db, id).await {
        Ok(r) => r,
        Err(err) => {
            tracing::warn!(error = %err, %id, "listing rag refs");
            return internal_error("listing refs failed");
        }
    };
    let files = rag_db::latest_source_files(&state.db, id)
        .await
        .unwrap_or_default();
    let mut views = Vec::with_capacity(refs.len());
    for r in refs {
        let documents = files.get(&r.id).copied().unwrap_or_default();
        let chunks = 0i64;
        views.push(RefView {
            id: r.id,
            collection_id: r.collection_id,
            git_ref: r.git_ref.clone(),
            git_url: r.git_url.clone(),
            is_primary: r.is_primary,
            status: format!("{:?}", r.status).to_lowercase(),
            last_indexed_at: r.last_indexed_at.map(|t| t.to_string()),
            last_indexed_commit: r.last_indexed_commit.clone(),
            last_error: r.last_error.clone(),
            chunk_count: chunks,
            document_count: documents,
        });
    }
    json_ok(&json!({ "data": views }))
}

#[derive(Deserialize)]
pub struct UpdateRefRequest {
    #[serde(default, deserialize_with = "deserialize_option_option")]
    pub git_url: Option<Option<String>>,
    pub git_ref: String,
}

/// PATCH /api/v0/rag/collections/{id}/refs/{ref_id} — change a source's
/// repository and branch/tag, then queue a full rebuild for the new target.
pub async fn update_ref(
    State(state): State<Arc<RamaState>>,
    Path(RagRefPath { id, ref_id }): Path<RagRefPath>,
    req: Request,
) -> Response {
    if let Err(resp) = require_admin(&state, &req).await {
        return resp;
    }
    let body = match read_json::<UpdateRefRequest>(req).await {
        Ok(body) => body,
        Err(resp) => return resp,
    };
    let git_ref = body.git_ref.trim();
    if git_ref.is_empty() {
        return invalid_request("`git_ref` must not be empty");
    }
    let existing = match rag_db::find_ref_by_id(&state.db, ref_id).await {
        Ok(Some(source)) if source.collection_id == id => source,
        Ok(_) => return not_found(&format!("no ref {ref_id} in collection {id}")),
        Err(err) => {
            tracing::warn!(error = %err, ref_id, "updating rag ref: lookup");
            return internal_error("ref lookup failed");
        }
    };
    let git_url = body
        .git_url
        .unwrap_or(existing.git_url)
        .map(|url| url.trim().to_string())
        .filter(|url| !url.is_empty());
    if let Err(err) = rag_db::update_ref(&state.db, ref_id, git_url.as_deref(), git_ref).await {
        tracing::warn!(error = %err, ref_id, "updating rag ref");
        return internal_error("updating the ref failed");
    }
    if let Err(err) = rag_db::request_full_rebuild(&state.db, ref_id).await {
        tracing::warn!(error = %err, ref_id, "queueing updated rag ref");
        return internal_error("the ref was saved but queueing its rebuild failed");
    }
    json_ok(&json!({
        "id": ref_id,
        "collection_id": id,
        "git_url": git_url,
        "git_ref": git_ref,
    }))
}

/// GET /api/v0/rag/collections/{id}/refs/{ref_id}/log — newest indexing
/// events first so an operator can diagnose a failed or stale source.
pub async fn ref_log(
    State(state): State<Arc<RamaState>>,
    Path(RagRefPath { id, ref_id }): Path<RagRefPath>,
    req: Request,
) -> Response {
    if let Err(resp) = require_admin(&state, &req).await {
        return resp;
    }
    match rag_db::find_ref_by_id(&state.db, ref_id).await {
        Ok(Some(source)) if source.collection_id == id => {}
        Ok(_) => return not_found(&format!("no ref {ref_id} in collection {id}")),
        Err(err) => {
            tracing::warn!(error = %err, ref_id, "reading rag ref log: lookup");
            return internal_error("ref lookup failed");
        }
    }
    let entries = match rag_db::list_log_entries(&state.db, ref_id, 100).await {
        Ok(entries) => entries,
        Err(err) => {
            tracing::warn!(error = %err, ref_id, "reading rag ref log");
            return internal_error("reading the index log failed");
        }
    };
    let data: Vec<_> = entries
        .into_iter()
        .map(|entry| {
            json!({
                "id": entry.id,
                "created_at": entry.created_at.to_string(),
                "level": entry.level.as_str(),
                "phase": entry.phase,
                "message": entry.message,
                "commit_sha": entry.commit_sha,
                "files": entry.files,
                "chunks": entry.chunks,
                "duration_ms": entry.duration_ms,
            })
        })
        .collect();
    json_ok(&json!({ "data": data }))
}

/// DELETE /api/v0/rag/collections/{id}/refs/{ref_id} — remove one source
/// (its store folder goes with it).
#[derive(serde::Deserialize)]
pub struct RagRefPath {
    pub id: i64,
    pub ref_id: i64,
}

pub async fn delete_ref(
    State(state): State<Arc<RamaState>>,
    Path(RagRefPath { id, ref_id }): Path<RagRefPath>,
    req: Request,
) -> Response {
    if let Err(resp) = require_admin(&state, &req).await {
        return resp;
    }
    match rag_db::delete_ref(&state.db, ref_id).await {
        Ok(Some(data_uuid)) => {
            // `drop_ref_storage`, not a bare `remove_dir_all`: the indexer's
            // `indexes`/`stores` maps hold live handles to this ref, so
            // deleting the files alone leaves RAG search happily answering
            // from the removed source's in-memory index. Dropping the caches
            // is the half that actually takes it out of service.
            if let Some(indexer) = state.indexer.as_ref() {
                indexer.drop_ref_storage(ref_id, &data_uuid);
            } else if let Some(rag) = state.config().rag.as_ref() {
                // No indexer wired: no caches to evict, so the folder is all
                // there is.
                let _ = tokio::fs::remove_dir_all(rag.data_dir.join(data_uuid)).await;
            }
            requeue_unified_if_aggregate(&state, id).await;
            json_ok(&json!({ "deleted": ref_id, "collection": id }))
        }
        Ok(None) => not_found(&format!("no ref {ref_id}")),
        Err(err) => {
            tracing::warn!(error = %err, ref_id, "deleting rag ref");
            internal_error("deleting the ref failed")
        }
    }
}

/// POST /api/v0/rag/collections/{id}/refs/{ref_id}/rebuild — force a full
/// re-walk of one source on the next index pass.
pub async fn rebuild_ref(
    State(state): State<Arc<RamaState>>,
    Path(RagRefPath { id, ref_id }): Path<RagRefPath>,
    req: Request,
) -> Response {
    if let Err(resp) = require_admin(&state, &req).await {
        return resp;
    }
    let collection = match rag_db::find_collection_by_id(&state.db, id).await {
        Ok(Some(collection)) => collection,
        Ok(None) => return not_found(&format!("no collection {id}")),
        Err(err) => {
            tracing::warn!(error = %err, %id, "looking up rag collection for rebuild");
            return internal_error("looking up collection failed");
        }
    };
    let source = match rag_db::find_ref_by_id(&state.db, ref_id).await {
        Ok(Some(source)) if source.collection_id == id => source,
        Ok(_) => return not_found(&format!("no ref {ref_id} in collection {id}")),
        Err(err) => {
            tracing::warn!(error = %err, ref_id, "looking up rag source for rebuild");
            return internal_error("looking up source failed");
        }
    };
    let target = if collection.search_mode == rag_db::SearchMode::Aggregate {
        match rag_db::primary_ref(&state.db, id).await {
            Ok(Some(primary)) => primary,
            Ok(None) => return invalid_request("the collection has no primary source to rebuild"),
            Err(err) => {
                tracing::warn!(error = %err, %id, "looking up aggregate primary source");
                return internal_error("looking up primary source failed");
            }
        }
    } else {
        source
    };
    match request_full_rebuild(&state, target.id).await {
        Ok(()) => json_ok(&json!({ "requested": target.id })),
        Err(err) => {
            tracing::warn!(error = %err, ref_id = target.id, "requesting rag rebuild");
            internal_error("requesting the rebuild failed")
        }
    }
}

/// POST /api/v0/rag/collections/{id}/sync-token — mint (rotate) the
/// push-to-sync trigger token; the plaintext is shown exactly once.
pub async fn rotate_sync_token(
    State(state): State<Arc<RamaState>>,
    Path(id): Path<i64>,
    req: Request,
) -> Response {
    if let Err(resp) = require_admin(&state, &req).await {
        return resp;
    }
    match rag_db::rotate_sync_token(&state.db, id).await {
        Ok(token) => json_ok(&json!({ "token": token, "url_hint": format!("/hooks/rag/{token}") })),
        Err(err) => {
            tracing::warn!(error = %err, %id, "rotating rag sync token");
            internal_error("rotating the token failed")
        }
    }
}

/// POST /api/v0/rag/collections/{id}/sync-token/clear
pub async fn clear_sync_token(
    State(state): State<Arc<RamaState>>,
    Path(id): Path<i64>,
    req: Request,
) -> Response {
    if let Err(resp) = require_admin(&state, &req).await {
        return resp;
    }
    match rag_db::clear_sync_token(&state.db, id).await {
        Ok(()) => json_ok(&json!({ "cleared": true })),
        Err(err) => {
            tracing::warn!(error = %err, %id, "clearing rag sync token");
            internal_error("clearing the token failed")
        }
    }
}
