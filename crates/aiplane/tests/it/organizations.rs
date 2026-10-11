// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use super::agents::{Fx, fixture, person};
use rama::http::{Method, StatusCode};
use serde_json::{Value, json};

async fn oauth_organization() -> (Fx, wiremock::MockServer, Value, String) {
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };
    let fx = fixture().await;
    let provider = MockServer::start().await;
    Mock::given(method("POST")).and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token":"organization-fixture-access","refresh_token":"organization-fixture-refresh",
            "token_type":"Bearer","expires_in":3600,"scope":"workspace.read"
        }))).mount(&provider).await;
    aiplane_core::server::db::gateway_groups::set_can_manage_dots(&fx.state.db, "managers", false)
        .await
        .unwrap();
    aiplane_core::server::db::gateway_groups::set_tools_for_group(
        &fx.state.db,
        "managers",
        &["mcp__workspace".into()],
    )
    .await
    .unwrap();
    fx.state.reload_rbac().await;
    sqlx::query("INSERT INTO mcp_catalog_connectors(key,name,url,auth,scope,use_dcr,client_id,authorize_url,token_url,enabled,created_at,updated_at) VALUES('workspace','Actual Workspace',?,'oauth2','per_user',0,'client',?,?,1,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')")
        .bind(format!("{}/mcp", provider.uri())).bind(format!("{}/authorize", provider.uri())).bind(format!("{}/token", provider.uri())).execute(&fx.state.db).await.unwrap();
    let org = organization(&fx, "Acme OAuth", &["managers"]).await;
    let (status, created) = create_dot(&fx, &fx.alice, &org, "Harald").await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let dot = id(&created["agent"]).to_owned();
    (fx, provider, org, dot)
}

fn connection_row(view: &Value) -> &Value {
    view["connections"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["key"] == "workspace")
        .unwrap()
}

async fn oauth_callback(
    state: aiplane::rama_server::RamaState,
    cookie: &str,
    oauth_state: &str,
) -> (StatusCode, Option<String>) {
    use rama::{
        Service,
        http::{Body, Request, header},
    };
    let request = Request::builder()
        .uri(format!(
            "/integrations/callback?state={oauth_state}&code=accepted"
        ))
        .header(header::COOKIE, format!("id={cookie}"))
        .body(Body::empty())
        .unwrap();
    let response = crate::common::app(state).serve(request).await.unwrap();
    let status = response.status();
    let location = response
        .headers()
        .get(header::LOCATION)
        .map(|value| value.to_str().unwrap().to_owned());
    crate::common::read_body(response).await;
    (status, location)
}

async fn start_organization_oauth(fx: &Fx, dot: &str) -> String {
    let (status, body) = fx
        .post(
            &fx.alice,
            &format!("/api/v0/dots/{dot}/connections/workspace/connect"),
            json!({"connection_id":null,"connection_generation":null}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let url = reqwest::Url::parse(body["authorization_url"].as_str().unwrap()).unwrap();
    assert!(url.query_pairs().any(|(key, _)| key == "code_challenge"));
    url.query_pairs()
        .find(|(key, _)| key == "state")
        .unwrap()
        .1
        .into_owned()
}

async fn seed_organization_credential(
    fx: &Fx,
    owner: aiplane_core::server::db::mcp_connections::ConnectionOwner,
) -> aiplane_core::server::db::mcp_connections::ConnectionIdentity {
    use aiplane_core::server::db::{
        mcp_catalog,
        mcp_connections::{self, NewConnection},
    };
    let catalog = mcp_catalog::get(&fx.state.db, "workspace")
        .await
        .unwrap()
        .unwrap();
    mcp_connections::upsert_connection(
        &fx.state.db,
        NewConnection {
            owner,
            connector_key: catalog.key,
            catalog_generation: catalog.dispatch_generation,
            access_token_ct: vec![0, 1, 255],
            access_token_nonce: vec![2],
            refresh_token_ct: None,
            refresh_token_nonce: None,
            token_expires_at: None,
            scopes: vec!["workspace.read".into()],
            dcr_client_id: None,
            dcr_client_secret_ct: None,
            dcr_client_secret_nonce: None,
            token_url: catalog.token_url,
        },
    )
    .await
    .unwrap()
    .unwrap()
}

async fn deny_organization_connections(fx: &Fx, cookie: &str, dot: &str) {
    let base = format!("/api/v0/dots/{dot}/connections");
    for (method, suffix, body) in [
        (Method::GET, "", None),
        (
            Method::PUT,
            "/workspace",
            Some(
                json!({"mode":"principal","expected_binding_generation":null,"connection_id":null,"connection_generation":null}),
            ),
        ),
        (
            Method::POST,
            "/workspace/connect",
            Some(json!({"connection_id":null,"connection_generation":null})),
        ),
        (
            Method::DELETE,
            "/workspace",
            Some(json!({"connection_id":null,"connection_generation":null})),
        ),
    ] {
        let (status, body) = fx
            .send(Some(cookie), method, &format!("{base}{suffix}"), body)
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
        assert!(body.get("connections").is_none());
    }
}

#[tokio::test]
async fn organization_oauth_inactive_member_writer_uses_explicit_principal_identity_without_personal_permission()
 {
    use aiplane_core::server::db::mcp_connections::{self, ConnectionOwner};
    let (fx, _provider, org, dot) = oauth_organization().await;
    let (_, me) = fx.get(&fx.alice, "/api/v0/me").await;
    assert_eq!(me["can_manage_agents"], true);
    assert_eq!(me["can_manage_dots"], false);
    let user = seed_organization_credential(&fx, ConnectionOwner::User("alice".into())).await;
    let (status, clara) = create_dot(&fx, &fx.alice, &org, "Clara").await;
    assert_eq!(status, StatusCode::CREATED);
    let other_owner = ConnectionOwner::Principal(id(&clara["agent"]).to_owned());
    let other = seed_organization_credential(&fx, other_owner.clone()).await;
    let (status, _) = fx
        .post(
            &fx.alice,
            &format!("/api/v0/organizations/{}/dots/{dot}/active", id(&org)),
            json!({"active":false}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let base = format!("/api/v0/dots/{dot}/connections");
    let (status, initial) = fx.get(&fx.alice, &base).await;
    assert_eq!(status, StatusCode::OK, "{initial}");
    assert_eq!(initial["dot"]["name"], "Harald");
    assert_eq!(initial["dot"]["active"], false);
    let row = connection_row(&initial);
    assert_eq!(row["supported_modes"], json!(["principal"]));
    assert_eq!(row["user_connection"], Value::Null);
    assert_eq!(row["principal_connection"], Value::Null);
    assert_eq!(row["can_select"], true);
    assert_eq!(
        row["selected"],
        json!({"mode":"principal","connection_id":null,"connection_generation":null,"binding_generation":null})
    );
    let oauth_state = start_organization_oauth(&fx, &dot).await;
    let (status, location) = oauth_callback(fx.state.clone(), &fx.alice, &oauth_state).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(
        location.as_deref(),
        Some(format!("/agents/{dot}?tab=setup").as_str())
    );
    let (status, connected) = fx.get(&fx.alice, &base).await;
    assert_eq!(status, StatusCode::OK, "{connected}");
    let row = connection_row(&connected);
    assert_eq!(row["principal_connection"]["connected"], true);
    assert_eq!(row["selected"]["connection_id"], Value::Null);
    assert_eq!(row["selected"]["binding_generation"], Value::Null);
    for (mode, connection_id, generation) in [
        (
            "user",
            user.connection_id.as_str(),
            user.generation.as_str(),
        ),
        (
            "principal",
            other.connection_id.as_str(),
            other.generation.as_str(),
        ),
    ] {
        let (status, _) = fx.send(Some(&fx.alice), Method::PUT, &format!("{base}/workspace"), Some(json!({
            "mode":mode,"expected_binding_generation":null,"connection_id":connection_id,"connection_generation":generation
        }))).await;
        assert_eq!(status, StatusCode::CONFLICT);
    }
    let selection = json!({"mode":"principal","expected_binding_generation":null,
        "connection_id":row["principal_connection"]["id"],"connection_generation":row["principal_connection"]["generation"]});
    let (status, selected) = fx
        .send(
            Some(&fx.alice),
            Method::PUT,
            &format!("{base}/workspace"),
            Some(selection),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{selected}");
    let selected_row = connection_row(&selected);
    assert_eq!(
        selected_row["selected"]["connection_id"],
        row["principal_connection"]["id"]
    );
    let (status, cleared) = fx.send(Some(&fx.alice), Method::DELETE, &format!("{base}/workspace"), Some(json!({
        "connection_id":row["principal_connection"]["id"],"connection_generation":row["principal_connection"]["generation"]
    }))).await;
    assert_eq!(status, StatusCode::OK, "{cleared}");
    assert_eq!(connection_row(&cleared)["user_connection"], Value::Null);
    assert_eq!(connection_row(&cleared)["selected"]["mode"], "principal");
    assert_eq!(
        connection_row(&cleared)["selected"]["connection_id"],
        Value::Null
    );
    assert_eq!(
        connection_row(&cleared)["selected"]["binding_generation"],
        selected_row["selected"]["binding_generation"]
    );
    assert_eq!(
        mcp_connections::get_connection(
            &fx.state.db,
            &ConnectionOwner::User("alice".into()),
            "workspace"
        )
        .await
        .unwrap()
        .unwrap()
        .id,
        user.connection_id
    );
    assert_eq!(
        mcp_connections::get_connection(&fx.state.db, &other_owner, "workspace")
            .await
            .unwrap()
            .unwrap()
            .id,
        other.connection_id
    );
}

#[tokio::test]
async fn organization_oauth_foreign_read_respond_and_nonmember_admin_never_obtain_credential_versions()
 {
    use aiplane_agents::db::agents;
    let (fx, _provider, _org, dot) = oauth_organization().await;
    for cookie in [&fx.root, &fx.bob, &fx.plain] {
        deny_organization_connections(&fx, cookie, &dot).await;
    }
    for access in ["read", "respond"] {
        let access = if access == "read" {
            agents::Access::Read
        } else {
            agents::Access::Respond
        };
        agents::set_share(
            &fx.state.db,
            &dot,
            agents::SubjectKind::User,
            "bob",
            access,
            "alice",
        )
        .await
        .unwrap();
        deny_organization_connections(&fx, &fx.bob, &dot).await;
    }
}

#[tokio::test]
async fn organization_oauth_current_membership_role_mapping_and_write_share_revocation_apply_without_rbac_reload()
 {
    use aiplane_core::server::db::gateway_groups;
    for change in ["membership", "mapping", "role", "share"] {
        let (fx, _provider, org, dot) = oauth_organization().await;
        let (status, body) = fx
            .get(&fx.alice, &format!("/api/v0/dots/{dot}/connections"))
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        match change {
            "membership" => {
                assert_eq!(edit(&fx, &org, "Acme OAuth", &[]).await.0, StatusCode::OK);
            }
            "mapping" => gateway_groups::set_mappings_for_group(&fx.state.db, "managers", &[])
                .await
                .unwrap(),
            "role" => {
                person(&fx.state, "alice", &[]).await;
            }
            "share" => {
                use aiplane_agents::db::agents::{self, Access, SubjectKind};
                agents::set_share(
                    &fx.state.db,
                    &dot,
                    SubjectKind::User,
                    "bob",
                    Access::Write,
                    "alice",
                )
                .await
                .unwrap();
                assert_eq!(
                    agents::remove_share(&fx.state.db, &dot, SubjectKind::User, "alice", "bob")
                        .await
                        .unwrap(),
                    agents::ShareChange::Changed
                );
            }
            _ => unreachable!(),
        }
        deny_organization_connections(&fx, &fx.alice, &dot).await;
    }
}

#[tokio::test]
async fn organization_oauth_retained_local_credential_can_be_erased_without_catalogue_access_or_user_projection()
 {
    use aiplane_core::server::db::mcp_connections::{self, ConnectionOwner};
    for restriction in ["disabled", "forbidden", "deleted"] {
        let (fx, _provider, _org, dot) = oauth_organization().await;
        let owner = ConnectionOwner::Principal(dot.clone());
        let own = seed_organization_credential(&fx, owner.clone()).await;
        let user = seed_organization_credential(&fx, ConnectionOwner::User("alice".into())).await;
        match restriction {
            "disabled" => {
                sqlx::query("UPDATE mcp_catalog_connectors SET enabled=0 WHERE key='workspace'")
                    .execute(&fx.state.db)
                    .await
                    .unwrap();
            }
            "forbidden" => {
                sqlx::query("UPDATE mcp_catalog_connectors SET allowed_groups='[\"admins\"]' WHERE key='workspace'").execute(&fx.state.db).await.unwrap();
            }
            "deleted" => {
                sqlx::query("DELETE FROM mcp_catalog_connectors WHERE key='workspace'")
                    .execute(&fx.state.db)
                    .await
                    .unwrap();
            }
            _ => unreachable!(),
        }
        let base = format!("/api/v0/dots/{dot}/connections");
        let (status, view) = fx.get(&fx.alice, &base).await;
        assert_eq!(status, StatusCode::OK, "{view}");
        let row = connection_row(&view);
        assert_eq!(row["connector"], Value::Null);
        assert_eq!(row["user_connection"], Value::Null);
        assert_eq!(row["supported_modes"], json!([]));
        assert_eq!(row["can_connect"], false);
        assert_eq!(row["can_disconnect"], true);
        assert_eq!(row["principal_connection"]["id"], own.connection_id);
        assert_eq!(row["principal_connection"]["generation"], own.generation);
        assert_eq!(row["selected"]["binding_generation"], Value::Null);
        let (status, _) = fx
            .send(
                Some(&fx.alice),
                Method::DELETE,
                &format!("{base}/workspace"),
                Some(json!({
                    "connection_id":own.connection_id,"connection_generation":own.generation
                })),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            mcp_connections::get_connection(&fx.state.db, &owner, "workspace")
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            mcp_connections::get_connection(
                &fx.state.db,
                &ConnectionOwner::User("alice".into()),
                "workspace"
            )
            .await
            .unwrap()
            .unwrap()
            .id,
            user.connection_id
        );
    }
}

#[tokio::test]
async fn organization_oauth_callback_rechecks_current_membership_before_exchange_and_consumes_state_once()
 {
    use aiplane_core::server::db::mcp_connections::{self, ConnectionOwner};
    let (fx, provider, org, dot) = oauth_organization().await;
    let oauth_state = start_organization_oauth(&fx, &dot).await;
    assert_eq!(edit(&fx, &org, "Acme OAuth", &[]).await.0, StatusCode::OK);
    assert_eq!(
        oauth_callback(fx.state.clone(), &fx.alice, &oauth_state)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert!(provider.received_requests().await.unwrap().is_empty());
    assert_eq!(
        oauth_callback(fx.state.clone(), &fx.alice, &oauth_state)
            .await
            .0,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert!(
        mcp_connections::get_connection(
            &fx.state.db,
            &ConnectionOwner::Principal(dot),
            "workspace"
        )
        .await
        .unwrap()
        .is_none()
    );
}

#[tokio::test]
async fn organization_oauth_freshly_mapped_catalogue_permission_is_admitted_without_rbac_reload() {
    let (fx, _provider, _org, dot) = oauth_organization().await;
    sqlx::query(
        "UPDATE mcp_catalog_connectors SET allowed_groups='[\"support\"]' WHERE key='workspace'",
    )
    .execute(&fx.state.db)
    .await
    .unwrap();
    aiplane_core::server::db::gateway_groups::set_mappings_for_group(
        &fx.state.db,
        "support",
        &["managers".into()],
    )
    .await
    .unwrap();
    let (status, inventory) = fx
        .get(&fx.alice, &format!("/api/v0/dots/{dot}/connections"))
        .await;
    assert_eq!(status, StatusCode::OK, "{inventory}");
    assert_eq!(connection_row(&inventory)["can_connect"], true);
    let oauth_state = start_organization_oauth(&fx, &dot).await;
    assert_eq!(
        oauth_callback(fx.state.clone(), &fx.alice, &oauth_state)
            .await
            .0,
        StatusCode::SEE_OTHER
    );
}

#[tokio::test]
async fn organization_oauth_callback_membership_revoked_during_exchange_never_saves_tokens() {
    use std::{sync::Arc, time::Duration};
    use wiremock::{
        Mock, ResponseTemplate,
        matchers::{method, path},
    };
    let (fx, provider, org, dot) = oauth_organization().await;
    let oauth_state = start_organization_oauth(&fx, &dot).await;
    let reached = Arc::new(tokio::sync::Notify::new());
    let exchanged = reached.clone();
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(move |_: &wiremock::Request| {
            exchanged.notify_one();
            ResponseTemplate::new(200).set_delay(Duration::from_millis(300)).set_body_json(json!({
            "access_token":"organization-fixture-access","token_type":"Bearer","expires_in":3600
        }))
        })
        .with_priority(1)
        .mount(&provider)
        .await;
    let state = fx.state.clone();
    let cookie = fx.alice.clone();
    let worker = tokio::spawn(async move { oauth_callback(state, &cookie, &oauth_state).await.0 });
    tokio::time::timeout(Duration::from_secs(3), reached.notified())
        .await
        .unwrap();
    assert_eq!(edit(&fx, &org, "Acme OAuth", &[]).await.0, StatusCode::OK);
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), worker)
            .await
            .unwrap()
            .unwrap(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(provider.received_requests().await.unwrap().len(), 1);
    assert!(
        aiplane_core::server::db::mcp_connections::get_connection(
            &fx.state.db,
            &aiplane_core::server::db::mcp_connections::ConnectionOwner::Principal(dot),
            "workspace"
        )
        .await
        .unwrap()
        .is_none()
    );
}

#[tokio::test]
async fn organization_oauth_deleted_initiator_cancels_pending_without_erasing_or_transferring_org_credentials()
 {
    use aiplane_agents::db::agents::{self, Access, SubjectKind};
    use aiplane_core::server::db::mcp_connections::{self, ConnectionOwner};
    let (fx, provider, _org, dot) = oauth_organization().await;
    let owner = ConnectionOwner::Principal(dot.clone());
    let own = seed_organization_credential(&fx, owner.clone()).await;
    agents::set_share(
        &fx.state.db,
        &dot,
        SubjectKind::User,
        "bob",
        Access::Write,
        "alice",
    )
    .await
    .unwrap();
    let base = format!("/api/v0/dots/{dot}/connections");
    let (status, selected) = fx.send(Some(&fx.alice), Method::PUT, &format!("{base}/workspace"), Some(json!({
        "mode":"principal","expected_binding_generation":null,"connection_id":own.connection_id,"connection_generation":own.generation
    }))).await;
    assert_eq!(status, StatusCode::OK, "{selected}");
    let binding = connection_row(&selected)["selected"].clone();
    let (status, started) = fx
        .post(
            &fx.alice,
            &format!("{base}/workspace/connect"),
            json!({
                "connection_id":own.connection_id,"connection_generation":own.generation
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let url = reqwest::Url::parse(started["authorization_url"].as_str().unwrap()).unwrap();
    let oauth_state = url
        .query_pairs()
        .find(|(key, _)| key == "state")
        .unwrap()
        .1
        .into_owned();
    sqlx::query("DELETE FROM users WHERE id='alice'")
        .execute(&fx.state.db)
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM pending_mcp_oauth WHERE principal_id=?")
            .bind(&dot)
            .fetch_one(&fx.state.db)
            .await
            .unwrap(),
        0
    );
    let (status, location) = oauth_callback(fx.state.clone(), &fx.alice, &oauth_state).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert!(location.is_some_and(|location| location.starts_with("/login?")));
    assert_eq!(
        oauth_callback(fx.state.clone(), &fx.bob, &oauth_state)
            .await
            .0,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert!(provider.received_requests().await.unwrap().is_empty());
    let (status, inventory) = fx.get(&fx.bob, &base).await;
    assert_eq!(status, StatusCode::OK, "{inventory}");
    let row = connection_row(&inventory);
    assert_eq!(row["can_select"], true);
    assert_eq!(row["user_connection"], Value::Null);
    assert_eq!(row["principal_connection"]["id"], own.connection_id);
    assert_eq!(row["principal_connection"]["generation"], own.generation);
    assert_eq!(row["principal_connection"]["authorizing"], false);
    assert_eq!(row["selected"], binding);
    assert_eq!(
        mcp_connections::get_connection(&fx.state.db, &owner, "workspace")
            .await
            .unwrap()
            .unwrap()
            .owner,
        owner
    );
}

async fn organization(fx: &Fx, name: &str, groups: &[&str]) -> Value {
    let (status, body) = fx
        .post(
            &fx.root,
            "/api/v0/admin/organizations",
            json!({
                "name":name,"description":"Actual support organization","member_groups":groups
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["organization"].clone()
}

fn id(row: &Value) -> &str {
    row["id"].as_str().unwrap()
}

async fn edit(fx: &Fx, original: &Value, name: &str, groups: &[&str]) -> (StatusCode, Value) {
    fx.send(
        Some(&fx.root),
        Method::PUT,
        &format!("/api/v0/admin/organizations/{}", id(original)),
        Some(json!({
            "name":name,"description":original["description"],"member_groups":groups,
            "expected_revision":original["revision"]
        })),
    )
    .await
}

async fn create_dot(fx: &Fx, cookie: &str, org: &Value, name: &str) -> (StatusCode, Value) {
    fx.post(
        cookie,
        &format!("/api/v0/organizations/{}/dots", id(org)),
        json!({
            "name":name,"display":"Harald Support","description":"Actual organization assistant"
        }),
    )
    .await
}

#[tokio::test]
async fn foundation_metadata_create_list_and_noop_preserve_actual_groups_and_revision() {
    let fx = fixture().await;
    let created = organization(&fx, "Acme Support", &["managers"]).await;
    assert_eq!(created["name"], "Acme Support");
    assert_eq!(created["description"], "Actual support organization");
    assert_eq!(created["member_groups"], json!(["managers"]));
    assert!(!created["revision"].as_str().unwrap().is_empty());
    let (status, list) = fx.get(&fx.root, "/api/v0/admin/organizations").await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(list["organizations"], json!([created.clone()]));
    let groups = list["groups"].as_array().unwrap();
    assert!(groups.contains(&json!("managers")));
    assert!(groups.contains(&json!("everyone")));
    let (status, saved) = edit(&fx, &created, "Acme Support", &["managers"]).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["organization"], created);
}

#[tokio::test]
async fn foundation_metadata_stale_revision_cannot_overwrite_new_bindings() {
    let fx = fixture().await;
    let created = organization(&fx, "Acme Support", &["managers"]).await;
    let (status, updated) = edit(&fx, &created, "Actual renamed organization", &["everyone"]).await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_ne!(updated["organization"]["revision"], created["revision"]);
    let (status, stale) = edit(&fx, &created, "Acme Support", &["managers"]).await;
    assert_eq!(status, StatusCode::CONFLICT, "{stale}");
    let (status, list) = fx.get(&fx.root, "/api/v0/admin/organizations").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        list["organizations"],
        json!([updated["organization"].clone()])
    );
    let (status, _) = fx.send(Some(&fx.root), Method::PUT, "/api/v0/admin/organizations/missing", Some(json!({
        "name":"Missing","description":"Actual description","member_groups":[],"expected_revision":"missing"
    }))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn foundation_metadata_invalid_groups_and_spoofed_ownership_are_rejected_atomically() {
    let fx = fixture().await;
    for body in [
        json!({"name":"Unknown group","description":"Actual","member_groups":["not-an-installed-group"]}),
        json!({"name":"Duplicate group","description":"Actual","member_groups":["managers","managers"]}),
        json!({"name":"","description":"Actual","member_groups":["managers"]}),
        json!({"name":"Spoofed owner","description":"Actual","member_groups":[],"created_by":"alice"}),
    ] {
        let (status, error) = fx.post(&fx.root, "/api/v0/admin/organizations", body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
    }
    let (_, list) = fx.get(&fx.root, "/api/v0/admin/organizations").await;
    assert_eq!(list["organizations"], json!([]));
    let empty = organization(&fx, "Admin recovery only", &[]).await;
    assert_eq!(empty["member_groups"], json!([]));
    for cookie in [&fx.alice, &fx.plain, &fx.root] {
        let (status, member) = fx.get(cookie, "/api/v0/organizations").await;
        assert_eq!(status, StatusCode::OK, "{member}");
        assert_eq!(member["organizations"], json!([]));
    }
}

#[tokio::test]
async fn foundation_metadata_and_member_routes_require_their_current_session_and_authority() {
    let fx = fixture().await;
    for (method, path) in [
        (Method::GET, "/api/v0/admin/organizations"),
        (Method::POST, "/api/v0/admin/organizations"),
        (Method::PUT, "/api/v0/admin/organizations/missing"),
        (Method::GET, "/api/v0/organizations"),
        (Method::GET, "/api/v0/organizations/missing"),
        (Method::POST, "/api/v0/organizations/missing/dots"),
        (
            Method::POST,
            "/api/v0/organizations/missing/dots/missing/active",
        ),
    ] {
        let (status, _) = fx.send(None, method.clone(), path, Some(json!({}))).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {path}");
    }
    for cookie in [&fx.alice, &fx.plain] {
        for (method, path) in [
            (Method::GET, "/api/v0/admin/organizations"),
            (Method::POST, "/api/v0/admin/organizations"),
            (Method::PUT, "/api/v0/admin/organizations/missing"),
        ] {
            let (status, _) = fx
                .send(Some(cookie), method.clone(), path, Some(json!({})))
                .await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{method} {path}");
        }
    }
}

#[tokio::test]
async fn foundation_membership_is_current_and_never_inferred_from_admin_or_unbound_groups() {
    let fx = fixture().await;
    let org = organization(&fx, "Acme Support", &["managers"]).await;
    for cookie in [&fx.alice, &fx.bob] {
        let (status, list) = fx.get(cookie, "/api/v0/organizations").await;
        assert_eq!(status, StatusCode::OK, "{list}");
        assert_eq!(list["organizations"].as_array().unwrap().len(), 1);
        assert_eq!(list["organizations"][0]["id"], org["id"]);
        assert_eq!(list["organizations"][0]["can_create_dot"], true);
    }
    for cookie in [&fx.root, &fx.plain] {
        let (status, list) = fx.get(cookie, "/api/v0/organizations").await;
        assert_eq!(status, StatusCode::OK, "{list}");
        assert_eq!(list["organizations"], json!([]));
        let (status, _) = fx
            .get(cookie, &format!("/api/v0/organizations/{}", id(&org)))
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
    let (status, detail) = fx
        .get(&fx.alice, &format!("/api/v0/organizations/{}", id(&org)))
        .await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["dots"], json!([]));
    assert_eq!(detail["active_count"], 0);
    assert_eq!(detail["active_limit"], 4);
    person(&fx.state, "alice", &[]).await;
    let (status, list) = fx.get(&fx.alice, "/api/v0/organizations").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["organizations"], json!([]));
    let (status, _) = fx
        .get(&fx.alice, &format!("/api/v0/organizations/{}", id(&org)))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn foundation_dot_creation_uses_member_manager_permission_not_personal_dot_permission() {
    let fx = fixture().await;
    let (status, group) = fx.send(Some(&fx.root), Method::PUT, "/api/v0/admin/groups", Some(json!({
        "name":"managers","oidc_values":["managers"],"can_manage_agents":true,"can_manage_dots":false
    }))).await;
    assert_eq!(status, StatusCode::OK, "{group}");
    let (_, me) = fx.get(&fx.alice, "/api/v0/me").await;
    assert_eq!(me["can_manage_agents"], true);
    assert_eq!(me["can_manage_dots"], false);
    let org = organization(&fx, "Acme Support", &["managers"]).await;
    let (status, created) = create_dot(&fx, &fx.alice, &org, "Harald").await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let agent = &created["agent"];
    assert_eq!(
        agent["dot"]["owner"],
        json!({"kind":"organization","id":org["id"],"name":org["name"]})
    );
    assert_eq!(agent["dot"]["name"], "Harald");
    assert_eq!(agent["dot"]["active"], true);
    assert_eq!(agent["display"], "Harald Support");
    assert_eq!(agent["access"], "write");
    assert_ne!(agent["name"], "Harald");
    let dot_id = id(agent);
    let (status, _) = fx
        .get(&fx.alice, &format!("/api/v0/dots/{dot_id}/access"))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, detail) = fx
        .get(&fx.alice, &format!("/api/v0/organizations/{}", id(&org)))
        .await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["dots"], json!([agent]));
    let (status, hidden) = fx
        .get(&fx.bob, &format!("/api/v0/organizations/{}", id(&org)))
        .await;
    assert_eq!(status, StatusCode::OK, "{hidden}");
    assert_eq!(hidden["dots"], json!([]));
    assert_eq!(hidden["active_count"], 1);
    let (status, _) = create_dot(&fx, &fx.plain, &org, "Foreign").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let plain_org = organization(&fx, "Everyone organization", &["everyone"]).await;
    let (status, _) = create_dot(&fx, &fx.plain, &plain_org, "No manager").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn foundation_dot_creation_rejects_spoofed_owner_and_invalid_names() {
    let fx = fixture().await;
    let org = organization(&fx, "Acme Support", &["managers"]).await;
    for body in [
        json!({"name":"Harald","organization_id":"different"}),
        json!({"name":"Harald","owner_user_id":"alice"}),
        json!({"name":"Harald","principal_id":"supplied"}),
        json!({"name":""}),
    ] {
        let (status, error) = fx
            .post(
                &fx.alice,
                &format!("/api/v0/organizations/{}/dots", id(&org)),
                body,
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
    }
    let (_, detail) = fx
        .get(&fx.alice, &format!("/api/v0/organizations/{}", id(&org)))
        .await;
    assert_eq!(detail["active_count"], 0);
}

#[tokio::test]
async fn foundation_dot_active_capacity_is_owner_scoped_and_reuses_deactivation() {
    let fx = fixture().await;
    let org = organization(&fx, "Acme Support", &["managers"]).await;
    let mut dots = Vec::new();
    for name in ["One", "Two", "Three", "Four"] {
        let (status, body) = create_dot(&fx, &fx.alice, &org, name).await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        dots.push(body["agent"].clone());
    }
    let (status, _) = create_dot(&fx, &fx.alice, &org, "Five").await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = create_dot(&fx, &fx.alice, &org, "One").await;
    assert_eq!(status, StatusCode::CONFLICT);
    let active_path = format!(
        "/api/v0/organizations/{}/dots/{}/active",
        id(&org),
        id(&dots[0])
    );
    let (status, disabled) = fx
        .post(&fx.alice, &active_path, json!({"active":false}))
        .await;
    assert_eq!(status, StatusCode::OK, "{disabled}");
    assert_eq!(disabled["agent"]["dot"]["active"], false);
    let (status, fifth) = create_dot(&fx, &fx.alice, &org, "Five").await;
    assert_eq!(status, StatusCode::CREATED, "{fifth}");
    let (status, _) = fx
        .post(&fx.alice, &active_path, json!({"active":true}))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let other = organization(&fx, "Other actual organization", &["managers"]).await;
    let (status, first_other) = create_dot(&fx, &fx.alice, &other, "One").await;
    assert_eq!(status, StatusCode::CREATED, "{first_other}");
    let wrong = format!(
        "/api/v0/organizations/{}/dots/{}/active",
        id(&other),
        id(&dots[0])
    );
    let (status, _) = fx.post(&fx.alice, &wrong, json!({"active":false})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    person(&fx.state, "alice", &[]).await;
    let (status, _) = fx
        .post(&fx.alice, &active_path, json!({"active":false}))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn organization_configuration_reuses_grants_publish_and_private_draft_but_refuses_shared_credentials()
 {
    use aiplane_core::server::db::mcp_catalog::{self, AuthKind, ConnectorInput, Scope};
    let mut fx = fixture().await;
    fx.state = fx.state.clone().with_agent_runner(std::sync::Arc::new(
        aiplane_runtime::agents::embed::LiveAgentRunner,
    ));
    mcp_catalog::create(
        &fx.state.db,
        ConnectorInput {
            key: "actual-shared-workspace".into(),
            name: "Actual shared Workspace".into(),
            description: None,
            icon: None,
            category: None,
            url: "https://operator.example.test/mcp".into(),
            auth: AuthKind::None,
            scope: Scope::Global,
            audit: false,
            use_dcr: false,
            client_id: None,
            client_secret_ct: None,
            client_secret_nonce: None,
            authorize_url: None,
            token_url: None,
            registration_url: None,
            scopes: Vec::new(),
            allowed_groups: Vec::new(),
        },
    )
    .await
    .unwrap();
    assert!(
        mcp_catalog::set_enabled(&fx.state.db, "actual-shared-workspace", true)
            .await
            .unwrap()
    );
    let org = organization(&fx, "Acme Support", &["managers"]).await;
    let (status, created) = create_dot(&fx, &fx.alice, &org, "Harald").await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let agent_id = id(&created["agent"]);
    let (status, resources) = fx
        .get(
            &fx.alice,
            &format!("/api/v0/agent-resources?agent_id={agent_id}"),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{resources}");
    let items = resources["items"].as_array().unwrap();
    assert!(
        items
            .iter()
            .all(|item| item["grant"]["kind"] != "connector")
    );
    assert!(items.iter().any(|item| {
        item["grant"]["refs"]
            .as_array()
            .unwrap()
            .contains(&json!(super::agents::TIME))
    }));
    let (status, refused) = fx
        .post(
            &fx.alice,
            &format!("/api/v0/system-principals/{agent_id}/grants"),
            json!({"kind":"connector","ref":"actual-shared-workspace"}),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{refused}");
    assert_eq!(
        fx.grant(&fx.alice, agent_id, "model", "m").await,
        StatusCode::CREATED
    );
    assert_eq!(
        fx.grant(&fx.alice, agent_id, "tool", super::agents::TIME)
            .await,
        StatusCode::CREATED
    );
    let (status, saved) = fx
        .put_draft(
            &fx.alice,
            agent_id,
            super::agents::spec("Actual organization task"),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    let (status, published) = fx.publish(&fx.alice, agent_id).await;
    assert_eq!(status, StatusCode::CREATED, "{published}");
    let (status, refused) = fx
        .post(
            &fx.alice,
            &format!("/api/v0/agents/{agent_id}/test/messages"),
            json!({"source":"draft","message":"Actual test request"}),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{refused}");
    assert_eq!(refused["agent_version"], 0);
    let (status, refused) = fx
        .post(
            &fx.root,
            &format!("/api/v0/system-principals/{agent_id}/tokens"),
            json!({"name":"No employee fallback"}),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{refused}");
}

#[tokio::test]
async fn organization_subject_search_filters_members_before_limit_and_keeps_exact_email_privacy() {
    let fx = fixture().await;
    let org = organization(&fx, "Acme Support", &["managers"]).await;
    let (status, created) = create_dot(&fx, &fx.alice, &org, "Harald").await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let agent_id = id(&created["agent"]);
    for index in 0..20 {
        person(&fx.state, &format!("candidate-{index:02}"), &[]).await;
    }
    person(&fx.state, "candidate-z-member", &["managers"]).await;
    let path = format!("/api/v0/agents/{agent_id}/share-subjects?q=candidate");
    let (status, found) = fx.get(&fx.alice, &path).await;
    assert_eq!(status, StatusCode::OK, "{found}");
    assert_eq!(
        found["users"],
        json!([{"id":"candidate-z-member","name":null}])
    );
    assert_eq!(found["truncated"], false);
    let (status, exact) = fx
        .get(
            &fx.alice,
            &format!("/api/v0/agents/{agent_id}/share-subjects?q=candidate-z-member%40example.com"),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{exact}");
    assert_eq!(exact["users"][0]["email"], "candidate-z-member@example.com");
    let (status, refused) = fx
        .share(&fx.alice, agent_id, "user", "candidate-00", "respond")
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{refused}");
    let (status, refused) = fx
        .share(&fx.alice, agent_id, "group", "everyone", "respond")
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{refused}");
    let (status, allowed) = fx
        .share(&fx.alice, agent_id, "user", "candidate-z-member", "respond")
        .await;
    assert_eq!(status, StatusCode::CREATED, "{allowed}");
}

#[tokio::test]
async fn organization_member_revocation_blocks_existing_agent_share_and_principal_mutation_surfaces()
 {
    let fx = fixture().await;
    let org = organization(&fx, "Acme Support", &["managers"]).await;
    let (status, created) = create_dot(&fx, &fx.alice, &org, "Harald").await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let agent_id = id(&created["agent"]);
    let (status, _) = fx.share(&fx.alice, agent_id, "user", "bob", "read").await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, visible) = fx.get(&fx.bob, &format!("/api/v0/agents/{agent_id}")).await;
    assert_eq!(status, StatusCode::OK, "{visible}");
    let (status, edited) = edit(&fx, &org, "Acme Support", &[]).await;
    assert_eq!(status, StatusCode::OK, "{edited}");
    for cookie in [&fx.alice, &fx.bob] {
        for path in [
            format!("/api/v0/agents/{agent_id}"),
            format!("/api/v0/system-principals/{agent_id}"),
            format!("/api/v0/agent-resources?agent_id={agent_id}"),
        ] {
            let (status, refused) = fx.get(cookie, &path).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{path}: {refused}");
        }
        let (status, list) = fx.get(cookie, "/api/v0/agents").await;
        assert_eq!(status, StatusCode::OK, "{list}");
        assert!(
            !list["agents"]
                .as_array()
                .unwrap()
                .iter()
                .any(|agent| agent["id"] == agent_id)
        );
    }
    let (status, refused) = fx.put_draft(&fx.alice, agent_id, json!({})).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{refused}");
    let (status, refused) = fx
        .post(
            &fx.alice,
            &format!("/api/v0/system-principals/{agent_id}/disable"),
            json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{refused}");
    let (status, refused) = fx
        .post(
            &fx.alice,
            &format!("/api/v0/system-principals/{agent_id}/grants"),
            json!({"kind":"model","ref":"m"}),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{refused}");
    let (status, recovery) = fx
        .get(&fx.root, &format!("/api/v0/agents/{agent_id}"))
        .await;
    assert_eq!(status, StatusCode::OK, "{recovery}");
    assert_eq!(recovery["agent"]["dot"]["owner"]["name"], "Acme Support");
}

#[tokio::test]
async fn organization_subject_search_reports_actual_scan_budget_without_leaking_nonmembers() {
    let fx = fixture().await;
    let org = organization(&fx, "Acme Support", &["managers"]).await;
    let (status, created) = create_dot(&fx, &fx.alice, &org, "Harald").await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let agent_id = id(&created["agent"]);
    for index in 0..1001 {
        let now = jiff::Timestamp::now();
        aiplane_core::server::db::users::upsert(
            &fx.state.db,
            &aiplane_core::server::db::users::User {
                id: format!("budget-{index:04}"),
                email: format!("budget-{index:04}@example.com"),
                name: None,
                roles: Vec::new(),
                created_at: now,
                updated_at: now,
                timezone: None,
                speech_voice: None,
            },
        )
        .await
        .unwrap();
    }
    person(&fx.state, "budget-z-member", &["managers"]).await;
    let (status, limited) = fx
        .get(
            &fx.alice,
            &format!("/api/v0/agents/{agent_id}/share-subjects?q=budget"),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{limited}");
    assert_eq!(limited["users"], json!([]));
    assert_eq!(limited["truncated"], true);
    let (status, narrowed) = fx
        .get(
            &fx.alice,
            &format!("/api/v0/agents/{agent_id}/share-subjects?q=budget-z-member"),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{narrowed}");
    assert_eq!(
        narrowed["users"],
        json!([{"id":"budget-z-member","name":null}])
    );
    assert_eq!(narrowed["truncated"], false);
}

#[tokio::test]
async fn organization_resources_use_current_tool_grants_without_global_rbac_reload() {
    use aiplane_core::server::db::gateway_groups;
    let fx = fixture().await;
    let org = organization(&fx, "Acme Support", &["managers"]).await;
    let (status, created) = create_dot(&fx, &fx.alice, &org, "Harald").await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let path = format!("/api/v0/agent-resources?agent_id={}", id(&created["agent"]));
    let (status, initial) = resource_choices(&fx, &path).await;
    assert_eq!(status, StatusCode::OK, "{initial}");
    assert!(resource_has(&initial, "tool", super::agents::TIME));
    gateway_groups::set_tools_for_group(&fx.state.db, "everyone", &[])
        .await
        .unwrap();
    let (status, current) = resource_choices(&fx, &path).await;
    assert_eq!(status, StatusCode::OK, "{current}");
    assert!(
        !resource_has(&current, "tool", super::agents::TIME),
        "{current}"
    );
    assert!(
        current["models"]["chat"]
            .as_array()
            .unwrap()
            .iter()
            .any(|model| model["id"] == "m")
    );
}

async fn resource_choices(fx: &Fx, path: &str) -> (StatusCode, Value) {
    tokio::time::timeout(std::time::Duration::from_secs(10), fx.get(&fx.alice, path))
        .await
        .expect("resource selection must finish without waiting on its own transaction")
}

fn resource_has(view: &Value, kind: &str, reference: &str) -> bool {
    view["items"].as_array().unwrap().iter().any(|item| {
        item["grant"]["kind"] == kind
            && item["grant"]["refs"]
                .as_array()
                .unwrap()
                .contains(&json!(reference))
    })
}

async fn mapped_resource_fixture() -> (Fx, Value, String) {
    use aiplane_core::server::{
        db::{gateway_groups, rag},
        upstreams::config::{PickerStrategy, PoolKind, UpstreamPoolConfig},
    };
    use aiplane_runtime::server::tools::{ToolRegistry, time::CurrentTimestamp};
    let fx = super::agents::fixture_with_pools(
        None,
        ToolRegistry::new().with(CurrentTimestamp),
        &[super::agents::TIME],
        vec![(
            "restricted".into(),
            UpstreamPoolConfig {
                voices: Default::default(),
                offer_voices: Vec::new(),
                allowed_groups: vec!["support".into()],
                fallback_offline: None,
                compliance: Default::default(),
                enforce_limits: true,
                kind: PoolKind::Chat,
                strategy: PickerStrategy::RoundRobin,
                models: Vec::new(),
                backend: vec![crate::common::mock_backend(
                    "restricted",
                    "http://127.0.0.1:9",
                )],
            },
            vec!["restricted-model".into()],
        )],
    )
    .await;
    gateway_groups::set_mappings_for_group(
        &fx.state.db,
        "support",
        &["resource-entitlement".into()],
    )
    .await
    .unwrap();
    fx.state.reload_rbac().await;
    person(&fx.state, "alice", &["managers", "resource-entitlement"]).await;
    let collection = rag::create_collection(
        &fx.state.db,
        &rag::NewCollection {
            name: "Actual restricted support knowledge".into(),
            description: Some("Actual configuration data".into()),
            git_url: "https://operator.example.test/support.git".into(),
            git_ref: "main".into(),
            pat: None,
            source: Default::default(),
            profile_id: None,
            extraction_model: None,
            embedding_model: "configured-embedding".into(),
            include_globs: Vec::new(),
            exclude_globs: Vec::new(),
            chunk_size: 1000,
            chunk_overlap: 100,
            search_mode: rag::SearchMode::Versioned,
            refresh_interval_mins: 0,
        },
    )
    .await
    .unwrap();
    rag::set_allowed_groups(&fx.state.db, collection.id, &["support".into()])
        .await
        .unwrap();
    let org = organization(&fx, "Acme Support", &["managers"]).await;
    let (status, created) = create_dot(&fx, &fx.alice, &org, "Harald").await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    (fx, created["agent"].clone(), collection.id.to_string())
}

#[tokio::test]
async fn organization_resources_use_current_group_mappings_for_models_and_knowledge() {
    use aiplane_core::server::db::gateway_groups;
    let (fx, agent, collection_id) = mapped_resource_fixture().await;
    let path = format!("/api/v0/agent-resources?agent_id={}", id(&agent));
    let (status, initial) = resource_choices(&fx, &path).await;
    assert_eq!(status, StatusCode::OK, "{initial}");
    assert!(
        resource_has(&initial, "rag_collection", &collection_id),
        "{initial}"
    );
    assert!(
        initial["models"]["chat"]
            .as_array()
            .unwrap()
            .iter()
            .any(|model| model["id"] == "restricted-model"),
        "{initial}"
    );
    gateway_groups::set_mappings_for_group(&fx.state.db, "support", &["other-entitlement".into()])
        .await
        .unwrap();
    let (status, current) = resource_choices(&fx, &path).await;
    assert_eq!(status, StatusCode::OK, "{current}");
    assert!(
        !resource_has(&current, "rag_collection", &collection_id),
        "{current}"
    );
    assert!(
        current["models"]["chat"]
            .as_array()
            .unwrap()
            .iter()
            .all(|model| model["id"] != "restricted-model"),
        "{current}"
    );
    assert!(resource_has(&current, "tool", super::agents::TIME));
}

#[tokio::test]
async fn organization_resources_reload_current_user_roles_without_changing_membership() {
    let (fx, agent, collection_id) = mapped_resource_fixture().await;
    let path = format!("/api/v0/agent-resources?agent_id={}", id(&agent));
    let (status, initial) = resource_choices(&fx, &path).await;
    assert_eq!(status, StatusCode::OK, "{initial}");
    assert!(resource_has(&initial, "rag_collection", &collection_id));
    person(&fx.state, "alice", &["managers"]).await;
    let (status, current) = resource_choices(&fx, &path).await;
    assert_eq!(status, StatusCode::OK, "{current}");
    assert!(
        !resource_has(&current, "rag_collection", &collection_id),
        "{current}"
    );
    assert!(
        current["models"]["chat"]
            .as_array()
            .unwrap()
            .iter()
            .all(|model| model["id"] != "restricted-model"),
        "{current}"
    );
    assert!(resource_has(&current, "tool", super::agents::TIME));
}

fn offered_workspace(view: &Value) -> bool {
    view["items"].as_array().unwrap().iter().any(|item| {
        item["grant"]["kind"] == "connector"
            && item["grant"]["refs"]
                .as_array()
                .unwrap()
                .contains(&json!("workspace"))
    })
}

async fn select_organization_credential(fx: &Fx, dot: &str) {
    use aiplane_core::server::db::mcp_connections::ConnectionOwner;
    let own = seed_organization_credential(fx, ConnectionOwner::Principal(dot.to_owned())).await;
    let (status, selected) = fx
        .send(
            Some(&fx.alice),
            Method::PUT,
            &format!("/api/v0/dots/{dot}/connections/workspace"),
            Some(
                json!({"mode":"principal","expected_binding_generation":null,
            "connection_id":own.connection_id,"connection_generation":own.generation}),
            ),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{selected}");
}

#[tokio::test]
async fn organization_connector_resources_and_grant_require_explicit_selected_own_identity() {
    use aiplane_core::server::db::mcp_connections::ConnectionOwner;
    let (fx, _provider, _org, dot) = oauth_organization().await;
    seed_organization_credential(&fx, ConnectionOwner::User("alice".into())).await;
    let path = format!("/api/v0/agent-resources?agent_id={dot}");
    let (status, before) = fx.get(&fx.alice, &path).await;
    assert_eq!(status, StatusCode::OK, "{before}");
    assert!(!offered_workspace(&before));
    let principal_path = format!("/api/v0/system-principals/{dot}/grants");
    let (status, _) = fx
        .post(
            &fx.alice,
            &principal_path,
            json!({"kind":"connector","ref":"workspace"}),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    select_organization_credential(&fx, &dot).await;
    let (status, selected) = fx.get(&fx.alice, &path).await;
    assert_eq!(status, StatusCode::OK, "{selected}");
    assert!(offered_workspace(&selected), "{selected}");
    let (_, untargeted) = fx.get(&fx.alice, "/api/v0/agent-resources").await;
    assert!(!offered_workspace(&untargeted));
    let (status, grant) = fx
        .post(
            &fx.alice,
            &principal_path,
            json!({"kind":"connector","ref":"workspace"}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{grant}");
    let grants = aiplane_agents::db::system_principals::grants(&fx.state.db, &dot)
        .await
        .unwrap();
    assert!(grants.iter().any(|grant| grant.kind
        == aiplane_core::server::principal::GrantKind::Connector
        && grant.reference == "workspace"));
}

#[tokio::test]
async fn organization_connector_grant_rechecks_membership_after_resource_offer() {
    let (fx, _provider, org, dot) = oauth_organization().await;
    select_organization_credential(&fx, &dot).await;
    let path = format!("/api/v0/agent-resources?agent_id={dot}");
    let (_, offered) = fx.get(&fx.alice, &path).await;
    assert!(offered_workspace(&offered), "{offered}");
    let (status, changed) = edit(&fx, &org, "Acme OAuth", &[]).await;
    assert_eq!(status, StatusCode::OK, "{changed}");
    let (status, _) = fx
        .post(
            &fx.alice,
            &format!("/api/v0/system-principals/{dot}/grants"),
            json!({"kind":"connector","ref":"workspace"}),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let grants = aiplane_agents::db::system_principals::grants(&fx.state.db, &dot)
        .await
        .unwrap();
    assert!(!grants.iter().any(|grant| grant.kind
        == aiplane_core::server::principal::GrantKind::Connector
        && grant.reference == "workspace"));
}

#[tokio::test]
async fn organization_private_draft_chat_admits_member_writer_without_personal_dot_permission() {
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };
    let upstream = MockServer::start().await;
    Mock::given(method("POST")).and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Actual organization answer.\"}}]}\n\ndata: [DONE]\n\n",
            "text/event-stream")).mount(&upstream).await;
    let mut fx = super::agents::fixture_on(Some(&upstream.uri())).await;
    fx.state = fx.state.clone().with_agent_runner(std::sync::Arc::new(
        aiplane_runtime::agents::embed::LiveAgentRunner,
    ));
    aiplane_core::server::db::gateway_groups::set_can_manage_dots(&fx.state.db, "managers", false)
        .await
        .unwrap();
    let org = organization(&fx, "Acme Draft", &["managers"]).await;
    let (status, created) = create_dot(&fx, &fx.alice, &org, "Harald").await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let dot = id(&created["agent"]);
    assert_eq!(
        fx.grant(&fx.alice, dot, "model", "m").await,
        StatusCode::CREATED
    );
    assert_eq!(
        fx.grant(&fx.alice, dot, "tool", super::agents::TIME).await,
        StatusCode::CREATED
    );
    let (status, saved) = fx
        .put_draft(
            &fx.alice,
            dot,
            super::agents::spec("Actual organization task"),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    let (status, started) = fx
        .post(
            &fx.alice,
            &format!("/api/v0/agents/{dot}/test/messages"),
            json!({"source":"draft","message":"Actual draft request"}),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{started}");
    assert_eq!(started["agent_version"], 0);
    let session = started["session_id"].as_str().unwrap();
    let row: (Option<String>, Option<String>, bool) =
        sqlx::query_as("SELECT principal_id,user_id,shared FROM chat_sessions WHERE id=?")
            .bind(session)
            .fetch_one(&fx.state.db)
            .await
            .unwrap();
    assert_eq!(row.0.as_deref(), Some(dot));
    assert_eq!(row.1, None);
    assert!(!row.2);
    let history = format!("/api/v0/agents/{dot}/test/{session}/events");
    let turn = started["turn_id"].as_str().unwrap();
    let debug = format!("/api/v0/agents/{dot}/test/{session}/turns/{turn}/debug");
    let (status, inspected) = fx.get(&fx.alice, &debug).await;
    assert_eq!(status, StatusCode::OK, "{inspected}");
    let (status, _) = fx.get(&fx.root, &debug).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, changed) = edit(&fx, &org, "Acme Draft", &[]).await;
    assert_eq!(status, StatusCode::OK, "{changed}");
    let (status, _) = fx.send(Some(&fx.alice), Method::GET, &history, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = fx.send(Some(&fx.root), Method::GET, &history, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

async fn organization_memory_fixture() -> (Fx, Value, String) {
    let fx = fixture().await;
    aiplane_core::server::db::gateway_groups::set_can_manage_dots(&fx.state.db, "managers", false)
        .await
        .unwrap();
    fx.state.reload_rbac().await;
    let org = organization(&fx, "Acme Knowledge", &["managers", "admins"]).await;
    let (status, created) = create_dot(&fx, &fx.alice, &org, "Harald").await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let dot = id(&created["agent"]).to_owned();
    (fx, org, dot)
}

fn memory_scope<'a>(view: &'a Value, scope: &str) -> &'a Value {
    view["scopes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["scope"] == scope)
        .unwrap()
}

#[tokio::test]
async fn organization_memory_uses_existing_dot_routes_with_actual_scope_metadata_and_separate_write_rights()
 {
    let (fx, org, dot) = organization_memory_fixture().await;
    let base = format!("/api/v0/dots/{dot}/memories");
    let (_, me) = fx.get(&fx.alice, "/api/v0/me").await;
    assert_eq!(me["can_manage_dots"], false);
    let (status, own) = fx
        .post(
            &fx.alice,
            &base,
            json!({"scope":"dot","kind":"project","content":"Actual isolated Dot lesson"}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{own}");
    assert_eq!(own["scope"], "dot");
    let (status, refused) = fx
        .post(
            &fx.alice,
            &base,
            json!({"scope":"organization","kind":"fact","content":"Unapproved shared change"}),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{refused}");
    let (status, shared) = fx.post(&fx.root,&base,
        json!({"scope":"organization","kind":"preference","content":"Actual shared organization preference"})).await;
    assert_eq!(status, StatusCode::CREATED, "{shared}");
    assert_eq!(shared["scope"], "organization");
    let (status, view) = fx.get(&fx.alice, &base).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(memory_scope(&view, "dot")["name"], "Harald");
    assert_eq!(memory_scope(&view, "dot")["writable"], true);
    assert_eq!(memory_scope(&view, "organization")["name"], org["name"]);
    assert_eq!(memory_scope(&view, "organization")["writable"], false);
    assert_eq!(view["memories"].as_array().unwrap().len(), 2);
    assert!(view["memories"].as_array().unwrap().contains(&own));
    assert!(view["memories"].as_array().unwrap().contains(&shared));
    let (status, _) = fx.share(&fx.alice, &dot, "user", "bob", "read").await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, reader) = fx.get(&fx.bob, &base).await;
    assert_eq!(status, StatusCode::OK, "{reader}");
    assert_eq!(reader["memories"], view["memories"]);
    assert_eq!(memory_scope(&reader, "dot")["writable"], false);
    assert_eq!(memory_scope(&reader, "organization")["writable"], false);
    let (status, denied) = fx
        .post(
            &fx.bob,
            &base,
            json!({"scope":"dot","kind":"fact","content":"Read-only cannot write"}),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{denied}");
    let (status, _) = fx.share(&fx.alice, &dot, "user", "bob", "respond").await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, responder) = fx.get(&fx.bob, &base).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{responder}");
    let (status, clara) = create_dot(&fx, &fx.alice, &org, "Clara").await;
    assert_eq!(status, StatusCode::CREATED);
    let (_, sibling_notes) = fx
        .get(
            &fx.alice,
            &format!("/api/v0/dots/{}/memories", id(&clara["agent"])),
        )
        .await;
    assert_eq!(sibling_notes["memories"], json!([shared.clone()]));
    let (status, updated) = fx
        .send(
            Some(&fx.root),
            Method::PUT,
            &format!("{base}/{}", shared["id"].as_str().unwrap()),
            Some(json!({"scope":"organization","kind":"fact","content":"Corrected shared fact"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    let (status, _) = fx
        .send(
            Some(&fx.root),
            Method::DELETE,
            &format!(
                "{base}/{}?scope=organization",
                shared["id"].as_str().unwrap()
            ),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn organization_memory_foreign_ids_and_other_scopes_do_not_disclose_or_mutate_notes() {
    let (fx, org, dot) = organization_memory_fixture().await;
    let other_org = organization(&fx, "Other actual organization", &["managers", "admins"]).await;
    let (status, created) = create_dot(&fx, &fx.alice, &other_org, "Clara").await;
    assert_eq!(status, StatusCode::CREATED);
    let other = id(&created["agent"]);
    let target = format!("/api/v0/dots/{dot}/memories");
    let foreign_base = format!("/api/v0/dots/{other}/memories");
    let (status, foreign) = fx.post(&fx.root,&foreign_base,
        json!({"scope":"organization","kind":"fact","content":"Other organization confidential marker"})).await;
    assert_eq!(status, StatusCode::CREATED, "{foreign}");
    for item in [foreign["id"].as_str().unwrap(), "unknown-memory"] {
        let (status, error) = fx.send(Some(&fx.root),Method::PUT,&format!("{target}/{item}"),
            Some(json!({"scope":"organization","kind":"fact","content":"Unauthorized overwrite"}))).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{error}");
        assert!(
            !error
                .to_string()
                .contains("Other organization confidential marker")
        );
        let (status, error) = fx
            .send(
                Some(&fx.root),
                Method::DELETE,
                &format!("{target}/{item}?scope=organization"),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{error}");
    }
    let (status, view) = fx.get(&fx.root, &foreign_base).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view["memories"], json!([foreign]));
    let (status, invalid) = fx
        .post(
            &fx.alice,
            &target,
            json!({"scope":"user","kind":"fact","content":"No employee scope"}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{invalid}");
    let (status, revised) = edit(&fx, &org, "Acme Knowledge", &["managers"]).await;
    assert_eq!(status, StatusCode::OK, "{revised}");
    let (status, nonmember_admin) = fx.get(&fx.root, &target).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{nonmember_admin}");
    assert!(nonmember_admin.get("memories").is_none());
}

#[tokio::test]
async fn organization_memory_rechecks_current_membership_and_admin_after_management_views() {
    let (fx, org, dot) = organization_memory_fixture().await;
    let target = format!("/api/v0/dots/{dot}/memories");
    let (status, own) = fx
        .post(
            &fx.alice,
            &target,
            json!({"scope":"dot","kind":"fact","content":"Preserved current authority lesson"}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{own}");
    let (_, offered) = fx.get(&fx.alice, &target).await;
    assert_eq!(memory_scope(&offered, "dot")["writable"], true);
    let (status, changed) = edit(&fx, &org, "Acme Knowledge", &["admins"]).await;
    assert_eq!(status, StatusCode::OK, "{changed}");
    let (status, denied) = fx
        .send(
            Some(&fx.alice),
            Method::PUT,
            &format!("{target}/{}", own["id"].as_str().unwrap()),
            Some(json!({"scope":"dot","kind":"fact","content":"Stale membership write"})),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{denied}");
    let (_, still_saved) = fx.get(&fx.root, &target).await;
    assert_eq!(still_saved["memories"], json!([own]));
    assert_eq!(memory_scope(&still_saved, "organization")["writable"], true);
    aiplane_core::server::db::gateway_groups::upsert_group(
        &fx.state.db,
        "admins",
        "",
        false,
        false,
    )
    .await
    .unwrap();
    let (status, denied) = fx
        .post(
            &fx.root,
            &target,
            json!({"scope":"organization","kind":"fact","content":"Stale administrator write"}),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{denied}");
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM dot_memories WHERE content='Stale administrator write'",
    )
    .fetch_one(&fx.state.db)
    .await
    .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn organization_memory_requires_explicit_scope_and_limits_human_saves_by_utf8_bytes() {
    let (fx, _, dot) = organization_memory_fixture().await;
    let base = format!("/api/v0/dots/{dot}/memories");
    let (status, accepted) = fx
        .post(
            &fx.alice,
            &base,
            json!({"scope":"dot","kind":"fact","content":"é".repeat(1000)}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{accepted}");
    let item = format!("{base}/{}", accepted["id"].as_str().unwrap());
    for (method, path, body) in [
        (
            Method::POST,
            base.clone(),
            json!({"kind":"fact","content":"Missing scope"}),
        ),
        (
            Method::POST,
            base.clone(),
            json!({"scope":"dot","kind":"fact","content":"é".repeat(1001)}),
        ),
        (
            Method::PUT,
            item.clone(),
            json!({"kind":"fact","content":"Missing scope"}),
        ),
        (
            Method::PUT,
            item.clone(),
            json!({"scope":"dot","kind":"fact","content":"x".repeat(2001)}),
        ),
    ] {
        let (status, denied) = fx.send(Some(&fx.alice), method, &path, Some(body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{denied}");
    }
    let (status, denied) = fx.send(Some(&fx.alice), Method::DELETE, &item, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{denied}");
    let (_, persisted) = fx.get(&fx.alice, &base).await;
    assert_eq!(persisted["memories"], json!([accepted]));
}

async fn private_published_organization() -> (Fx, wiremock::MockServer, Value, String) {
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };
    let upstream = MockServer::start().await;
    Mock::given(method("POST")).and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Published organization answer.\"}}]}\n\ndata: [DONE]\n\n",
            "text/event-stream")).mount(&upstream).await;
    let mut fx = super::agents::fixture_on(Some(&upstream.uri())).await;
    fx.state = fx.state.clone().with_agent_runner(std::sync::Arc::new(
        aiplane_runtime::agents::embed::LiveAgentRunner,
    ));
    let org = organization(&fx, "Acme Published", &["managers"]).await;
    let (status, created) = create_dot(&fx, &fx.alice, &org, "Harald").await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let dot = id(&created["agent"]).to_owned();
    for (kind, reference) in [("model", "m"), ("tool", super::agents::TIME)] {
        assert_eq!(
            fx.grant(&fx.alice, &dot, kind, reference).await,
            StatusCode::CREATED
        );
    }
    assert_eq!(
        fx.put_draft(
            &fx.alice,
            &dot,
            super::agents::spec("First published instructions")
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(fx.publish(&fx.alice, &dot).await.0, StatusCode::CREATED);
    (fx, upstream, org, dot)
}

#[tokio::test]
async fn organization_private_published_chat_requires_explicit_source_and_refuses_ordinary_published()
 {
    let (fx, upstream, _org, dot) = private_published_organization().await;
    let endpoint = format!("/api/v0/agents/{dot}/test/messages");
    for body in [
        json!({"message":"Missing source"}),
        json!({"message":"Invalid source","source":"live"}),
        json!({"message":"Null source","source":null}),
    ] {
        let (status, response) = fx.post(&fx.alice, &endpoint, body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{response}");
    }
    let ordinary = fx.runnable("ordinary-private-source").await;
    let (status, refused) = fx
        .post(
            &fx.alice,
            &format!("/api/v0/agents/{ordinary}/test/messages"),
            json!({"message":"No ordinary published admission","source":"published"}),
        )
        .await;
    assert!(!status.is_success(), "{refused}");
    assert!(upstream.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn organization_private_published_chat_keeps_pinned_version_after_publish_and_rejects_source_change()
 {
    let (fx, upstream, _org, dot) = private_published_organization().await;
    let endpoint = format!("/api/v0/agents/{dot}/test/messages");
    let (status, first) = fx
        .post(
            &fx.alice,
            &endpoint,
            json!({"message":"First private message","source":"published"}),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{first}");
    assert_eq!(first["agent_version"], 1);
    assert_eq!(first["source"], "published");
    assert!(first.get("draft_version").is_none());
    let session = first["session_id"].as_str().unwrap();
    let turn = first["turn_id"].as_str().unwrap();
    let debug = crate::common::test_chat_settled(&fx.state, &fx.alice, &dot, session, turn).await;
    assert_eq!(debug["status"], "completed", "{debug}");
    assert_eq!(
        fx.put_draft(
            &fx.alice,
            &dot,
            super::agents::spec("Second published instructions")
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(fx.publish(&fx.alice, &dot).await.0, StatusCode::CREATED);
    let (status, second) = fx.post(&fx.alice, &endpoint, json!({"message":"Continue pinned conversation","source":"published","session_id":session})).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{second}");
    assert_eq!(second["agent_version"], 1);
    assert_eq!(second["source"], "published");
    crate::common::test_chat_settled(
        &fx.state,
        &fx.alice,
        &dot,
        session,
        second["turn_id"].as_str().unwrap(),
    )
    .await;
    let (status, wrong) = fx
        .post(
            &fx.alice,
            &endpoint,
            json!({"message":"Do not rebase","source":"draft","session_id":session}),
        )
        .await;
    assert!(!status.is_success(), "{wrong}");
    let requests = upstream.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2);
    for request in requests {
        let body = String::from_utf8(request.body).unwrap();
        assert!(body.contains("First published instructions"), "{body}");
        assert!(!body.contains("Second published instructions"), "{body}");
    }
    let row = aiplane_agents::db::run_sessions::get_principal_session(&fx.state.db, &dot, session)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.agent_version, Some(1));
}

#[tokio::test]
async fn organization_private_published_history_requires_current_member_write_and_private_context()
{
    let (fx, _upstream, org, dot) = private_published_organization().await;
    let (status, started) = fx
        .post(
            &fx.alice,
            &format!("/api/v0/agents/{dot}/test/messages"),
            json!({"message":"Private history","source":"published"}),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{started}");
    let session = started["session_id"].as_str().unwrap();
    let turn = started["turn_id"].as_str().unwrap();
    let debug_path = format!("/api/v0/agents/{dot}/test/{session}/turns/{turn}/debug");
    let events_path = format!("/api/v0/agents/{dot}/test/{session}/events");
    let (status, debug) = fx.get(&fx.alice, &debug_path).await;
    assert_eq!(status, StatusCode::OK, "{debug}");
    assert_eq!(debug["agent_version"], 1);
    assert_eq!(debug["source"], "published");
    assert_eq!(fx.get(&fx.root, &debug_path).await.0, StatusCode::NOT_FOUND);
    for (access, expected) in [
        ("read", StatusCode::FORBIDDEN),
        ("respond", StatusCode::NOT_FOUND),
    ] {
        assert_eq!(
            fx.share(&fx.alice, &dot, "user", "bob", access).await.0,
            StatusCode::CREATED
        );
        for uri in [&debug_path, &events_path] {
            let (status, hidden) = fx.send(Some(&fx.bob), Method::GET, uri, None).await;
            assert_eq!(status, expected, "{hidden}");
            assert!(hidden.get("debug").is_none());
            assert!(hidden.get("source").is_none());
            assert!(hidden.get("agent_version").is_none());
        }
    }
    assert_eq!(
        edit(&fx, &org, "Acme Published", &[]).await.0,
        StatusCode::OK
    );
    assert_eq!(
        fx.get(&fx.alice, &debug_path).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        fx.send(Some(&fx.alice), Method::GET, &events_path, None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        fx.get(&fx.alice, &format!("/api/v0/chat/sessions/{session}"))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn organization_private_published_pause_inbox_is_current_member_writer_only() {
    use wiremock::{
        Mock, ResponseTemplate,
        matchers::{method, path},
    };
    let (fx, upstream, org, dot) = private_published_organization().await;
    upstream.reset().await;
    let delta = json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"private-published-time","type":"function","function":{"name":super::agents::TIME,"arguments":"{}"}}]}}]});
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            format!("data: {delta}\n\ndata: [DONE]\n\n"),
            "text/event-stream",
        ))
        .mount(&upstream)
        .await;
    let mut spec = super::agents::spec("Ask before reading time");
    spec["main"]["tool_resources"] = json!({(super::agents::TIME):{"permission":"always_ask"}});
    assert_eq!(fx.put_draft(&fx.alice, &dot, spec).await.0, StatusCode::OK);
    assert_eq!(fx.publish(&fx.alice, &dot).await.0, StatusCode::CREATED);
    let (status, started) = fx
        .post(
            &fx.alice,
            &format!("/api/v0/agents/{dot}/test/messages"),
            json!({"message":"Request approved time","source":"published"}),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{started}");
    let settled = crate::common::test_chat_settled(
        &fx.state,
        &fx.alice,
        &dot,
        started["session_id"].as_str().unwrap(),
        started["turn_id"].as_str().unwrap(),
    )
    .await;
    assert_eq!(settled["status"], "suspended", "{settled}");
    let (status, inbox) = fx.get(&fx.alice, "/api/v0/agents/inbox").await;
    assert_eq!(status, StatusCode::OK, "{inbox}");
    assert_eq!(inbox["count"], 1, "{inbox}");
    for access in ["read", "respond"] {
        assert_eq!(
            fx.share(&fx.alice, &dot, "user", "bob", access).await.0,
            StatusCode::CREATED
        );
        let (status, hidden) = fx.get(&fx.bob, "/api/v0/agents/inbox").await;
        assert_eq!(status, StatusCode::OK, "{hidden}");
        assert_eq!(hidden["count"], 0, "{hidden}");
    }
    let (status, hidden) = fx.get(&fx.root, "/api/v0/agents/inbox").await;
    assert_eq!(status, StatusCode::OK, "{hidden}");
    assert_eq!(hidden["count"], 0, "{hidden}");
    assert_eq!(
        edit(&fx, &org, "Acme Published", &[]).await.0,
        StatusCode::OK
    );
    let (status, hidden) = fx.get(&fx.alice, "/api/v0/agents/inbox").await;
    assert_eq!(status, StatusCode::OK, "{hidden}");
    assert_eq!(hidden["count"], 0, "{hidden}");
}

#[tokio::test]
async fn organization_private_published_debug_rechecks_membership_after_waiting_for_release() {
    let (fx, _upstream, org, dot) = private_published_organization().await;
    let (status, started) = fx
        .post(
            &fx.alice,
            &format!("/api/v0/agents/{dot}/test/messages"),
            json!({"message":"Private delayed debug","source":"published"}),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{started}");
    let session = started["session_id"].as_str().unwrap();
    let turn = started["turn_id"].as_str().unwrap();
    crate::common::test_chat_settled(&fx.state, &fx.alice, &dot, session, turn).await;
    let claim =
        aiplane_runtime::agents::embed::claim(&fx.state.chats, &dot, session, turn).unwrap();
    let updates = claim.worker().broadcast.clone();
    let state = fx.state.clone();
    let cookie = fx.alice.clone();
    let uri = format!("/api/v0/agents/{dot}/test/{session}/turns/{turn}/debug");
    let request = tokio::spawn(async move { crate::common::get_json(&state, &cookie, &uri).await });
    for _ in 0..500 {
        if updates.receiver_count() > 0 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert!(
        updates.receiver_count() > 0,
        "the debug request reached its release wait"
    );
    assert_eq!(
        edit(&fx, &org, "Acme Published", &[]).await.0,
        StatusCode::OK
    );
    drop(claim);
    let (status, body) = tokio::time::timeout(std::time::Duration::from_secs(3), request)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert!(body.get("debug").is_none());
}

fn organization_schedule_body(dot: &str) -> Value {
    json!({"principal_id":dot,"agent_version":1,"name":"Incident review","prompt":"Review incidents",
        "cron":"0 9 * * *","timezone":"Europe/Berlin","tools_enabled":true,
        "reuse_conversation":false,"reuse_rounds":5})
}

#[tokio::test]
async fn organization_schedule_inventory_create_and_history_use_current_member_write() {
    let (fx, _upstream, _org, dot) = private_published_organization().await;
    aiplane_core::server::db::gateway_groups::set_can_manage_dots(&fx.state.db, "managers", false)
        .await
        .unwrap();
    let path = format!("/api/v0/scheduled?principal_id={dot}");
    let (status, inventory) = fx.get(&fx.alice, &path).await;
    assert_eq!(status, StatusCode::OK, "{inventory}");
    assert_eq!(inventory["principal_id"], dot);
    assert_eq!(inventory["can_write"], true);
    assert_eq!(inventory["models"], json!([]));
    assert_eq!(inventory["default_timezone"], Value::Null);
    assert_eq!(inventory["dots"][0]["id"], dot);
    assert_eq!(inventory["versions"][0]["version"], 1);
    let (status, action) = fx
        .post(
            &fx.alice,
            "/api/v0/scheduled",
            organization_schedule_body(&dot),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{action}");
    assert_eq!(action["principal_id"], dot);
    assert_eq!(action["agent_version"], 1);
    assert_eq!(action["model"], Value::Null);
    assert!(
        action["configuration_generation"]
            .as_str()
            .is_some_and(|generation| !generation.is_empty())
    );
    let runs = format!("/api/v0/scheduled/{}/runs", id(&action));
    assert_eq!(fx.get(&fx.alice, &runs).await.0, StatusCode::OK);
    for cookie in [&fx.bob, &fx.root] {
        let (status, denied) = fx.get(cookie, &runs).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{denied}");
        assert!(denied.get("runs").is_none());
    }
    sqlx::query("DELETE FROM organization_groups WHERE organization_id=(SELECT organization_id FROM dots WHERE principal_id=?)")
        .bind(&dot).execute(&fx.state.db).await.unwrap();
    assert_eq!(fx.get(&fx.alice, &runs).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn organization_schedule_rejects_employee_model_reuse_and_stale_edit() {
    let (fx, _upstream, _org, dot) = private_published_organization().await;
    for invalid in [
        json!({"model":"m"}),
        json!({"reuse_conversation":true}),
        json!({"agent_version":0}),
        json!({"agent_version":999}),
    ] {
        let mut body = organization_schedule_body(&dot);
        for (key, value) in invalid.as_object().unwrap() {
            body[key] = value.clone();
        }
        let (status, denied) = fx.post(&fx.alice, "/api/v0/scheduled", body).await;
        assert!(status.is_client_error(), "{status}: {denied}");
    }
    let (status, action) = fx
        .post(
            &fx.alice,
            "/api/v0/scheduled",
            organization_schedule_body(&dot),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{action}");
    let endpoint = format!("/api/v0/scheduled/{}/toggle", id(&action));
    let (status, disabled)=fx.post(&fx.alice,&endpoint,json!({"enabled":false,"expected_configuration_generation":action["configuration_generation"]})).await;
    assert_eq!(status, StatusCode::OK, "{disabled}");
    assert_eq!(
        disabled["configuration_generation"],
        action["configuration_generation"]
    );
    let (status, denied) = fx
        .post(
            &fx.alice,
            &endpoint,
            json!({"enabled":true,"expected_configuration_generation":"stale"}),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{denied}");
    let (status, denied) = fx.post(&fx.alice, &endpoint, json!({"enabled":true})).await;
    assert!(status.is_client_error(), "{status}: {denied}");
}

#[tokio::test]
async fn organization_retained_schedule_refuses_dot_deletion_until_source_deleted() {
    use aiplane_runtime::server::scheduled::{self, NewAction, ScheduleOwner};
    let (fx, _upstream, _org, dot) = private_published_organization().await;
    let action = scheduled::create(
        &fx.state.db,
        NewAction {
            owner: ScheduleOwner::Principal(dot.clone()),
            created_by: "alice".into(),
            dot_id: None,
            name: "Retained schedule".into(),
            prompt: "Review incidents".into(),
            model: None,
            agent_version: Some(1),
            cron: "0 9 * * *".into(),
            timezone: "Europe/Berlin".into(),
            tools_enabled: true,
            reuse_conversation: false,
            reuse_rounds: 5,
            next_run_at: None,
            linked_session_id: None,
        },
    )
    .await
    .unwrap();
    let endpoint = format!("/api/v0/agents/{dot}");
    let (status, denied) = fx
        .send(Some(&fx.alice), Method::DELETE, &endpoint, None)
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{denied}");
    assert!(
        aiplane_agents::db::agents::get(&fx.state.db, &dot)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(
        scheduled::delete(&fx.state.db, &action.owner, &action.id)
            .await
            .unwrap(),
        scheduled::DeleteOutcome::Deleted
    );
    assert_eq!(
        fx.send(Some(&fx.alice), Method::DELETE, &endpoint, None)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn organization_private_cancel_is_current_writer_structural_even_after_deactivation() {
    use wiremock::{
        Mock, ResponseTemplate,
        matchers::{method, path},
    };
    let (fx, upstream, _org, dot) = private_published_organization().await;
    upstream.reset().await;
    let delta = json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"cancel-time","type":"function","function":{"name":super::agents::TIME,"arguments":"{}"}}]}}]});
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            format!("data: {delta}\n\ndata: [DONE]\n\n"),
            "text/event-stream",
        ))
        .mount(&upstream)
        .await;
    let mut spec = super::agents::spec("Ask before reading time");
    spec["main"]["tool_resources"] = json!({(super::agents::TIME):{"permission":"always_ask"}});
    assert_eq!(fx.put_draft(&fx.alice, &dot, spec).await.0, StatusCode::OK);
    assert_eq!(fx.publish(&fx.alice, &dot).await.0, StatusCode::CREATED);
    let (status, started) = fx
        .post(
            &fx.alice,
            &format!("/api/v0/agents/{dot}/test/messages"),
            json!({"message":"Ask for time","source":"published"}),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{started}");
    let session = started["session_id"].as_str().unwrap();
    let turn = started["turn_id"].as_str().unwrap();
    let settled = crate::common::test_chat_settled(&fx.state, &fx.alice, &dot, session, turn).await;
    assert_eq!(settled["status"], "suspended", "{settled}");
    let endpoint = format!("/api/v0/agents/{dot}/test/{session}/cancel");
    let body = json!({"turn_id":turn});
    for cookie in [&fx.bob, &fx.root] {
        assert_eq!(
            fx.post(cookie, &endpoint, body.clone()).await.0,
            StatusCode::NOT_FOUND
        );
        assert!(
            session_core::db::get_suspension(&fx.state.db, turn)
                .await
                .unwrap()
                .is_some()
        );
    }
    sqlx::query("UPDATE dots SET active=0 WHERE principal_id=?")
        .bind(&dot)
        .execute(&fx.state.db)
        .await
        .unwrap();
    sqlx::query("UPDATE system_principals SET disabled_at='2026-10-11T09:00:00Z' WHERE id=?")
        .bind(&dot)
        .execute(&fx.state.db)
        .await
        .unwrap();
    let (status, cancelled) = fx.post(&fx.alice, &endpoint, body).await;
    assert_eq!(status, StatusCode::OK, "{cancelled}");
    assert_eq!(cancelled, json!({"ok":true}));
    assert!(
        session_core::db::get_suspension(&fx.state.db, turn)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        session_core::db::get_turn(&fx.state.db, session, turn)
            .await
            .unwrap()
            .unwrap()
            .status,
        session_core::db::TurnStatus::Cancelled
    );
}

#[tokio::test]
async fn organization_scheduled_pause_stop_cleans_stale_generation_and_unlocks_deletion() {
    use aiplane_runtime::server::scheduled::{self, NewAction, ScheduleOwner};
    use wiremock::{
        Mock, ResponseTemplate,
        matchers::{method, path},
    };
    let (fx, upstream, org, dot) = private_published_organization().await;
    upstream.reset().await;
    let delta = json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"scheduled-stop-time","type":"function","function":{"name":super::agents::TIME,"arguments":"{}"}}]}}]});
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            format!("data: {delta}\n\ndata: [DONE]\n\n"),
            "text/event-stream",
        ))
        .mount(&upstream)
        .await;
    let mut spec = super::agents::spec("Ask before reading time");
    spec["main"]["tool_resources"] = json!({(super::agents::TIME):{"permission":"always_ask"}});
    assert_eq!(fx.put_draft(&fx.alice, &dot, spec).await.0, StatusCode::OK);
    assert_eq!(fx.publish(&fx.alice, &dot).await.0, StatusCode::CREATED);
    let action = scheduled::create(
        &fx.state.db,
        NewAction {
            owner: ScheduleOwner::Principal(dot.clone()),
            created_by: "alice".into(),
            dot_id: None,
            name: "Pending schedule".into(),
            prompt: "Ask for time".into(),
            model: None,
            agent_version: Some(2),
            cron: "0 9 * * *".into(),
            timezone: "Europe/Berlin".into(),
            tools_enabled: true,
            reuse_conversation: false,
            reuse_rounds: 5,
            next_run_at: Some("2020-01-01T09:00:00Z".parse().unwrap()),
            linked_session_id: None,
        },
    )
    .await
    .unwrap();
    scheduled::worker::spawn(std::sync::Arc::new(fx.state.clone()));
    let mut captured = None;
    for _ in 0..200 {
        if let Some(run) = scheduled::list_runs(&fx.state.db, &action.id, 1)
            .await
            .unwrap()
            .first()
            && let (Some(session), Some(turn)) = (&run.session_id, &run.assistant_turn_id)
            && session_core::db::get_suspension(&fx.state.db, turn)
                .await
                .unwrap()
                .is_some()
        {
            captured = Some((session.clone(), turn.clone()));
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let (session, turn) = captured.expect("finite scheduled approval pause");
    let source = format!("/api/v0/scheduled/{}", action.id);
    assert_eq!(
        fx.send(Some(&fx.alice), Method::DELETE, &source, None)
            .await
            .0,
        StatusCode::CONFLICT
    );
    let endpoint = format!("/api/v0/agents/{dot}/test/{session}/cancel");
    assert_eq!(
        fx.post(&fx.alice, &endpoint, json!({"turn_id":"missing"}))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let ordinary = session_core::db::create_session(&fx.state.db, "alice")
        .await
        .unwrap();
    assert_eq!(
        fx.post(
            &fx.alice,
            &format!("/api/v0/agents/{dot}/test/{}/cancel", ordinary.id),
            json!({"turn_id":turn})
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    sqlx::query("DELETE FROM organization_groups WHERE organization_id=?")
        .bind(id(&org))
        .execute(&fx.state.db)
        .await
        .unwrap();
    assert_eq!(
        fx.post(&fx.alice, &endpoint, json!({"turn_id":turn}))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert!(
        session_core::db::get_suspension(&fx.state.db, &turn)
            .await
            .unwrap()
            .is_some()
    );
    sqlx::query(
        "INSERT INTO organization_groups(organization_id,gateway_group) VALUES(?,'managers')",
    )
    .bind(id(&org))
    .execute(&fx.state.db)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE scheduled_actions SET configuration_generation='edited-after-admission' WHERE id=?",
    )
    .bind(&action.id)
    .execute(&fx.state.db)
    .await
    .unwrap();
    let (status, cancelled) = fx.post(&fx.alice, &endpoint, json!({"turn_id":turn})).await;
    assert_eq!(status, StatusCode::OK, "{cancelled}");
    assert!(
        session_core::db::get_suspension(&fx.state.db, &turn)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        scheduled::list_runs(&fx.state.db, &action.id, 1)
            .await
            .unwrap()[0]
            .status
            .as_deref(),
        Some("error")
    );
    assert_eq!(
        fx.send(Some(&fx.alice), Method::DELETE, &source, None)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        fx.send(
            Some(&fx.alice),
            Method::DELETE,
            &format!("/api/v0/agents/{dot}"),
            None
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
}
