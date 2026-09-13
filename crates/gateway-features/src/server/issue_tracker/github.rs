// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Minimal GitHub REST client for the feedback widget.
//!
//! Files a user's feedback as a labelled issue, optionally embedding the
//! viewport screenshot and any images the reporter pasted in. GitHub has no
//! issue-attachment API, so each image is committed as a file on a dedicated
//! assets branch (created off the repo's default branch if missing) and
//! linked into the issue body via its raw URL — the approach the
//! `yachtlistings2` widget uses, ported here.
//!
//! Only the handful of endpoints the feature needs are wrapped; everything
//! goes through `reqwest` (the shared `AppState.http` client) with the three
//! headers GitHub requires (`User-Agent`, `Accept`, `X-GitHub-Api-Version`).

use serde_json::json;

use gateway_core::server::config::FeedbackConfig;

use super::markdown::{build_issue_body, issue_title};
use super::{IssueInput, IssueResult, TrackerError, issue_labels};

const ACCEPT: &str = "application/vnd.github+json";
const API_VERSION: &str = "2022-11-28";
const USER_AGENT: &str = "croit-llm-gateway-feedback";

/// File a feedback issue. Uploads the images first (best-effort — an upload
/// failure degrades to an issue without that image rather than failing the
/// whole submission), then creates the issue.
pub async fn create_issue(
    http: &reqwest::Client,
    cfg: &FeedbackConfig,
    input: IssueInput,
) -> Result<IssueResult, TrackerError> {
    let token = cfg.github_token().ok_or(TrackerError::NotConfigured)?;

    // Screenshot → committed asset → raw URL. Non-fatal: if any step fails we
    // log and proceed without the image.
    let screenshot_url = match input.screenshot_png_base64.as_deref() {
        Some(b64) if !b64.is_empty() => match upload_image(http, cfg, &token, b64).await {
            Ok(url) => Some(url),
            Err(err) => {
                tracing::warn!(error = %err, "feedback screenshot upload failed; filing without it");
                None
            }
        },
        _ => None,
    };
    let mut attachment_urls = Vec::new();
    for (i, b64) in input.attachments_png_base64.iter().enumerate() {
        if b64.is_empty() {
            continue;
        }
        match upload_image(http, cfg, &token, b64).await {
            Ok(url) => attachment_urls.push(url),
            Err(err) => {
                tracing::warn!(error = %err, index = i, "feedback attachment upload failed; skipping")
            }
        }
    }

    let body = build_issue_body(&input, screenshot_url.as_deref(), &attachment_urls);

    let url = format!(
        "{}/repos/{}/{}/issues",
        cfg.github_api_base.trim_end_matches('/'),
        cfg.github_owner,
        cfg.github_repo,
    );
    let resp = http
        .post(&url)
        .bearer_auth(&token)
        .header("Accept", ACCEPT)
        .header("X-GitHub-Api-Version", API_VERSION)
        .header("User-Agent", USER_AGENT)
        .json(&json!({
            "title": issue_title(&input.title),
            "body": body,
            "labels": issue_labels(cfg, &input),
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
    let number = v.get("number").and_then(|n| n.as_u64()).unwrap_or(0);
    let html_url = v
        .get("html_url")
        .and_then(|u| u.as_str())
        .unwrap_or("")
        .to_string();
    Ok(IssueResult {
        number,
        url: html_url,
    })
}

/// Commit one image to the assets branch and return a raw URL that renders
/// inline in the issue body.
async fn upload_image(
    http: &reqwest::Client,
    cfg: &FeedbackConfig,
    token: &str,
    png_base64: &str,
) -> Result<String, TrackerError> {
    // Cheap sanity check before spending two round trips. We don't decode
    // (GitHub takes the base64 verbatim and validates it anyway) — just
    // reject obvious garbage and oversize blobs. ~14MB of base64 ≈ ~10MB PNG,
    // far above any real viewport screenshot.
    const MAX_B64_LEN: usize = 14 * 1024 * 1024;
    if png_base64.len() > MAX_B64_LEN {
        return Err(TrackerError::Transport("image too large".into()));
    }
    if !png_base64
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='))
    {
        return Err(TrackerError::Transport("image is not valid base64".into()));
    }

    ensure_assets_branch(http, cfg, token).await?;

    // A stable, collision-free path. `uuid::simple` keeps it filesystem- and
    // URL-clean.
    let path = format!("screenshots/{}.png", uuid::Uuid::new_v4().simple());
    let url = format!(
        "{}/repos/{}/{}/contents/{}",
        cfg.github_api_base.trim_end_matches('/'),
        cfg.github_owner,
        cfg.github_repo,
        path,
    );
    let resp = http
        .put(&url)
        .bearer_auth(token)
        .header("Accept", ACCEPT)
        .header("X-GitHub-Api-Version", API_VERSION)
        .header("User-Agent", USER_AGENT)
        .json(&json!({
            "message": "feedback: add screenshot",
            "content": png_base64,
            "branch": cfg.assets_branch,
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
    // Prefer the canonical download URL GitHub hands back; fall back to the
    // web `/raw/` form (renders inline for viewers with repo access).
    let download = v
        .pointer("/content/download_url")
        .and_then(|u| u.as_str())
        .map(str::to_string);
    Ok(download.unwrap_or_else(|| {
        format!(
            "{}/{}/{}/raw/{}/{}",
            web_base(&cfg.github_api_base),
            cfg.github_owner,
            cfg.github_repo,
            cfg.assets_branch,
            path,
        )
    }))
}

/// Create the assets branch off the repo's default branch if it doesn't
/// exist yet. Idempotent: an existing branch (or a lost race that 422s on
/// "Reference already exists") is treated as success.
async fn ensure_assets_branch(
    http: &reqwest::Client,
    cfg: &FeedbackConfig,
    token: &str,
) -> Result<(), TrackerError> {
    let api = cfg.github_api_base.trim_end_matches('/');
    let (owner, repo) = (&cfg.github_owner, &cfg.github_repo);

    // Already there?
    let head_url = format!(
        "{api}/repos/{owner}/{repo}/git/ref/heads/{}",
        cfg.assets_branch
    );
    let head = http
        .get(&head_url)
        .bearer_auth(token)
        .header("Accept", ACCEPT)
        .header("X-GitHub-Api-Version", API_VERSION)
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .map_err(|e| TrackerError::Transport(e.to_string()))?;
    if head.status().is_success() {
        return Ok(());
    }

    // Default branch name → its head sha → create the new ref off it.
    let repo_url = format!("{api}/repos/{owner}/{repo}");
    let repo_v = get_json(http, &repo_url, token).await?;
    let default_branch = repo_v
        .get("default_branch")
        .and_then(|b| b.as_str())
        .unwrap_or("main");

    let base_ref_url = format!("{api}/repos/{owner}/{repo}/git/ref/heads/{default_branch}");
    let base_v = get_json(http, &base_ref_url, token).await?;
    let sha = base_v
        .pointer("/object/sha")
        .and_then(|s| s.as_str())
        .ok_or_else(|| TrackerError::Transport("default branch has no head sha".into()))?
        .to_string();

    let create_url = format!("{api}/repos/{owner}/{repo}/git/refs");
    let resp = http
        .post(&create_url)
        .bearer_auth(token)
        .header("Accept", ACCEPT)
        .header("X-GitHub-Api-Version", API_VERSION)
        .header("User-Agent", USER_AGENT)
        .json(&json!({
            "ref": format!("refs/heads/{}", cfg.assets_branch),
            "sha": sha,
        }))
        .send()
        .await
        .map_err(|e| TrackerError::Transport(e.to_string()))?;
    let status = resp.status();
    if status.is_success() || status.as_u16() == 422 {
        // 422 == "Reference already exists" (a concurrent first submit won).
        return Ok(());
    }
    let body = resp.text().await.unwrap_or_default();
    Err(TrackerError::Api {
        status: status.as_u16(),
        body: body.chars().take(400).collect(),
    })
}

async fn get_json(
    http: &reqwest::Client,
    url: &str,
    token: &str,
) -> Result<serde_json::Value, TrackerError> {
    let resp = http
        .get(url)
        .bearer_auth(token)
        .header("Accept", ACCEPT)
        .header("X-GitHub-Api-Version", API_VERSION)
        .header("User-Agent", USER_AGENT)
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
    serde_json::from_slice(&bytes).map_err(|e| TrackerError::Transport(e.to_string()))
}

/// Derive the web host from the API base. `api.github.com` → `github.com`;
/// an Enterprise `https://host/api/v3` → `https://host`.
fn web_base(api_base: &str) -> String {
    let trimmed = api_base.trim_end_matches('/');
    if trimmed == "https://api.github.com" {
        return "https://github.com".to_string();
    }
    trimmed
        .strip_suffix("/api/v3")
        .unwrap_or(trimmed)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_base_maps_public_and_enterprise() {
        assert_eq!(web_base("https://api.github.com"), "https://github.com");
        assert_eq!(web_base("https://api.github.com/"), "https://github.com");
        assert_eq!(
            web_base("https://ghe.example.com/api/v3"),
            "https://ghe.example.com"
        );
    }
}
