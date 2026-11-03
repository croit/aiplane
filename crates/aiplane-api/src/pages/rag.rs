// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The RAG sync webhook (`GET|POST /hooks/rag/{token}`).
//!
//! All that is left of the old `/rag` page: the operator screen is a SPA
//! route now, backed by the JSON handlers in `rama_server::rag_api`.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use rama::http::service::web::extract::State;
use rama::http::{Request, Response, StatusCode, header};

use aiplane_core::server::db::rag as rag_db;
use aiplane_runtime::rama_server::state::RamaState;

/// A per-collection minimum interval between accepted sync-hook calls. The
/// pending-ref check only covers the window while a build is running; a leaked
/// token could otherwise start a fresh full reindex the moment the last one
/// finished. Process-local on purpose: the indexer is one process, and this is
/// an abuse guard, not a durable quota.
const SYNC_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Default)]
struct SyncThrottle(Mutex<HashMap<i64, Instant>>);

impl SyncThrottle {
    /// Whether `collection_id` may start a sync at `now`. The first call and
    /// one after the interval pass; a repeat inside it does not.
    fn admit(&self, collection_id: i64, now: Instant) -> bool {
        let mut last = self.0.lock().unwrap_or_else(|e| e.into_inner());
        last.retain(|_, at| now.duration_since(*at) < SYNC_INTERVAL * 10);
        if let Some(previous) = last.get(&collection_id)
            && now.duration_since(*previous) < SYNC_INTERVAL
        {
            return false;
        }
        last.insert(collection_id, now);
        true
    }
}

static LAST_SYNC: LazyLock<SyncThrottle> = LazyLock::new(SyncThrottle::default);

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
/// What a sync hook queued.
#[derive(serde::Serialize)]
struct SyncQueued {
    collection: String,
    queued: usize,
}

pub async fn rag_sync_hook(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    // Read the token off the raw URI, not through `Path`: rama's extractor
    // lowercases every segment, and `rotate_sync_token` mints from a
    // mixed-case alphabet. Through `Path` a token containing any capital —
    // which is to say very nearly all of them — hashes to something that
    // matches no row, and the hook 404s forever.
    let Some(token) = sync_hook_token(req.uri().path()) else {
        return super::not_found("unknown sync token");
    };
    // A missing collection and a wrong token get the same answer: anything
    // else turns this into an oracle for guessing valid tokens.
    let Ok(Some(collection)) = rag_db::find_by_sync_token(&state.db, &token).await else {
        return super::not_found("unknown sync token");
    };
    if !LAST_SYNC.admit(collection.id, Instant::now()) {
        let mut resp = super::json_error(
            StatusCode::TOO_MANY_REQUESTS,
            "too_many_requests",
            "This collection was synced recently; try again in a minute.",
        );
        resp.headers_mut()
            .insert(header::RETRY_AFTER, header::HeaderValue::from_static("60"));
        return resp;
    }
    let Some(indexer) = state.indexer.as_ref() else {
        return super::json_error(
            rama::http::StatusCode::SERVICE_UNAVAILABLE,
            "indexer_unavailable",
            "the indexer is not running; check the RAG settings at /admin/settings",
        );
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
    super::json_ok(
        rama::http::StatusCode::ACCEPTED,
        SyncQueued {
            collection: collection.name,
            queued,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_sync_inside_the_interval_is_refused() {
        let throttle = SyncThrottle::default();
        let now = Instant::now();
        assert!(throttle.admit(1, now));
        assert!(!throttle.admit(1, now + Duration::from_secs(1)));
        assert!(throttle.admit(2, now));
        assert!(throttle.admit(1, now + SYNC_INTERVAL));
    }
}
