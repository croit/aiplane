// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use rama::http::{Method, StatusCode};
use serde_json::{Value, json};

use super::agents::{Fx, fixture};

async fn create(fx: &Fx, cookie: &str, name: &str) -> String {
    let (status, body) = fx.post(cookie, "/api/v0/dots", json!({"name":name})).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["dot"]["id"].as_str().unwrap().to_string()
}

fn peer<'a>(body: &'a Value, id: &str) -> &'a Value {
    body["peers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["dot"]["id"] == id)
        .unwrap()
}

#[tokio::test]
async fn personal_dot_communication_inventory_is_private_and_uses_actual_peers() {
    let fx = fixture().await;
    let caller = create(&fx, &fx.alice, "Harald").await;
    let receiver = create(&fx, &fx.alice, "Clara").await;
    let foreign = create(&fx, &fx.bob, "Foreign").await;
    let path = format!("/api/v0/dots/{caller}/communication");
    let (status, body) = fx.get(&fx.alice, &path).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["dot"]["id"], caller);
    assert_eq!(body["peers"].as_array().unwrap().len(), 1);
    assert_eq!(peer(&body, &receiver)["dot"]["name"], "Clara");
    assert_eq!(
        peer(&body, &receiver)["outgoing"],
        json!({"outbound_enabled":false,"inbound_enabled":false,"generation":null})
    );
    assert_eq!(
        peer(&body, &receiver)["incoming"],
        peer(&body, &receiver)["outgoing"]
    );
    assert_eq!(fx.get(&fx.bob, &path).await.0, StatusCode::NOT_FOUND);
    assert_eq!(
        fx.send(None, Method::GET, &path, None).await.0,
        StatusCode::UNAUTHORIZED
    );
    for target in [&caller, &foreign] {
        let (status, _) = fx.send(Some(&fx.alice), Method::PUT, &path, Some(json!({"peer_id":target,"direction":"outbound","enabled":true,"expected_generation":null}))).await;
        assert_eq!(
            status,
            if target == &caller {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::NOT_FOUND
            }
        );
    }
    aiplane_agents::db::agents::set_personal_dot_active(&fx.state.db, &receiver, "alice", false, 4)
        .await
        .unwrap();
    let (status, body) = fx.get(&fx.alice, &path).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(peer(&body, &receiver)["dot"]["active"], false);
}

#[tokio::test]
async fn personal_dot_communication_saves_one_direction_and_rejects_stale_noops() {
    let fx = fixture().await;
    let caller = create(&fx, &fx.alice, "Harald").await;
    let receiver = create(&fx, &fx.alice, "Clara").await;
    let path = format!("/api/v0/dots/{caller}/communication");
    let receiver_path = format!("/api/v0/dots/{receiver}/communication");
    let body = json!({"peer_id":receiver,"direction":"outbound","enabled":true,"expected_generation":null});
    let (status, first) = fx
        .send(Some(&fx.alice), Method::PUT, &path, Some(body.clone()))
        .await;
    assert_eq!(status, StatusCode::OK, "{first}");
    let first_edge = peer(&first, &receiver)["outgoing"].clone();
    let generation = first_edge["generation"].as_str().unwrap();
    assert_eq!(first_edge["outbound_enabled"], true);
    assert_eq!(first_edge["inbound_enabled"], false);
    let (status, second) = fx.send(Some(&fx.alice), Method::PUT, &receiver_path, Some(json!({"peer_id":caller,"direction":"inbound","enabled":true,"expected_generation":generation}))).await;
    assert_eq!(status, StatusCode::OK, "{second}");
    let accepted = &peer(&second, &caller)["incoming"];
    assert_eq!(accepted["outbound_enabled"], true);
    assert_eq!(accepted["inbound_enabled"], true);
    assert_ne!(accepted["generation"], generation);
    assert_eq!(
        peer(&second, &caller)["outgoing"]["generation"],
        Value::Null
    );
    let stale = json!({"peer_id":receiver,"direction":"outbound","enabled":true,"expected_generation":generation});
    assert_eq!(
        fx.send(Some(&fx.alice), Method::PUT, &path, Some(stale))
            .await
            .0,
        StatusCode::CONFLICT
    );
    let unchanged = json!({"peer_id":receiver,"direction":"outbound","enabled":true,"expected_generation":accepted["generation"]});
    let (status, same) = fx
        .send(Some(&fx.alice), Method::PUT, &path, Some(unchanged))
        .await;
    assert_eq!(status, StatusCode::OK, "{same}");
    assert_eq!(&peer(&same, &receiver)["outgoing"], accepted);
    assert_eq!(
        fx.send(Some(&fx.bob), Method::PUT, &path, Some(body))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn personal_dot_communication_revocation_is_allowed_inactive_and_requires_current_management()
{
    let fx = fixture().await;
    let caller = create(&fx, &fx.alice, "Harald").await;
    let receiver = create(&fx, &fx.alice, "Clara").await;
    let path = format!("/api/v0/dots/{caller}/communication");
    let (status, initial) = fx.send(Some(&fx.alice), Method::PUT, &path, Some(json!({"peer_id":receiver,"direction":"outbound","enabled":true,"expected_generation":null}))).await;
    assert_eq!(status, StatusCode::OK, "{initial}");
    let generation = &peer(&initial, &receiver)["outgoing"]["generation"];
    aiplane_agents::db::agents::set_personal_dot_active(&fx.state.db, &receiver, "alice", false, 4)
        .await
        .unwrap();
    let (status, revoked) = fx.send(Some(&fx.alice), Method::PUT, &path, Some(json!({"peer_id":receiver,"direction":"outbound","enabled":false,"expected_generation":generation}))).await;
    assert_eq!(status, StatusCode::OK, "{revoked}");
    let generation = &peer(&revoked, &receiver)["outgoing"]["generation"];
    assert_eq!(fx.send(Some(&fx.alice), Method::PUT, &path, Some(json!({"peer_id":receiver,"direction":"outbound","enabled":true,"expected_generation":generation}))).await.0, StatusCode::CONFLICT);
    let (status, _) = fx.send(Some(&fx.root), Method::PUT, "/api/v0/admin/groups", Some(json!({"name":"managers","oidc_values":["managers"],"can_manage_agents":true,"can_manage_dots":false}))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fx.get(&fx.alice, &path).await.0, StatusCode::OK);
    assert_eq!(fx.send(Some(&fx.alice), Method::PUT, &path, Some(json!({"peer_id":receiver,"direction":"outbound","enabled":false,"expected_generation":generation}))).await.0, StatusCode::FORBIDDEN);
}
