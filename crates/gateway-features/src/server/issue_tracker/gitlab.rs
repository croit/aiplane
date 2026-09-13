// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Minimal GitLab REST (v4) client for the feedback widget.
//!
//! A port of `croit.erp`'s `gitlab-service.ts`, which is the deployment this
//! feature has to interoperate with. Two endpoints:
//!
//!   - `POST /projects/:id/uploads` — multipart image upload. Unlike GitHub,
//!     GitLab has a first-class attachment store, so there is no assets
//!     branch and no commit: the response's `markdown` field is a ready-made
//!     `![file](/uploads/…)` embed that the issue body uses verbatim.
//!   - `POST /projects/:id/issues` — create the issue, labels comma-joined.
//!
//! Auth is the `PRIVATE-TOKEN` header (personal, project or group access
//! token with the `api` scope), not a bearer — GitLab accepts `Bearer` only
//! for OAuth tokens, and an access token sent that way fails with a 401 that
//! says nothing useful.

use serde_json::json;

use gateway_core::server::config::FeedbackConfig;

use super::markdown::{build_issue_body, issue_title};
use super::{IssueInput, IssueResult, TrackerError, decode_png, issue_labels};

/// Header GitLab authenticates access tokens with.
const TOKEN_HEADER: &str = "PRIVATE-TOKEN";

pub async fn create_issue(
    http: &reqwest::Client,
    cfg: &FeedbackConfig,
    input: IssueInput,
) -> Result<IssueResult, TrackerError> {
    let token = cfg.gitlab_token().ok_or(TrackerError::NotConfigured)?;

    // Images first. Best-effort, exactly like the GitHub path: a failed
    // upload costs the image, never the report.
    let screenshot_url = match input.screenshot_png_base64.as_deref() {
        Some(b64) if !b64.is_empty() => {
            match upload_image(http, cfg, &token, b64, "feedback-screenshot.png").await {
                Ok(md) => Some(md),
                Err(err) => {
                    tracing::warn!(error = %err, "feedback screenshot upload failed; filing without it");
                    None
                }
            }
        }
        _ => None,
    };
    let mut attachment_urls = Vec::new();
    for (i, b64) in input.attachments_png_base64.iter().enumerate() {
        if b64.is_empty() {
            continue;
        }
        let name = format!("feedback-attachment-{}.png", i + 1);
        match upload_image(http, cfg, &token, b64, &name).await {
            Ok(md) => attachment_urls.push(md),
            Err(err) => {
                tracing::warn!(error = %err, index = i, "feedback attachment upload failed; skipping")
            }
        }
    }

    let body = build_issue_body(&input, screenshot_url.as_deref(), &attachment_urls);

    let resp = http
        .post(project_url(cfg, "issues"))
        .header(TOKEN_HEADER, &token)
        .json(&json!({
            "title": issue_title(&input.title),
            "description": body,
            // GitLab takes labels as one comma-separated string, not a list.
            "labels": issue_labels(cfg, &input).join(","),
        }))
        .send()
        .await
        .map_err(|e| TrackerError::Transport(e.to_string()))?;

    let status = resp.status();
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| TrackerError::Transport(e.to_string()))?;
    if !status.is_success() {
        return Err(TrackerError::Api {
            status: status.as_u16(),
            body: String::from_utf8_lossy(&bytes).chars().take(400).collect(),
        });
    }
    let v: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| TrackerError::Transport(e.to_string()))?;
    // `iid` is the per-project number humans quote ("#42"); `id` is the
    // instance-wide surrogate key and means nothing to the reporter.
    let number = v.get("iid").and_then(|n| n.as_u64()).unwrap_or(0);
    let url = v
        .get("web_url")
        .and_then(|u| u.as_str())
        .unwrap_or("")
        .to_string();
    Ok(IssueResult { number, url })
}

/// Upload one PNG to the project's attachment store; returns GitLab's own
/// markdown embed for it.
async fn upload_image(
    http: &reqwest::Client,
    cfg: &FeedbackConfig,
    token: &str,
    png_base64: &str,
    filename: &str,
) -> Result<String, TrackerError> {
    let bytes = decode_png(png_base64)?;
    let part = reqwest::multipart::Part::bytes(bytes)
        .file_name(filename.to_string())
        .mime_str("image/png")
        .map_err(|e| TrackerError::Transport(e.to_string()))?;
    let form = reqwest::multipart::Form::new().part("file", part);

    let resp = http
        .post(project_url(cfg, "uploads"))
        .header(TOKEN_HEADER, token)
        .multipart(form)
        .send()
        .await
        .map_err(|e| TrackerError::Transport(e.to_string()))?;
    let status = resp.status();
    let body = resp
        .bytes()
        .await
        .map_err(|e| TrackerError::Transport(e.to_string()))?;
    if !status.is_success() {
        return Err(TrackerError::Api {
            status: status.as_u16(),
            body: String::from_utf8_lossy(&body).chars().take(400).collect(),
        });
    }
    let v: serde_json::Value =
        serde_json::from_slice(&body).map_err(|e| TrackerError::Transport(e.to_string()))?;
    // `markdown` is the embed; `url` is the bare path, used as a fallback so
    // an older GitLab that omits `markdown` still produces a working link.
    if let Some(md) = v.get("markdown").and_then(|m| m.as_str())
        && !md.is_empty()
    {
        return Ok(md.to_string());
    }
    v.get("url")
        .and_then(|u| u.as_str())
        .filter(|u| !u.is_empty())
        .map(|u| format!("{}{}", cfg.gitlab_url.trim_end_matches('/'), u))
        .ok_or_else(|| TrackerError::Transport("upload returned no url".into()))
}

/// `{base}/api/v4/projects/{id}/{leaf}`.
///
/// The project id may be a numeric id or a `group/project` path, and GitLab
/// requires the path form percent-encoded *including* its slashes — so the
/// encoding is done here rather than left to the URL parser, which would
/// keep them as path separators and 404.
fn project_url(cfg: &FeedbackConfig, leaf: &str) -> String {
    format!(
        "{}/api/v4/projects/{}/{leaf}",
        cfg.gitlab_url.trim_end_matches('/'),
        encode_project_id(cfg.gitlab_project_id.trim()),
    )
}

/// Percent-encode a project path for use as a single URL path segment.
/// Mirrors `encodeURIComponent`: unreserved characters pass through, every
/// other byte becomes `%XX`.
fn encode_project_id(id: &str) -> String {
    let mut out = String::with_capacity(id.len());
    for b in id.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(url: &str, project: &str) -> FeedbackConfig {
        FeedbackConfig {
            provider: "gitlab".into(),
            github_owner: String::new(),
            github_repo: String::new(),
            github_token: None,
            github_token_env: None,
            labels: vec!["feedback".into()],
            assets_branch: "feedback-assets".into(),
            extraction_model: None,
            voice_model: None,
            github_api_base: "https://api.github.com".into(),
            gitlab_url: url.into(),
            gitlab_project_id: project.into(),
            gitlab_token: Some("glpat-x".into()),
            gitlab_token_env: None,
        }
    }

    #[test]
    fn numeric_project_ids_pass_through() {
        assert_eq!(
            project_url(&cfg("https://gitlab.example.com", "1234"), "issues"),
            "https://gitlab.example.com/api/v4/projects/1234/issues"
        );
    }

    // The slashes of a group path MUST be encoded, or GitLab reads them as
    // path separators and answers 404.
    #[test]
    fn path_project_ids_are_fully_encoded() {
        assert_eq!(
            project_url(
                &cfg("https://gitlab.example.com/", "group/sub/proj"),
                "uploads"
            ),
            "https://gitlab.example.com/api/v4/projects/group%2Fsub%2Fproj/uploads"
        );
    }

    #[test]
    fn encode_project_id_matches_encode_uri_component() {
        assert_eq!(encode_project_id("a-b_c.d~e"), "a-b_c.d~e");
        assert_eq!(encode_project_id("a b/c"), "a%20b%2Fc");
    }
}
