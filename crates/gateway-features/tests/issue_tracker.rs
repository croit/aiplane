// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! HTTP-level tests for the two feedback trackers.
//!
//! The unit tests next to each client cover URL building and markdown; what
//! they cannot cover is the shape of the requests that actually leave the
//! process — the auth header GitLab needs, the comma-joined label string it
//! needs, the multipart upload, and the two-step commit GitHub needs instead.
//! Those are the parts that fail against a real instance while every in-crate
//! test still passes, so they are exercised here against a mock server.
//!
//! The GitLab half in particular has no live counterpart in this repo's CI
//! (no instance, no token), which makes pinning the wire format here the only
//! guard it has.

use gateway_core::server::config::FeedbackConfig;
use gateway_features::server::issue_tracker::{IssueInput, create_feedback_issue};
use serde_json::json;
use wiremock::matchers::{body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

/// A 1×1 PNG, base64 — enough for the upload paths to have real bytes.
const PNG_B64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAAC0lEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

fn base_config() -> FeedbackConfig {
    FeedbackConfig {
        provider: "github".into(),
        github_owner: "croit".into(),
        github_repo: "llm-gateway".into(),
        github_token: Some("ghp_test".into()),
        github_token_env: None,
        labels: vec!["feedback".into(), "from-gateway".into()],
        assets_branch: "feedback-assets".into(),
        extraction_model: None,
        voice_model: None,
        github_api_base: "https://api.github.com".into(),
        gitlab_url: "https://gitlab.example.com".into(),
        gitlab_project_id: String::new(),
        gitlab_token: None,
        gitlab_token_env: None,
    }
}

fn input() -> IssueInput {
    IssueInput {
        title: "Something broke".into(),
        description: "It threw on save".into(),
        business_value: "Blocks invoicing".into(),
        acceptance_criteria: "- saving works".into(),
        priority: "high".into(),
        reporter_email: "reporter@example.com".into(),
        screenshot_png_base64: Some(PNG_B64.into()),
        attachments_png_base64: vec![PNG_B64.into()],
        system_info: json!({
            "url": "https://gateway.example.com/admin/settings",
            "console_logs": [
                { "timestamp": "2026-06-23T10:00:01.123Z", "level": "error", "args": ["boom"] }
            ],
        }),
    }
}

#[tokio::test]
async fn gitlab_uploads_each_image_then_files_the_issue() {
    let server = MockServer::start().await;

    // GitLab's attachment store. Answers with the ready-made markdown embed
    // the issue body is supposed to use verbatim.
    Mock::given(method("POST"))
        .and(path("/api/v4/projects/group%2Fsub%2Fproj/uploads"))
        // The access token goes in PRIVATE-TOKEN. Sent as a bearer instead,
        // a real instance answers 401 with nothing useful in it.
        .and(header("PRIVATE-TOKEN", "glpat-test"))
        .respond_with(move |req: &Request| {
            // Multipart, with the file part actually carrying bytes.
            let body = String::from_utf8_lossy(&req.body).to_string();
            assert!(
                body.contains("name=\"file\""),
                "upload is not a multipart file part: {body}"
            );
            assert!(
                body.contains("filename=\"feedback-screenshot.png\"")
                    || body.contains("filename=\"feedback-attachment-1.png\""),
                "upload has no recognised filename: {body}"
            );
            ResponseTemplate::new(201).set_body_json(json!({
                "alt": "shot",
                "url": "/uploads/abc/shot.png",
                "markdown": "![shot](/uploads/abc/shot.png)",
            }))
        })
        .expect(2) // the screenshot and the one attachment
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/api/v4/projects/group%2Fsub%2Fproj/issues"))
        .and(header("PRIVATE-TOKEN", "glpat-test"))
        // Labels are ONE comma-separated string for GitLab, not a JSON array.
        .and(body_string_contains(
            r#""labels":"feedback,from-gateway,priority:high""#,
        ))
        .respond_with(move |req: &Request| {
            let body: serde_json::Value = serde_json::from_slice(&req.body).expect("json body");
            let description = body["description"].as_str().unwrap_or_default();
            // GitLab's own embed is emitted verbatim — wrapping it in another
            // `![]()` would print the markup instead of the image.
            assert!(
                description.contains("![shot](/uploads/abc/shot.png)"),
                "screenshot embed missing: {description}"
            );
            assert!(
                !description.contains("![screenshot](!["),
                "embed double-wrapped"
            );
            assert!(
                description.contains("## Attachments"),
                "attachments section missing"
            );
            assert!(description.contains("Blocks invoicing"));
            assert!(description.contains("<summary>Console logs (1)</summary>"));
            // GitLab calls the issue body `description`, not `body`.
            assert!(body.get("body").is_none());
            ResponseTemplate::new(201).set_body_json(json!({
                "id": 900001,
                "iid": 42,
                "web_url": "https://gitlab.example.com/group/sub/proj/-/issues/42",
            }))
        })
        .expect(1)
        .mount(&server)
        .await;

    let mut cfg = base_config();
    cfg.provider = "gitlab".into();
    cfg.gitlab_url = server.uri();
    cfg.gitlab_project_id = "group/sub/proj".into();
    cfg.gitlab_token = Some("glpat-test".into());

    let result = create_feedback_issue(&reqwest::Client::new(), &cfg, input())
        .await
        .expect("issue filed");
    // `iid` is the number humans quote, not the instance-wide `id`.
    assert_eq!(result.number, 42);
    assert_eq!(
        result.url,
        "https://gitlab.example.com/group/sub/proj/-/issues/42"
    );
}

#[tokio::test]
async fn gitlab_files_the_issue_even_when_an_upload_fails() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v4/projects/1234/uploads"))
        .respond_with(ResponseTemplate::new(413).set_body_string("too big"))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v4/projects/1234/issues"))
        .respond_with(move |req: &Request| {
            let body: serde_json::Value = serde_json::from_slice(&req.body).expect("json body");
            let description = body["description"].as_str().unwrap_or_default();
            // The report survives without its images rather than being lost.
            assert!(!description.contains("## Screenshot"));
            assert!(description.contains("It threw on save"));
            ResponseTemplate::new(201).set_body_json(json!({ "iid": 7, "web_url": "u" }))
        })
        .expect(1)
        .mount(&server)
        .await;

    let mut cfg = base_config();
    cfg.provider = "gitlab".into();
    cfg.gitlab_url = server.uri();
    cfg.gitlab_project_id = "1234".into();
    cfg.gitlab_token = Some("glpat-test".into());

    let result = create_feedback_issue(&reqwest::Client::new(), &cfg, input())
        .await
        .expect("issue filed without images");
    assert_eq!(result.number, 7);
}

#[tokio::test]
async fn github_commits_each_image_to_the_assets_branch_then_files_the_issue() {
    let server = MockServer::start().await;

    // The branch already exists, so no creation round trip.
    Mock::given(method("GET"))
        .and(path(
            "/repos/croit/llm-gateway/git/ref/heads/feedback-assets",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "ref": "ok" })))
        .mount(&server)
        .await;

    // GitHub has no attachment API — images become commits, base64 inline.
    Mock::given(method("PUT"))
        .and(header("Accept", "application/vnd.github+json"))
        .and(header("X-GitHub-Api-Version", "2022-11-28"))
        .and(body_string_contains(PNG_B64))
        .and(body_string_contains(r#""branch":"feedback-assets""#))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "content": { "download_url": "https://raw.example.com/shot.png" }
        })))
        .expect(2)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/repos/croit/llm-gateway/issues"))
        .respond_with(move |req: &Request| {
            let body: serde_json::Value = serde_json::from_slice(&req.body).expect("json body");
            // Labels are a JSON array for GitHub — the mirror image of GitLab.
            assert_eq!(
                body["labels"],
                json!(["feedback", "from-gateway", "priority:high"])
            );
            let issue_body = body["body"].as_str().unwrap_or_default();
            assert!(issue_body.contains("![screenshot](https://raw.example.com/shot.png)"));
            assert!(issue_body.contains("Reported by **reporter@example.com**"));
            // GitHub calls the body `body`, not `description`.
            assert!(body.get("description").is_none());
            ResponseTemplate::new(201).set_body_json(json!({
                "number": 99,
                "html_url": "https://github.com/croit/llm-gateway/issues/99",
            }))
        })
        .expect(1)
        .mount(&server)
        .await;

    let mut cfg = base_config();
    cfg.github_api_base = server.uri();

    let result = create_feedback_issue(&reqwest::Client::new(), &cfg, input())
        .await
        .expect("issue filed");
    assert_eq!(result.number, 99);
    assert_eq!(result.url, "https://github.com/croit/llm-gateway/issues/99");
}

/// The selector decides, full stop. A deployment that has switched to GitLab
/// must never reach the GitHub repo it still holds credentials for — a
/// mis-routed customer report would go unnoticed for weeks.
#[tokio::test]
async fn a_gitlab_selection_never_touches_the_github_api() {
    let server = MockServer::start().await;
    // No mounted mocks at all: any request to this server is a failure, and
    // wiremock's own "no matching mock" 404 makes the call fail loudly.
    let mut cfg = base_config();
    cfg.provider = "gitlab".into();
    cfg.github_api_base = server.uri();
    cfg.gitlab_url = server.uri();
    cfg.gitlab_project_id = "1234".into();
    cfg.gitlab_token = Some("glpat-test".into());

    let _ = create_feedback_issue(&reqwest::Client::new(), &cfg, input()).await;

    let requests = server
        .received_requests()
        .await
        .expect("mock server records requests");
    assert!(
        requests
            .iter()
            .all(|r| r.url.path().starts_with("/api/v4/")),
        "a gitlab-selected deployment called a non-GitLab path: {:?}",
        requests.iter().map(|r| r.url.path()).collect::<Vec<_>>()
    );
}
