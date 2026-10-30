// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Who may read a shared conversation: the link (within the policy's level
//! limit, re-checked on every read), listed users and members of listed
//! groups; and a fork keeping the source's level.

use std::sync::Arc;

use crate::common;
use aiplane::rama_server::RamaState;
use aiplane::rama_server::router::router;
use aiplane_core::server::classification as core;
use aiplane_core::server::db::users;
use common::Service as _;
use rama::http::body::util::BodyExt;
use rama::http::{Body, Method, Request, StatusCode, header};
use serde_json::{Value, json};
use session_core::db as chat;

struct World {
    state: Arc<RamaState>,
    alice: String,
    bob: String,
    carol: String,
    dave: String,
    session: String,
}

async fn seed_user(state: &RamaState, id: &str, name: &str, roles: &[&str]) -> String {
    let cookie = common::seed_session(state, id, &format!("{id}@example.com")).await;
    users::upsert(
        &state.db,
        &users::User {
            id: id.into(),
            email: format!("{id}@example.com"),
            name: Some(name.into()),
            roles: roles.iter().map(|r| (*r).to_owned()).collect(),
            created_at: jiff::Timestamp::now(),
            updated_at: jiff::Timestamp::now(),
            timezone: None,
            speech_voice: None,
        },
    )
    .await
    .unwrap();
    cookie
}

/// Alice owns a conversation; bob is a listed user, carol belongs to the group
/// `admin`, dave is a stranger. With `levels`, the scheme is public < internal
/// < confidential and a link may be shared up to `internal`.
async fn world(levels: bool) -> World {
    let state = common::state_with_admin_rbac("http://unused.invalid").await;
    let alice = seed_user(&state, "alice", "Alice Owner", &[]).await;
    let bob = seed_user(&state, "bob", "Bob Reader", &[]).await;
    let carol = seed_user(&state, "carol", "Carol Member", &["admin"]).await;
    let dave = seed_user(&state, "dave", "Dave Stranger", &[]).await;
    aiplane_core::server::db::gateway_groups::upsert_group(&state.db, "admin", "", true, false)
        .await
        .unwrap();
    if levels {
        sqlx::raw_sql(
            "INSERT INTO classification_levels (id, rank, name, color) VALUES
                 ('internal', 1, 'Internal', 'info'),
                 ('confidential', 2, 'Confidential', 'warning');",
        )
        .execute(&state.db)
        .await
        .unwrap();
        aiplane_core::server::db::app_settings::set(
            &state.db,
            "settings.classification.link_share_max_level",
            "internal",
        )
        .await
        .unwrap();
        state.reload_settings().await;
        state.reload_scheme().await;
    }
    let session = chat::create_session(&state.db, "alice").await.unwrap().id;
    World {
        state: Arc::new(state),
        alice,
        bob,
        carol,
        dave,
        session,
    }
}

async fn call(
    w: &World,
    cookie: &str,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("cookie", format!("id={cookie}"));
    let body = match body {
        Some(body) => {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            Body::from(serde_json::to_vec(&body).unwrap())
        }
        None => Body::empty(),
    };
    let resp = router(w.state.clone())
        .serve(builder.body(body).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let is_json = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains("json"));
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let value = if is_json {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    (status, value)
}

fn session_uri(w: &World, tail: &str) -> String {
    format!("/api/v0/chat/sessions/{}{tail}", w.session)
}

async fn read_status(w: &World, cookie: &str) -> StatusCode {
    call(w, cookie, Method::GET, &session_uri(w, ""), None)
        .await
        .0
}

async fn put_shares(w: &World, body: Value) -> (StatusCode, Value) {
    call(
        w,
        &w.alice,
        Method::PUT,
        &session_uri(w, "/shares"),
        Some(body),
    )
    .await
}

#[tokio::test]
async fn with_one_level_a_link_works_as_it_always_did() {
    let w = world(false).await;
    assert_eq!(read_status(&w, &w.dave).await, StatusCode::NOT_FOUND);

    let (status, shares) = put_shares(&w, json!({"link": true, "subjects": []})).await;

    assert_eq!(status, StatusCode::OK, "{shares}");
    assert_eq!(shares["link"], true);
    assert_eq!(shares["link_allowed"], true);
    assert_eq!(read_status(&w, &w.dave).await, StatusCode::OK);
}

#[tokio::test]
async fn raising_a_link_shared_conversation_above_the_limit_stops_the_link_at_once() {
    let w = world(true).await;
    put_shares(&w, json!({"link": true, "subjects": []})).await;
    assert_eq!(read_status(&w, &w.dave).await, StatusCode::OK);
    let events = session_uri(&w, "/events");
    assert_ne!(
        call(&w, &w.dave, Method::GET, &events, None).await.0,
        StatusCode::NOT_FOUND
    );

    core::set_level(&w.state.db, &w.session, "confidential")
        .await
        .unwrap();

    assert_eq!(read_status(&w, &w.dave).await, StatusCode::NOT_FOUND);
    assert_eq!(
        call(&w, &w.dave, Method::GET, &events, None).await.0,
        StatusCode::NOT_FOUND,
        "the live stream is checked the same way"
    );
    let (_, shares) = call(&w, &w.alice, Method::GET, &session_uri(&w, "/shares"), None).await;
    assert_eq!(shares["link_allowed"], false);
    assert_eq!(shares["link"], false, "the link is shown as closed");
    assert_eq!(read_status(&w, &w.alice).await, StatusCode::OK);
}

#[tokio::test]
async fn a_link_above_the_limit_is_refused_with_409() {
    let w = world(true).await;
    core::set_level(&w.state.db, &w.session, "confidential")
        .await
        .unwrap();

    let (status, error) = put_shares(&w, json!({"link": true, "subjects": []})).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error["error"]["code"], "link_sharing_above_limit");

    let (status, _) = call(
        &w,
        &w.alice,
        Method::POST,
        &session_uri(&w, "/share"),
        Some(json!({"shared": true})),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "the single toggle follows the same rule"
    );
    assert_eq!(read_status(&w, &w.dave).await, StatusCode::NOT_FOUND);

    let (status, _) = put_shares(&w, json!({"link": false, "subjects": []})).await;
    assert_eq!(status, StatusCode::OK, "people and groups remain possible");
}

#[tokio::test]
async fn listed_users_and_members_of_listed_groups_read_and_others_do_not() {
    let w = world(true).await;
    core::set_level(&w.state.db, &w.session, "confidential")
        .await
        .unwrap();

    let (status, shares) = put_shares(
        &w,
        json!({"link": false, "subjects": [
            {"kind": "user", "id": "bob"},
            {"kind": "group", "id": "admin"}
        ]}),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{shares}");
    assert_eq!(
        shares["subjects"],
        json!([
            {"kind": "group", "id": "admin", "name": "admin"},
            {"kind": "user", "id": "bob", "name": "Bob Reader"}
        ])
    );
    assert_eq!(read_status(&w, &w.bob).await, StatusCode::OK);
    assert_eq!(read_status(&w, &w.carol).await, StatusCode::OK);
    assert_eq!(read_status(&w, &w.dave).await, StatusCode::NOT_FOUND);

    put_shares(&w, json!({"link": false, "subjects": []})).await;
    assert_eq!(read_status(&w, &w.bob).await, StatusCode::NOT_FOUND);
    assert_eq!(read_status(&w, &w.carol).await, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn only_the_owner_manages_shares_and_unknown_subjects_are_refused() {
    let w = world(true).await;
    let (status, _) = call(&w, &w.bob, Method::GET, &session_uri(&w, "/shares"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(
        &w,
        &w.dave,
        Method::PUT,
        &session_uri(&w, "/shares"),
        Some(json!({"link": true, "subjects": []})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(read_status(&w, &w.dave).await, StatusCode::NOT_FOUND);

    let (status, _) = put_shares(
        &w,
        json!({"link": false, "subjects": [{"kind": "user", "id": "nobody"}]}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = put_shares(
        &w,
        json!({"link": false, "subjects": [{"kind": "group", "id": "nogroup"}]}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn the_owner_searches_users_and_groups_to_share_with() {
    let w = world(true).await;
    let uri = |q: &str| session_uri(&w, &format!("/share-subjects?q={q}"));

    let (status, found) = call(&w, &w.alice, Method::GET, &uri("bo"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(found["users"].as_array().unwrap().len(), 1);
    assert_eq!(found["users"][0]["id"], "bob");
    assert_eq!(found["truncated"], false);

    let (_, found) = call(&w, &w.alice, Method::GET, &uri("adm"), None).await;
    assert_eq!(found["groups"], json!(["admin"]));

    let (_, found) = call(&w, &w.alice, Method::GET, &uri("b"), None).await;
    assert_eq!(found["users"], json!([]), "a search, never a roster");

    let (status, _) = call(&w, &w.dave, Method::GET, &uri("bo"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_fork_keeps_the_level_starts_unshared_and_needs_read_access() {
    let w = world(true).await;
    core::set_level(&w.state.db, &w.session, "confidential")
        .await
        .unwrap();
    chat::create_user_turn(&w.state.db, &w.session, "turn-1", "hello")
        .await
        .unwrap();
    put_shares(
        &w,
        json!({"link": false, "subjects": [{"kind": "user", "id": "bob"}]}),
    )
    .await;

    let (status, _) = call(&w, &w.dave, Method::POST, &session_uri(&w, "/fork"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, forked) = call(&w, &w.bob, Method::POST, &session_uri(&w, "/fork"), None).await;
    assert_eq!(status, StatusCode::CREATED, "{forked}");
    let copy = forked["id"].as_str().unwrap();
    assert_eq!(
        core::level_id(&w.state.db, copy).await.unwrap().as_deref(),
        Some("confidential")
    );
    let (_, detail) = call(
        &w,
        &w.bob,
        Method::GET,
        &format!("/api/v0/chat/sessions/{copy}"),
        None,
    )
    .await;
    assert_eq!(detail["session"]["shared"], false);
    assert_eq!(detail["classification"]["cause"], "fork");
    let (_, shares) = call(
        &w,
        &w.bob,
        Method::GET,
        &format!("/api/v0/chat/sessions/{copy}/shares"),
        None,
    )
    .await;
    assert_eq!(shares["subjects"], json!([]));
    assert_eq!(shares["link"], false);
}

#[tokio::test]
async fn an_attachment_turn_follows_the_conversations_readers() {
    use aiplane_runtime::chat_sharing::{Reader, turn_readable};
    let w = world(true).await;
    chat::create_user_turn(&w.state.db, &w.session, "turn-a", "hello")
        .await
        .unwrap();
    put_shares(
        &w,
        json!({"link": false, "subjects": [{"kind": "user", "id": "bob"}]}),
    )
    .await;
    let reader = async |id: &str| {
        let user = users::find_by_id(&w.state.db, id).await.unwrap().unwrap();
        Reader::new(&w.state, &user)
    };

    assert!(
        turn_readable(&w.state, &reader("alice").await, "turn-a")
            .await
            .unwrap()
    );
    assert!(
        turn_readable(&w.state, &reader("bob").await, "turn-a")
            .await
            .unwrap()
    );
    assert!(
        !turn_readable(&w.state, &reader("dave").await, "turn-a")
            .await
            .unwrap()
    );
    assert!(
        !turn_readable(&w.state, &reader("alice").await, "missing")
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn a_delegated_child_conversation_stays_private_to_its_owner() {
    let w = world(true).await;
    sqlx::query("UPDATE chat_sessions SET parent_turn_id = 'parent' WHERE id = ?")
        .bind(&w.session)
        .execute(&w.state.db)
        .await
        .unwrap();
    sqlx::query("UPDATE chat_sessions SET shared = 1 WHERE id = ?")
        .bind(&w.session)
        .execute(&w.state.db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO chat_session_shares (session_id, subject_kind, subject_id) VALUES (?, 'user', 'bob')")
        .bind(&w.session)
        .execute(&w.state.db)
        .await
        .unwrap();

    assert_eq!(read_status(&w, &w.dave).await, StatusCode::NOT_FOUND);
    assert_eq!(read_status(&w, &w.bob).await, StatusCode::NOT_FOUND);
    assert_eq!(read_status(&w, &w.alice).await, StatusCode::OK);
}
