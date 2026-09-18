// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Where the feedback widget files its issues.
//!
//! Two backends, one shape. The widget collects the same report either way —
//! four text fields, a priority, an annotated screenshot, extra pasted
//! images, and a pile of browser diagnostics — so the *body* is built once
//! ([`markdown::build_issue_body`]) and only the transport differs:
//!
//!   - [`github`] commits each image to an orphan assets branch and links its
//!     raw URL, because the GitHub REST API has no issue-attachment endpoint.
//!   - [`gitlab`] POSTs each image to the project's `uploads` endpoint and
//!     uses the markdown snippet GitLab hands back — the same call the
//!     `croit.erp` widget makes.
//!
//! [`create_feedback_issue`] dispatches on `FeedbackConfig::provider`, so the
//! handler never learns which tracker is live.

use aiplane_core::server::config::{FeedbackConfig, FeedbackProvider};

pub mod github;
pub mod gitlab;
pub mod markdown;

/// What the caller wants filed. Text fields are already validated/trimmed by
/// the handler; the image fields are raw standard-base64 PNG bytes (no
/// `data:` prefix).
pub struct IssueInput {
    pub title: String,
    pub description: String,
    pub business_value: String,
    pub acceptance_criteria: String,
    /// One of `low` / `medium` / `high`.
    pub priority: String,
    /// Email of the signed-in user who filed it (for attribution in the body).
    pub reporter_email: String,
    /// The annotated viewport capture, if the reporter kept one.
    pub screenshot_png_base64: Option<String>,
    /// Extra images the reporter pasted or dropped into the dialog.
    pub attachments_png_base64: Vec<String>,
    /// Free-form diagnostics (url, viewport, user agent, console + network
    /// logs, chat tail) rendered as collapsible sections. A JSON object;
    /// non-object values are ignored.
    pub system_info: serde_json::Value,
}

/// The created issue, surfaced back to the browser so it can toast a link.
#[derive(Debug)]
pub struct IssueResult {
    /// The number the tracker shows users: GitHub's `number`, GitLab's `iid`.
    pub number: u64,
    pub url: String,
}

#[derive(Debug, thiserror::Error)]
pub enum TrackerError {
    #[error("feedback is not configured")]
    NotConfigured,
    #[error("issue tracker request failed: {0}")]
    Transport(String),
    #[error("issue tracker api error ({status}): {body}")]
    Api { status: u16, body: String },
}

/// File a feedback issue in whichever tracker the operator selected.
pub async fn create_feedback_issue(
    http: &reqwest::Client,
    cfg: &FeedbackConfig,
    input: IssueInput,
) -> Result<IssueResult, TrackerError> {
    if !cfg.is_configured() {
        return Err(TrackerError::NotConfigured);
    }
    match cfg.provider() {
        FeedbackProvider::Github => github::create_issue(http, cfg, input).await,
        FeedbackProvider::Gitlab => gitlab::create_issue(http, cfg, input).await,
    }
}

/// Labels every issue carries: the operator's list plus the priority, which
/// both trackers spell as a scoped-ish label so a board can filter on it.
pub(crate) fn issue_labels(cfg: &FeedbackConfig, input: &IssueInput) -> Vec<String> {
    let mut labels = cfg.labels.clone();
    labels.push(format!(
        "priority:{}",
        markdown::normalise_priority(&input.priority)
    ));
    labels
}

/// Decode one of the base64 image fields into bytes for a multipart upload.
///
/// Deliberately strict and bounded: the same ~14 MB ceiling the GitHub path
/// applies before spending a round trip, and a hard reject on anything that
/// isn't standard base64 rather than a lossy best-effort decode.
pub(crate) fn decode_png(b64: &str) -> Result<Vec<u8>, TrackerError> {
    use base64::Engine as _;

    const MAX_B64_LEN: usize = 14 * 1024 * 1024;
    if b64.len() > MAX_B64_LEN {
        return Err(TrackerError::Transport("image too large".into()));
    }
    base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| TrackerError::Transport(format!("image is not valid base64: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(provider: &str) -> FeedbackConfig {
        FeedbackConfig {
            provider: provider.into(),
            github_owner: String::new(),
            github_repo: String::new(),
            github_token: None,
            github_token_env: None,
            labels: vec!["feedback".into()],
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
            title: "t".into(),
            description: "d".into(),
            business_value: String::new(),
            acceptance_criteria: String::new(),
            priority: "HIGH".into(),
            reporter_email: String::new(),
            screenshot_png_base64: None,
            attachments_png_base64: Vec::new(),
            system_info: serde_json::Value::Null,
        }
    }

    #[test]
    fn labels_carry_the_normalised_priority() {
        let mut c = cfg("github");
        c.labels = vec!["feedback".into(), "ui".into()];
        assert_eq!(
            issue_labels(&c, &input()),
            vec!["feedback", "ui", "priority:high"]
        );
    }

    #[test]
    fn decode_png_round_trips_and_rejects_garbage() {
        use base64::Engine as _;
        let raw = b"\x89PNG\r\n\x1a\n";
        let b64 = base64::engine::general_purpose::STANDARD.encode(raw);
        assert_eq!(decode_png(&b64).expect("decodes"), raw);
        assert!(decode_png("not base64!!").is_err());
    }

    // A half-configured block must keep the widget hidden for the SELECTED
    // provider — never fall through to the other one's credentials.
    #[tokio::test]
    async fn dispatch_refuses_when_the_selected_provider_is_incomplete() {
        let mut c = cfg("gitlab");
        // GitHub fully configured, GitLab not: still refused.
        c.github_owner = "croit".into();
        c.github_repo = "aiplane".into();
        c.github_token = Some("ghp_x".into());
        let http = reqwest::Client::new();
        let err = create_feedback_issue(&http, &c, input())
            .await
            .expect_err("gitlab is not configured");
        assert!(matches!(err, TrackerError::NotConfigured));
    }
}
