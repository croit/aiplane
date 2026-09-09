// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! `/rag` page — operator-facing CRUD for indexed RAG collections.
//!
//! Mirrors `pages/tokens.rs` in shape: a list of cards, a create form
//! at the top, and per-row Re-index / Delete actions wired via
//! datastar `@post` + SSE patches so the page updates surgically
//! without a full reload. Admin-gated (`require_admin_or_403`); the
//! sidebar entry is only rendered for admins, matching `/admin/*`.
//!
//! Live status: while the page is open it polls `GET /rag/status` on a
//! datastar interval and morphs each ref's `#rag-ref-{id}` status row, so
//! the background indexer's progress (`pending` → `cloning` → `indexing`
//! → `ready`/`error`) — and especially *failures* like a branch that
//! doesn't exist — show up without a manual reload. Each ref also has a
//! "Log" button (`GET /rag/refs/{ref_id}/log`) that opens its full
//! indexing timeline; the ref itself only carries the latest `last_error`,
//! the log keeps the history. The poll deliberately re-patches only the
//! status rows, leaving the add-source inputs and any open log untouched.

use std::sync::Arc;

use rama::http::service::web::extract::State;
use rama::http::service::web::response::IntoResponse;
use rama::http::{Request, Response};

use gateway_core::server::db::rag as rag_db;
use gateway_runtime::rama_server::state::RamaState;

/// The token from `/hooks/rag/{token}`, with its case intact.
///
/// See [`rag_sync_hook`]: the `Path` extractor would lowercase it.
fn sync_hook_token(path: &str) -> Option<String> {
    let tail = path.rsplit_once("/hooks/rag/")?.1;
    let token = tail.trim_end_matches('/');
    (!token.is_empty() && !token.contains('/')).then(|| token.to_string())
}

/// POST /hooks/rag/{token} — re-sync the collection this token belongs to.
///
/// Unauthenticated by design: the token in the URL *is* the credential, the
/// same shape `/hooks/{secret}` uses for user webhooks. Nextcloud's
/// webhook_listeners app (or ownCloud's, or a cron line, or anything that can
/// make an HTTP request) points at this on file events.
///
/// The body is ignored. This is a doorbell, not a change feed: what actually
/// changed is established by the walk that follows, which is cheap on a
/// source that supports subtree pruning. Accepting a payload here would mean
/// trusting an unauthenticated caller's description of the corpus.
pub async fn rag_sync_hook(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    // Read the token off the raw URI, not through `Path`: rama's extractor
    // lowercases every segment, and `rotate_sync_token` mints from a
    // mixed-case alphabet. Through `Path` a token containing any capital —
    // which is to say very nearly all of them — hashes to something that
    // matches no row, and the hook 404s forever.
    let Some(token) = sync_hook_token(req.uri().path()) else {
        return (
            rama::http::StatusCode::NOT_FOUND,
            [(rama::http::header::CONTENT_TYPE, "application/json")],
            r#"{"error":"unknown sync token"}"#,
        )
            .into_response();
    };
    // A missing collection and a wrong token get the same answer: anything
    // else turns this into an oracle for guessing valid tokens.
    let Ok(Some(collection)) = rag_db::find_by_sync_token(&state.db, &token).await else {
        return (
            rama::http::StatusCode::NOT_FOUND,
            [(rama::http::header::CONTENT_TYPE, "application/json")],
            r#"{"error":"unknown sync token"}"#,
        )
            .into_response();
    };
    let Some(indexer) = state.indexer.as_ref() else {
        return (
            rama::http::StatusCode::SERVICE_UNAVAILABLE,
            [(rama::http::header::CONTENT_TYPE, "application/json")],
            r#"{"error":"the indexer is not running"}"#,
        )
            .into_response();
    };
    let refs = rag_db::list_refs(&state.db, collection.id)
        .await
        .unwrap_or_default();
    let mut queued = 0usize;
    for r in &refs {
        // Already-pending refs are left alone: a burst of file events must
        // not re-queue a build that is about to run anyway.
        if r.status == rag_db::CollectionStatus::Pending {
            continue;
        }
        if indexer.request_reindex(r.id).await.is_ok() {
            queued += 1;
        }
    }
    tracing::info!(
        collection = %collection.name,
        queued,
        "rag: sync hook fired"
    );
    (
        rama::http::StatusCode::ACCEPTED,
        [(rama::http::header::CONTENT_TYPE, "application/json")],
        // Serialised, not interpolated: a collection name containing a quote
        // or a backslash would otherwise emit invalid JSON to the caller.
        serde_json::json!({ "collection": collection.name, "queued": queued }).to_string(),
    )
        .into_response()
}
