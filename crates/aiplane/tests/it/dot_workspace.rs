// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use rama::http::{Method, StatusCode};
use serde_json::json;

use super::agents::fixture;

#[tokio::test]
async fn personal_dot_workspace_reports_missing_storage_without_inventing_entries() {
    let fx = fixture().await;
    let (_, created) = fx
        .post(&fx.alice, "/api/v0/dots", json!({"name":"Files"}))
        .await;
    let id = created["dot"]["id"].as_str().unwrap();
    let url = format!("/api/v0/dots/{id}/workspace");
    let (status, view) = fx.get(&fx.alice, &url).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["dot_id"], id);
    assert_eq!(view["dot_name"], "Files");
    assert_eq!(view["available"], false);
    assert!(view["entries"].is_null());
    assert!(view["busy"].is_null());
    assert!(view["next_cursor"].is_null());
    assert_eq!(fx.get(&fx.bob, &url).await.0, StatusCode::NOT_FOUND);
    assert_eq!(
        fx.send(None, Method::GET, &url, None).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        fx.get(&fx.alice, "/api/v0/dots/missing/workspace").await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn personal_dot_workspace_gates_each_operation_before_storage_access() {
    let fx = fixture().await;
    let (_, created) = fx
        .post(&fx.alice, "/api/v0/dots", json!({"name":"Private files"}))
        .await;
    let id = created["dot"]["id"].as_str().unwrap();
    for (method, suffix, body) in [
        (Method::GET, "file?path=report.txt", None),
        (Method::PUT, "file?path=report.txt", Some(json!("bytes"))),
        (Method::POST, "folder", Some(json!({"path":"project"}))),
        (Method::DELETE, "entry?path=project", None),
    ] {
        let url = format!("/api/v0/dots/{id}/workspace/{suffix}");
        assert_eq!(
            fx.send(Some(&fx.bob), method.clone(), &url, body.clone())
                .await
                .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            fx.send(None, method.clone(), &url, body.clone()).await.0,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            fx.send(Some(&fx.alice), method, &url, body).await.0,
            StatusCode::SERVICE_UNAVAILABLE
        );
    }
}

#[tokio::test]
async fn personal_dot_workspace_remains_owner_readable_when_inactive() {
    let fx = fixture().await;
    let (_, created) = fx
        .post(&fx.alice, "/api/v0/dots", json!({"name":"Inactive files"}))
        .await;
    let id = created["dot"]["id"].as_str().unwrap();
    let (status, body) = fx
        .send(
            Some(&fx.alice),
            Method::PUT,
            &format!("/api/v0/dots/{id}/active"),
            Some(json!({"active":false})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        fx.get(&fx.alice, &format!("/api/v0/dots/{id}/workspace"))
            .await
            .0,
        StatusCode::OK
    );
}

use std::sync::Arc;
use std::time::Duration;

use rama::Service;
use rama::http::{Body, HeaderMap, Request, header};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

use super::agents::Fx;
use crate::common;

async fn configured_workspace() -> (Fx, MockServer, String) {
    let fx = fixture().await;
    let runner = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/healthz"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"status":"ok","workspace":true,"egress":false})),
        )
        .mount(&runner)
        .await;
    let client = aiplane_runtime::server::tools::sandbox::SandboxClient::new(
        Arc::new(aiplane_core::server::config::SandboxConfig {
            enabled: true,
            runner_url: runner.uri(),
            timeout_secs: 5,
            max_artifact_bytes: 1024,
        }),
        aiplane_runtime::server::state::RuntimeSettings::new_handle("https://gateway.example"),
    );
    let app = std::ops::Deref::deref(&fx.state).clone();
    let _installed = app.with_sandbox_client(client);
    let (status, created) = fx
        .post(
            &fx.alice,
            "/api/v0/dots",
            json!({"name":"Actual workspace"}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let dot = created["dot"]["id"].as_str().unwrap().to_owned();
    (fx, runner, dot)
}

async fn raw_workspace_request(
    fx: &Fx,
    cookie: Option<&str>,
    method: Method,
    url: &str,
    bytes: Vec<u8>,
    declared: Option<usize>,
) -> (StatusCode, HeaderMap, Vec<u8>) {
    let mut request = Request::builder().method(method).uri(url);
    if let Some(cookie) = cookie {
        request = request.header(header::COOKIE, format!("id={cookie}"));
    }
    if let Some(length) = declared {
        request = request.header(header::CONTENT_LENGTH, length);
    }
    let response = common::app(fx.state.clone())
        .serve(request.body(Body::from(bytes)).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    (status, headers, common::read_body(response).await.to_vec())
}

async fn workspace_rpcs(runner: &MockServer) -> Vec<serde_json::Value> {
    runner
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|request| request.url.path() == "/workspace")
        .map(|request| serde_json::from_slice(&request.body).unwrap())
        .collect()
}

#[tokio::test]
async fn configured_personal_workspace_lists_actual_entries_and_preserves_bound_cursor() {
    let (fx, runner, dot) = configured_workspace().await;
    Mock::given(method("POST")).and(path("/workspace"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "entries":[{"name":"Actual-ß.txt","kind":"file","size_bytes":7,"modified_at":"2026-10-11T12:00:00Z"}],
            "next_cursor":"Actual-ß.txt","busy":true
        }))).mount(&runner).await;
    let url = format!("/api/v0/dots/{dot}/workspace?path=project");
    let (status, view) = fx.get(&fx.alice, &url).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["available"], true);
    assert_eq!(view["busy"], true);
    assert_eq!(view["entries"][0]["name"], "Actual-ß.txt");
    assert_eq!(view["entries"][0]["size_bytes"], 7);
    assert_eq!(view["entries"][0]["modified_at"], "2026-10-11T12:00:00Z");
    let cursor = view["next_cursor"].as_str().unwrap();
    let query = serde_urlencoded::to_string([("path", "project"), ("cursor", cursor)]).unwrap();
    assert_eq!(
        fx.get(&fx.alice, &format!("/api/v0/dots/{dot}/workspace?{query}"))
            .await
            .0,
        StatusCode::OK
    );
    let requests = workspace_rpcs(&runner).await;
    assert_eq!(
        requests[0],
        json!({"workspace":{"owner_user_id":"alice","dot_id":dot},"operation":{"kind":"list","path":"project","after":null}})
    );
    assert_eq!(requests[1]["operation"]["after"], "Actual-ß.txt");
    assert_eq!(
        fx.get(
            &fx.alice,
            &format!("/api/v0/dots/{dot}/workspace?cursor=%%%")
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let wrong = serde_urlencoded::to_string([("path", "other"), ("cursor", cursor)]).unwrap();
    assert_eq!(
        fx.get(&fx.alice, &format!("/api/v0/dots/{dot}/workspace?{wrong}"))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(workspace_rpcs(&runner).await.len(), 2);
}

#[tokio::test]
async fn configured_personal_workspace_downloads_raw_private_attachments() {
    let (fx, runner, dot) = configured_workspace().await;
    let bytes = vec![0, 0xff, b'\n', b'<', b'>'];
    Mock::given(method("POST"))
        .and(path("/workspace"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(bytes.clone()))
        .mount(&runner)
        .await;
    let query = serde_urlencoded::to_string([("path", "project/Actual-ß.txt")]).unwrap();
    let (status, headers, body) = raw_workspace_request(
        &fx,
        Some(&fx.alice),
        Method::GET,
        &format!("/api/v0/dots/{dot}/workspace/file?{query}"),
        vec![],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, bytes);
    assert_eq!(headers[header::CONTENT_TYPE], "application/octet-stream");
    assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    assert!(
        headers[header::CONTENT_DISPOSITION]
            .to_str()
            .unwrap()
            .contains("filename*=UTF-8''Actual-%C3%9F.txt")
    );
    assert_eq!(
        workspace_rpcs(&runner).await[0],
        json!({"workspace":{"owner_user_id":"alice","dot_id":dot},"operation":{"kind":"read","path":"project/Actual-ß.txt"}})
    );
}

#[tokio::test]
async fn configured_personal_workspace_mutations_send_exact_trusted_owner_paths_and_bytes() {
    let (fx, runner, dot) = configured_workspace().await;
    Mock::given(method("POST"))
        .and(path("/workspace"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&runner)
        .await;
    let bytes = vec![0, 0xff, b'\n', b'x'];
    let query = serde_urlencoded::to_string([("path", "project/Actual-ß.bin")]).unwrap();
    let (status, _, body) = raw_workspace_request(
        &fx,
        Some(&fx.alice),
        Method::PUT,
        &format!("/api/v0/dots/{dot}/workspace/file?{query}"),
        bytes.clone(),
        Some(bytes.len()),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(body.is_empty());
    assert_eq!(
        fx.post(
            &fx.alice,
            &format!("/api/v0/dots/{dot}/workspace/folder"),
            json!({"path":"project/Actual-ß"})
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        fx.send(
            Some(&fx.alice),
            Method::DELETE,
            &format!("/api/v0/dots/{dot}/workspace/entry?{query}"),
            None
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let requests = workspace_rpcs(&runner).await;
    assert_eq!(requests.len(), 3);
    assert_eq!(
        requests[0],
        json!({"workspace":{"owner_user_id":"alice","dot_id":dot},"operation":{"kind":"write","path":"project/Actual-ß.bin","content_b64":shared::b64::encode(&bytes)}})
    );
    assert_eq!(
        requests[1]["operation"],
        json!({"kind":"create_folder","path":"project/Actual-ß"})
    );
    assert_eq!(
        requests[2]["operation"],
        json!({"kind":"remove","path":"project/Actual-ß.bin"})
    );
    assert!(
        requests
            .iter()
            .all(|request| request["workspace"] == json!({"owner_user_id":"alice","dot_id":dot}))
    );
}

#[tokio::test]
async fn configured_personal_workspace_maps_runner_errors_and_refuses_foreign_access_before_runner()
{
    let (fx, runner, dot) = configured_workspace().await;
    let url = format!("/api/v0/dots/{dot}/workspace/file?path=actual.txt");
    for (runner_status, expected) in [
        (409, StatusCode::CONFLICT),
        (413, StatusCode::PAYLOAD_TOO_LARGE),
        (503, StatusCode::SERVICE_UNAVAILABLE),
    ] {
        let mock = Mock::given(method("POST"))
            .and(path("/workspace"))
            .respond_with(
                ResponseTemplate::new(runner_status)
                    .set_body_json(json!({"error":"operator-only-detail"})),
            )
            .mount_as_scoped(&runner)
            .await;
        let (status, body) = fx.get(&fx.alice, &url).await;
        assert_eq!(status, expected, "{body}");
        assert!(!body.to_string().contains("operator-only-detail"));
        drop(mock);
    }
    let before = runner.received_requests().await.unwrap().len();
    for (method, suffix, body) in [
        (Method::GET, "", None),
        (Method::GET, "/file?path=actual.txt", None),
        (
            Method::PUT,
            "/file?path=actual.txt",
            Some(json!("untrusted")),
        ),
        (Method::POST, "/folder", Some(json!({"path":"actual"}))),
        (Method::DELETE, "/entry?path=actual", None),
    ] {
        let url = format!("/api/v0/dots/{dot}/workspace{suffix}");
        assert_eq!(
            fx.send(Some(&fx.bob), method.clone(), &url, body.clone())
                .await
                .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            fx.send(None, method, &url, body).await.0,
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(runner.received_requests().await.unwrap().len(), before);
}

#[tokio::test]
async fn configured_personal_workspace_rejects_oversized_declared_upload_before_runner() {
    let (fx, runner, dot) = configured_workspace().await;
    Mock::given(method("POST"))
        .and(path("/workspace"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&runner)
        .await;
    let (status, _, _) = raw_workspace_request(
        &fx,
        Some(&fx.alice),
        Method::PUT,
        &format!("/api/v0/dots/{dot}/workspace/file?path=too-large"),
        b"bounded".to_vec(),
        Some(shared::sandbox::STAGE_TOTAL_MAX_BYTES + 1),
    )
    .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert!(runner.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn configured_personal_workspace_rechecks_deleted_owner_after_waiting_for_runner() {
    let (fx, runner, dot) = configured_workspace().await;
    let entered = Arc::new(tokio::sync::Notify::new());
    let reached = entered.clone();
    Mock::given(method("POST"))
        .and(path("/workspace"))
        .respond_with(move |_: &wiremock::Request| {
            reached.notify_one();
            ResponseTemplate::new(200)
                .set_delay(Duration::from_millis(1200))
                .set_body_bytes(b"private-runner-bytes".to_vec())
        })
        .mount(&runner)
        .await;
    let state = fx.state.clone();
    let cookie = fx.alice.clone();
    let url = format!("/api/v0/dots/{dot}/workspace/file?path=actual.txt");
    let task = tokio::spawn(async move {
        common::app(state)
            .serve(
                Request::builder()
                    .method(Method::GET)
                    .uri(url)
                    .header(header::COOKIE, format!("id={cookie}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    });
    tokio::time::timeout(Duration::from_secs(2), entered.notified())
        .await
        .unwrap();
    let removed = tokio::time::timeout(
        Duration::from_millis(500),
        aiplane_agents::db::agents::delete(&fx.state.db, &dot, "alice"),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(removed);
    let response = tokio::time::timeout(Duration::from_secs(3), task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = common::read_body(response).await;
    assert!(!String::from_utf8_lossy(&body).contains("private-runner-bytes"));
    assert_eq!(workspace_rpcs(&runner).await.len(), 1);
}
