// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! End-to-end test for indexing a mailing-list archive.
//!
//! Stands up a wiremock server that serves what HyperKitty serves — one
//! gzipped mbox at `export/<list>.mbox.gz` — and drives a real [`Indexer`]
//! over a collection whose source is `hyperkitty`.
//!
//! What this pins, beyond "it works":
//!   * **a thread is one document**, so three messages become one file and a
//!     search for wording that only ever appeared in a *reply* returns the
//!     conversation that contains it — the whole reason this source exists;
//!   * quoting is stripped on the way in, so the question is not indexed once
//!     per reply;
//!   * a second sync fetches only the days since the first one — the whole
//!     export is downloaded once — and the reply it brings back joins its
//!     thread, moves that thread's version, and leaves the other one alone;
//!   * turning the local copy off goes back to downloading the whole export
//!     every time, because that is what the checkbox promises;
//!   * a list URL that 404s fails the ref with a message naming the URL,
//!     instead of going `ready` and empty.

use std::collections::{HashMap, HashSet};
use std::io::Write as _;
use std::sync::Arc;

use gateway_core::server::db::{self, rag as rag_db};
use gateway_core::server::upstreams::{
    UpstreamRegistry,
    config::{BackendConfig, PickerStrategy, PoolKind, UpstreamPoolConfig},
};
use gateway_features::server::embeddings;
use gateway_features::server::rag::worker::{Indexer, IndexerConfig, search_chunks};
use serde_json::{Value, json};
use tempfile::tempdir;
use wiremock::matchers::{method, path, query_param_is_missing};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

/// The list's page path under the mock server, mirroring HyperKitty's layout.
const LIST_PATH: &str = "/hyperkitty/list/users@example.com";
const EXPORT_PATH: &str = "/hyperkitty/list/users@example.com/export/users@example.com.mbox.gz";

/// Two conversations. In each, the answer is in a reply and is quoted nowhere
/// else, so a hit on it can only come from the thread being one document.
const ARCHIVE: &str = "From users@example.com Mon Sep  1 09:00:00 2026\n\
Message-ID: <osd-1@example.com>\n\
Subject: [users] OSD flapping after upgrade\n\
From: Jane Roe <jane@example.com>\n\
Date: Mon, 1 Sep 2026 09:00:00 +0000\n\
Content-Type: text/plain; charset=utf-8\n\
\n\
After upgrading our storage nodes the daemons flap every few minutes.\n\
\n\
From users@example.com Mon Sep  1 10:00:00 2026\n\
Message-ID: <osd-2@example.com>\n\
In-Reply-To: <osd-1@example.com>\n\
Subject: [users] Re: OSD flapping after upgrade\n\
From: John Doe <john@example.com>\n\
Date: Mon, 1 Sep 2026 10:00:00 +0000\n\
Content-Type: text/plain; charset=utf-8\n\
\n\
On Mon, 1 Sep 2026 at 09:00, Jane Roe wrote:\n\
> After upgrading our storage nodes the daemons flap every few minutes.\n\
\n\
Raise heartbeat grace to 30 seconds. Known regression in that release.\n\
\n\
From users@example.com Tue Sep  2 08:00:00 2026\n\
Message-ID: <rgw-1@example.com>\n\
Subject: [users] Multisite sync stuck\n\
From: Ada Byron <ada@example.com>\n\
Date: Tue, 2 Sep 2026 08:00:00 +0000\n\
Content-Type: text/plain; charset=utf-8\n\
\n\
Our second zone stopped catching up yesterday afternoon.\n\
\n\
From users@example.com Tue Sep  2 09:00:00 2026\n\
Message-ID: <rgw-2@example.com>\n\
In-Reply-To: <rgw-1@example.com>\n\
Subject: [users] Re: Multisite sync stuck\n\
From: John Doe <john@example.com>\n\
Date: Tue, 2 Sep 2026 09:00:00 +0000\n\
Content-Type: text/plain; charset=utf-8\n\
\n\
On Tue, 2 Sep 2026 at 08:00, Ada Byron wrote:\n\
> Our second zone stopped catching up yesterday afternoon.\n\
\n\
Cancel the reshard that is blocking the data log and it will resume.\n\
\n";

/// What a date window returns once the thread has gained a reply. Note that
/// it carries *only* the new message: a window is not a second copy of the
/// archive, which is the entire point of fetching one.
const LATE_REPLY: &str = "From users@example.com Wed Sep  3 07:00:00 2026\n\
Message-ID: <osd-3@example.com>\n\
In-Reply-To: <osd-2@example.com>\n\
Subject: [users] Re: OSD flapping after upgrade\n\
From: Jane Roe <jane@example.com>\n\
Date: Wed, 3 Sep 2026 07:00:00 +0000\n\
Content-Type: text/plain; charset=utf-8\n\
\n\
Confirmed, that settled it. Thanks!\n\
";

fn gzip(mbox: &str) -> Vec<u8> {
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    enc.write_all(mbox.as_bytes()).expect("in-memory write");
    enc.finish().expect("in-memory finish")
}

/// A list server: the full export at the bare URL, and a date window when
/// `start` is present. Both are mounted from the beginning — which sync
/// fetches which is the provider's decision, and the test's subject.
async fn start_list(window: &str) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(EXPORT_PATH))
        .and(query_param_is_missing("start"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(gzip(ARCHIVE))
                .insert_header("content-type", "application/gzip"),
        )
        .mount(&server)
        .await;
    let window = gzip(window);
    Mock::given(method("GET"))
        .and(path(EXPORT_PATH))
        .and(|req: &Request| req.url.query_pairs().any(|(k, _)| k == "start"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(window)
                .insert_header("content-type", "application/gzip"),
        )
        .mount(&server)
        .await;
    server
}

/// Every cached archive segment under `dir`, at any depth. The indexer hands
/// each collection its own subdirectory, so this is what "did anything get
/// written" actually means.
fn archive_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(archive_files(&path));
        } else if path.to_string_lossy().ends_with(".mbox.gz") {
            out.push(path);
        }
    }
    out
}

/// How many full-export and how many windowed requests the list has served.
async fn request_split(server: &MockServer) -> (usize, usize) {
    let requests = server.received_requests().await.unwrap_or_default();
    let windowed = requests
        .iter()
        .filter(|r| r.url.query_pairs().any(|(k, _)| k == "start"))
        .count();
    (requests.len() - windowed, windowed)
}

fn one_hot(input: &str) -> [f32; 4] {
    let s = input.to_lowercase();
    if s.contains("heartbeat") {
        [1.0, 0.0, 0.0, 0.0]
    } else if s.contains("reshard") {
        [0.0, 1.0, 0.0, 0.0]
    } else {
        [0.0, 0.0, 0.0, 1.0]
    }
}

async fn start_embedding_upstream() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/embeddings"))
        .respond_with(|req: &Request| {
            let body: Value = serde_json::from_slice(&req.body).unwrap_or(json!({}));
            let inputs: Vec<String> = match body.get("input") {
                Some(Value::Array(a)) => a
                    .iter()
                    .map(|v| v.as_str().unwrap_or_default().to_string())
                    .collect(),
                Some(Value::String(s)) => vec![s.clone()],
                _ => Vec::new(),
            };
            let data: Vec<Value> = inputs
                .iter()
                .enumerate()
                .map(|(i, s)| json!({"object": "embedding", "index": i, "embedding": one_hot(s)}))
                .collect();
            ResponseTemplate::new(200).set_body_json(json!({"object": "list", "data": data}))
        })
        .mount(&server)
        .await;
    server
}

fn registry_pointed_at(upstream_url: &str) -> Arc<UpstreamRegistry> {
    let mut pools = HashMap::new();
    pools.insert(
        "embed".to_string(),
        UpstreamPoolConfig {
            voices: Default::default(),
            offer_voices: Vec::new(),
            allowed_groups: Vec::new(),
            fallback_offline: None,
            compliance: Default::default(),
            enforce_limits: true,
            kind: PoolKind::Embedding,
            strategy: PickerStrategy::RoundRobin,
            models: Vec::new(),
            backend: vec![BackendConfig {
                alias: None,
                probe_models: true,
                supports_edit: false,
                enabled: true,
                name: "mock".into(),
                base_url: upstream_url.into(),
                api_key_env: None,
                api_key: None,
                weight: 1,
                max_inflight: 16,
                health_path: "/models".into(),
                models: Vec::new(),
            }],
        },
    );
    let registry = UpstreamRegistry::new(&pools).unwrap();
    let pool = registry
        .pools()
        .into_iter()
        .find(|p| p.name == "embed")
        .unwrap();
    pool.backends[0].set_models(HashSet::from(["embed-test".to_string()]));
    registry
}

/// The `source` spec a `hyperkitty` collection carries. No secrets: a public
/// archive is the only kind there is.
fn hyperkitty_source(base_url: &str) -> rag_db::SourceSpec {
    source_with_cache(base_url, true)
}

fn source_with_cache(base_url: &str, cache: bool) -> rag_db::SourceSpec {
    rag_db::SourceSpec {
        kind: "hyperkitty".into(),
        config: [
            ("list_url".to_string(), format!("{base_url}{LIST_PATH}/")),
            ("cache_archive".to_string(), cache.to_string()),
        ]
        .into_iter()
        .collect(),
        secrets: None,
    }
}

async fn seed(
    pool: &db::Pool,
    source: rag_db::SourceSpec,
) -> (rag_db::Collection, rag_db::CollectionRef) {
    let collection = rag_db::create_collection(
        pool,
        &rag_db::NewCollection {
            name: "list".into(),
            description: None,
            // Unused by a remote source, but the column is NOT NULL.
            git_url: String::new(),
            git_ref: "main".into(),
            pat: None,
            source,
            profile_id: None,
            extraction_model: None,
            embedding_model: "embed-test".into(),
            include_globs: Vec::new(),
            exclude_globs: Vec::new(),
            chunk_size: 400,
            chunk_overlap: 40,
            search_mode: rag_db::SearchMode::Versioned,
            refresh_interval_mins: 0,
        },
    )
    .await
    .unwrap();
    let r = rag_db::add_ref(pool, collection.id, "main", None, true)
        .await
        .unwrap();
    (collection, r)
}

fn indexer(
    pool: &db::Pool,
    registry: Arc<UpstreamRegistry>,
    data_dir: &std::path::Path,
) -> Indexer {
    Indexer::new(
        pool.clone(),
        registry,
        reqwest::Client::new(),
        IndexerConfig {
            data_dir: data_dir.to_path_buf(),
            ..IndexerConfig::default()
        },
        None,
    )
}

#[tokio::test]
async fn a_thread_is_one_document_and_the_answer_in_a_reply_is_searchable() {
    let list = start_list(LATE_REPLY).await;
    let upstream = start_embedding_upstream().await;
    let registry = registry_pointed_at(&upstream.uri());
    let pool = db::open(std::path::Path::new(":memory:")).await.unwrap();
    let data_dir = tempdir().unwrap();
    let indexer = indexer(&pool, Arc::clone(&registry), data_dir.path());

    let (collection, r) = seed(&pool, hyperkitty_source(&list.uri())).await;
    indexer.index_ref(r.id).await.unwrap();

    let after = rag_db::find_ref_by_id(&pool, r.id).await.unwrap().unwrap();
    assert_eq!(
        after.status,
        rag_db::CollectionStatus::Ready,
        "last_error: {:?}",
        after.last_error
    );

    let store = indexer
        .collection_store(after.id, &after.data_uuid)
        .await
        .unwrap();
    let files = rag_db::list_files_for_collection(&store, collection.id)
        .await
        .unwrap();
    let mut paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    paths.sort();
    assert_eq!(
        paths.len(),
        2,
        "four messages, two conversations, two documents: {paths:?}"
    );
    assert!(
        paths
            .iter()
            .all(|p| p.starts_with("2026/09/") && p.ends_with(".md")),
        "documents are filed by date: {paths:?}"
    );

    // The payoff: "heartbeat grace" appears only in a reply. Retrieving it
    // means the reply was indexed together with the question it answers.
    let query = "how do I stop the daemons flapping";
    let query_vec = embeddings::embed(
        &reqwest::Client::new(),
        &registry,
        "embed-test",
        &["heartbeat".to_string()],
    )
    .await
    .unwrap()
    .pop()
    .unwrap();
    let hits = search_chunks(&indexer, &after, query, &query_vec, 3, None)
        .await
        .unwrap();
    assert!(!hits.is_empty(), "expected at least one hit");
    assert!(
        hits[0].0.file_path.contains("osd-flapping-after-upgrade"),
        "the flapping thread should rank first, got {}",
        hits[0].0.file_path
    );
    assert!(
        hits.iter().any(|h| h.0.content.contains("heartbeat grace")),
        "the answer text is in the index"
    );
    assert!(
        hits[0]
            .0
            .web_url
            .as_deref()
            .is_some_and(|u| u.contains("/message/")),
        "a hit cites the archive's own permalink: {:?}",
        hits[0].0.web_url
    );

    // Quoting is stripped, so the question exists once and not once per reply.
    let occurrences: usize = hits
        .iter()
        .map(|h| {
            h.0.content
                .matches("the daemons flap every few minutes")
                .count()
        })
        .sum();
    assert!(
        occurrences <= 1,
        "the quoted copy of the question was indexed too ({occurrences} copies)"
    );
}

/// Read this one as the answer to "what does the second sync cost?".
#[tokio::test]
async fn a_second_sync_fetches_only_the_new_days_and_moves_only_that_thread() {
    let upstream = start_embedding_upstream().await;
    let registry = registry_pointed_at(&upstream.uri());
    let pool = db::open(std::path::Path::new(":memory:")).await.unwrap();
    let data_dir = tempdir().unwrap();
    let indexer = indexer(&pool, Arc::clone(&registry), data_dir.path());

    let list = start_list(LATE_REPLY).await;
    let (collection, r) = seed(&pool, hyperkitty_source(&list.uri())).await;
    indexer.index_ref(r.id).await.unwrap();
    assert_eq!(
        request_split(&list).await,
        (1, 0),
        "the first sync has nothing to build on, so it takes the whole export"
    );

    let first = rag_db::find_ref_by_id(&pool, r.id).await.unwrap().unwrap();
    let store = indexer
        .collection_store(first.id, &first.data_uuid)
        .await
        .unwrap();
    let versions_before: HashMap<String, Option<String>> =
        rag_db::list_files_for_collection(&store, collection.id)
            .await
            .unwrap()
            .into_iter()
            .map(|f| (f.path, f.source_version))
            .collect();

    // Same list, same URL, one reply later.
    indexer.index_ref(r.id).await.unwrap();
    assert_eq!(
        request_split(&list).await,
        (1, 1),
        "the second sync takes a date window — the 57 MB export is downloaded once, not daily"
    );

    let second = rag_db::find_ref_by_id(&pool, r.id).await.unwrap().unwrap();
    assert_eq!(
        second.status,
        rag_db::CollectionStatus::Ready,
        "last_error: {:?}",
        second.last_error
    );
    let store = indexer
        .collection_store(second.id, &second.data_uuid)
        .await
        .unwrap();
    let after: Vec<rag_db::IndexedFile> = rag_db::list_files_for_collection(&store, collection.id)
        .await
        .unwrap();
    let versions_after: HashMap<String, Option<String>> = after
        .iter()
        .cloned()
        .map(|f| (f.path, f.source_version))
        .collect();

    assert_eq!(
        versions_after.len(),
        versions_before.len(),
        "a reply joins a thread, it does not create a document"
    );
    let moved: Vec<&String> = versions_after
        .iter()
        .filter(|(path, v)| versions_before.get(*path) != Some(*v))
        .map(|(path, _)| path)
        .collect();
    assert_eq!(moved.len(), 1, "exactly one thread changed: {moved:?}");
    assert!(
        moved[0].contains("osd-flapping-after-upgrade"),
        "and it is the one that gained the reply: {moved:?}"
    );
    assert_eq!(
        archive_files(&data_dir.path().join("source-cache")).len(),
        2,
        "the export and the window sit side by side, decoded as one stream"
    );

    // The window's message really is in the corpus — the point of fetching it.
    let query_vec = embeddings::embed(
        &reqwest::Client::new(),
        &registry,
        "embed-test",
        &["heartbeat".to_string()],
    )
    .await
    .unwrap()
    .pop()
    .unwrap();
    let hits = search_chunks(&indexer, &second, "did it help", &query_vec, 3, None)
        .await
        .unwrap();
    assert!(
        hits.iter().any(|h| h.0.content.contains("settled it")),
        "the reply that only the date window carried is searchable"
    );
}

/// The checkbox has to mean what it says: with the local copy off, every sync
/// pays for the whole export again.
#[tokio::test]
async fn turning_the_local_copy_off_downloads_the_whole_export_every_time() {
    let upstream = start_embedding_upstream().await;
    let pool = db::open(std::path::Path::new(":memory:")).await.unwrap();
    let data_dir = tempdir().unwrap();
    let indexer = indexer(&pool, registry_pointed_at(&upstream.uri()), data_dir.path());

    let list = start_list(LATE_REPLY).await;
    let (_, r) = seed(&pool, source_with_cache(&list.uri(), false)).await;
    indexer.index_ref(r.id).await.unwrap();
    indexer.index_ref(r.id).await.unwrap();

    assert_eq!(
        request_split(&list).await,
        (2, 0),
        "no cache means no window to ask for, and the export twice"
    );
    let after = rag_db::find_ref_by_id(&pool, r.id).await.unwrap().unwrap();
    assert_eq!(after.status, rag_db::CollectionStatus::Ready);
    assert!(
        archive_files(&data_dir.path().join("source-cache")).is_empty(),
        "and not a byte of the archive was left on disk"
    );
}

#[tokio::test]
async fn a_list_url_that_does_not_exist_fails_the_ref_with_the_url_in_it() {
    let list = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&list)
        .await;
    let upstream = start_embedding_upstream().await;
    let pool = db::open(std::path::Path::new(":memory:")).await.unwrap();
    let data_dir = tempdir().unwrap();
    let indexer = indexer(&pool, registry_pointed_at(&upstream.uri()), data_dir.path());

    let (_, r) = seed(&pool, hyperkitty_source(&list.uri())).await;
    let _ = indexer.index_ref(r.id).await;

    let after = rag_db::find_ref_by_id(&pool, r.id).await.unwrap().unwrap();
    assert_eq!(
        after.status,
        rag_db::CollectionStatus::Error,
        "a 404 archive must not read as an empty list"
    );
    let msg = after.last_error.unwrap_or_default();
    assert!(
        msg.contains("export/users@example.com.mbox.gz"),
        "the message names the URL that was tried: {msg}"
    );
}
