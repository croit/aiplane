// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use super::agents::{Fx, fixture, person};
use rama::http::{Method, StatusCode};
use serde_json::{Value, json};

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
async fn organization_configuration_reuses_grants_and_publish_but_refuses_execution_and_shared_credentials()
 {
    use aiplane_core::server::db::mcp_catalog::{self, AuthKind, ConnectorInput, Scope};
    let fx = fixture().await;
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
            json!({"message":"Actual test request"}),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{refused}");
    assert_eq!(
        refused["error"]["type"],
        "organization_execution_unavailable"
    );
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
