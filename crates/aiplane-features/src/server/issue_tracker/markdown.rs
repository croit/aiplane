// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The issue body both trackers post.
//!
//! GitHub and GitLab both render GitHub-flavoured markdown with `<details>`
//! support, so one builder serves both: the four form sections, the images,
//! then the collapsible diagnostics the browser collected (system info, the
//! chat tail on a chat page, console logs, network activity). Only the image
//! *links* differ per tracker, so they arrive here already resolved.

use super::IssueInput;

/// Normalise the submitted priority to the three values the label and the
/// body agree on. Anything else is `medium` — the form's default, and the
/// only safe reading of a value the client made up.
pub fn normalise_priority(p: &str) -> &str {
    match p.trim().to_ascii_lowercase().as_str() {
        "low" => "low",
        "high" => "high",
        _ => "medium",
    }
}

/// Issue titles are capped by both trackers (GitHub at 256 chars); keep
/// margin and trim.
pub fn issue_title(title: &str) -> String {
    let t = title.trim();
    if t.chars().count() <= 200 {
        t.to_string()
    } else {
        t.chars().take(200).collect()
    }
}

/// Build the issue body markdown. Mirrors the source widgets' layout:
/// description, business value, acceptance criteria, screenshot, the pasted
/// attachments, then the collapsible diagnostics.
///
/// `screenshot_url` and `attachment_urls` are whatever the tracker's upload
/// returned: a raw URL for GitHub, GitLab's own `![](/uploads/…)` markdown
/// snippet. A GitLab snippet is already a complete image embed, so it is
/// emitted verbatim rather than wrapped in another `![]()`.
pub fn build_issue_body(
    input: &IssueInput,
    screenshot_url: Option<&str>,
    attachment_urls: &[String],
) -> String {
    let mut out = String::new();

    if !input.description.trim().is_empty() {
        out.push_str(input.description.trim());
        out.push_str("\n\n");
    }
    if !input.business_value.trim().is_empty() {
        out.push_str("## Why / Business value\n\n");
        out.push_str(input.business_value.trim());
        out.push_str("\n\n");
    }
    if !input.acceptance_criteria.trim().is_empty() {
        out.push_str("## Acceptance criteria\n\n");
        out.push_str(input.acceptance_criteria.trim());
        out.push_str("\n\n");
    }
    if let Some(url) = screenshot_url {
        out.push_str("## Screenshot\n\n");
        out.push_str(&image_embed("screenshot", url));
        out.push_str("\n\n");
    }
    if !attachment_urls.is_empty() {
        out.push_str("## Attachments\n\n");
        for (i, url) in attachment_urls.iter().enumerate() {
            out.push_str(&image_embed(&format!("attachment {}", i + 1), url));
            out.push_str("\n\n");
        }
    }

    out.push_str("---\n\n");
    if !input.reporter_email.trim().is_empty() {
        out.push_str(&format!(
            "Reported by **{}** · priority **{}**\n\n",
            input.reporter_email.trim(),
            normalise_priority(&input.priority),
        ));
    }

    if let Some(map) = input.system_info.as_object()
        && !map.is_empty()
    {
        // Scalar fields → a compact table. The structured diagnostics
        // (console/network/chat) get their own collapsible sections below.
        out.push_str("<details>\n<summary>System information</summary>\n\n");
        out.push_str("| Field | Value |\n|---|---|\n");
        for (k, v) in map {
            if matches!(k.as_str(), "console_logs" | "network_logs" | "chat") {
                continue;
            }
            let val = match v {
                serde_json::Value::String(s) => s.clone(),
                // e.g. `allowed_tools` is a string array → comma-join.
                serde_json::Value::Array(a) => a
                    .iter()
                    .filter_map(|x| x.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                other => other.to_string(),
            };
            out.push_str(&format!(
                "| {} | {} |\n",
                md_cell(k),
                md_cell(&val.chars().take(400).collect::<String>()),
            ));
        }
        out.push_str("\n</details>\n\n");

        if let Some(chat) = map.get("chat").and_then(|c| c.as_object()) {
            let sid = chat
                .get("session_id")
                .and_then(|s| s.as_str())
                .unwrap_or("");
            let tail = chat
                .get("transcript_tail")
                .and_then(|s| s.as_str())
                .unwrap_or("");
            if !tail.is_empty() {
                out.push_str(&format!(
                    "<details>\n<summary>Chat context (session {})</summary>\n\n```\n{}\n```\n\n</details>\n\n",
                    md_cell(sid),
                    fence_safe(tail),
                ));
            }
        }

        if let Some(logs) = map.get("console_logs").and_then(|l| l.as_array())
            && !logs.is_empty()
        {
            out.push_str(&render_console_logs(logs));
        }
        if let Some(logs) = map.get("network_logs").and_then(|l| l.as_array())
            && !logs.is_empty()
        {
            out.push_str(&render_network_logs(logs));
        }
    }

    out
}

/// Render an uploaded image. GitLab's `uploads` endpoint already answers with
/// a complete markdown embed (`![file](/uploads/…/file.png)`) and wrapping
/// that in another `![]()` would print the literal text instead of the image,
/// so a value that already looks like one is passed through.
fn image_embed(alt: &str, url_or_markdown: &str) -> String {
    let trimmed = url_or_markdown.trim();
    if trimmed.starts_with("![") {
        trimmed.to_string()
    } else {
        format!("![{alt}]({trimmed})")
    }
}

/// Escape the pipe + newlines so a value can't break the markdown table.
fn md_cell(s: &str) -> String {
    s.replace('|', "\\|").replace(['\n', '\r'], " ")
}

/// Neutralise a closing code fence inside fenced content.
fn fence_safe(s: &str) -> String {
    s.replace("```", "ʼʼʼ")
}

/// Collapsible console-log table. Renders the last 50 entries (the client
/// already caps the buffer at 100) so a chatty page can't bloat the issue.
fn render_console_logs(logs: &[serde_json::Value]) -> String {
    let mut out = format!(
        "<details>\n<summary>Console logs ({})</summary>\n\n| Time | Level | Message |\n|---|---|---|\n",
        logs.len()
    );
    for entry in logs.iter().rev().take(50).rev() {
        let o = match entry.as_object() {
            Some(o) => o,
            None => continue,
        };
        let time = short_time(o.get("timestamp").and_then(|t| t.as_str()).unwrap_or(""));
        let level = o.get("level").and_then(|l| l.as_str()).unwrap_or("log");
        let message = o
            .get("args")
            .and_then(|a| a.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default();
        out.push_str(&format!(
            "| {} | {} | {} |\n",
            md_cell(&time),
            md_cell(level),
            md_cell(&message.chars().take(300).collect::<String>()),
        ));
    }
    out.push_str("\n</details>\n\n");
    out
}

/// Collapsible network-activity table.
fn render_network_logs(logs: &[serde_json::Value]) -> String {
    let mut out = format!(
        "<details>\n<summary>Network activity ({})</summary>\n\n| Time | Method | Status | ms | URL |\n|---|---|---|---|---|\n",
        logs.len()
    );
    for entry in logs.iter().rev().take(50).rev() {
        let o = match entry.as_object() {
            Some(o) => o,
            None => continue,
        };
        let time = short_time(o.get("timestamp").and_then(|t| t.as_str()).unwrap_or(""));
        let method = o.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let status = o
            .get("status")
            .and_then(serde_json::Value::as_i64)
            .map(|s| s.to_string())
            .unwrap_or_default();
        let dur = o
            .get("duration")
            .and_then(serde_json::Value::as_f64)
            .map(|d| format!("{d:.0}"))
            .unwrap_or_default();
        let url = o.get("url").and_then(|u| u.as_str()).unwrap_or("");
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            md_cell(&time),
            md_cell(method),
            md_cell(&status),
            md_cell(&dur),
            md_cell(&url.chars().take(200).collect::<String>()),
        ));
    }
    out.push_str("\n</details>\n\n");
    out
}

/// Keep just the `HH:MM:SS` of an ISO timestamp for compact tables.
fn short_time(iso: &str) -> String {
    iso.split('T')
        .nth(1)
        .map(|t| {
            t.trim_end_matches('Z')
                .split('.')
                .next()
                .unwrap_or(t)
                .to_string()
        })
        .unwrap_or_else(|| iso.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(system_info: serde_json::Value) -> IssueInput {
        IssueInput {
            title: "t".into(),
            description: "It broke".into(),
            business_value: "Saves time".into(),
            acceptance_criteria: "- works".into(),
            priority: "high".into(),
            reporter_email: "a@b.c".into(),
            screenshot_png_base64: None,
            attachments_png_base64: Vec::new(),
            system_info,
        }
    }

    #[test]
    fn priority_normalises() {
        assert_eq!(normalise_priority("HIGH"), "high");
        assert_eq!(normalise_priority(" low "), "low");
        assert_eq!(normalise_priority("garbage"), "medium");
        assert_eq!(normalise_priority(""), "medium");
    }

    #[test]
    fn body_includes_sections_and_escapes_table() {
        let body = build_issue_body(
            &input(serde_json::json!({ "url": "https://x/y|z", "dpr": 2 })),
            Some("https://img/x.png"),
            &[],
        );
        assert!(body.contains("It broke"));
        assert!(body.contains("## Why / Business value"));
        assert!(body.contains("## Acceptance criteria"));
        assert!(body.contains("![screenshot](https://img/x.png)"));
        assert!(body.contains("Reported by **a@b.c**"));
        assert!(body.contains("https://x/y\\|z")); // pipe escaped
        assert!(body.contains("| dpr | 2 |"));
    }

    // GitLab's uploads endpoint answers with a ready-made markdown embed;
    // wrapping it again would print the literal `![...](...)` text.
    #[test]
    fn gitlab_upload_markdown_is_passed_through_verbatim() {
        let body = build_issue_body(
            &input(serde_json::Value::Null),
            Some("![shot.png](/uploads/abc/shot.png)"),
            &["![a.png](/uploads/def/a.png)".into()],
        );
        assert!(body.contains("![shot.png](/uploads/abc/shot.png)"));
        assert!(!body.contains("![screenshot](!["));
        assert!(body.contains("## Attachments"));
        assert!(body.contains("![a.png](/uploads/def/a.png)"));
    }

    #[test]
    fn plain_attachment_urls_get_an_alt_text() {
        let body = build_issue_body(
            &input(serde_json::Value::Null),
            None,
            &["https://img/a.png".into(), "https://img/b.png".into()],
        );
        assert!(body.contains("![attachment 1](https://img/a.png)"));
        assert!(body.contains("![attachment 2](https://img/b.png)"));
    }

    #[test]
    fn body_renders_console_and_network_sections() {
        let body = build_issue_body(
            &input(serde_json::json!({
                "url": "https://x/y",
                "allowed_tools": ["search_web", "fetch_url"],
                "console_logs": [{ "timestamp": "2026-06-23T10:00:01.123Z", "level": "error", "args": ["boom", "x"] }],
                "network_logs": [{ "timestamp": "2026-06-23T10:00:02.000Z", "method": "POST", "url": "/api/v0/x", "status": 500, "duration": 42.7 }],
                "chat": { "session_id": "abc", "transcript_tail": "hello world" },
            })),
            None,
            &[],
        );
        assert!(body.contains("<summary>Console logs (1)</summary>"));
        assert!(body.contains("| 10:00:01 | error | boom x |"));
        assert!(body.contains("<summary>Network activity (1)</summary>"));
        assert!(body.contains("| 10:00:02 | POST | 500 | 43 | /api/v0/x |"));
        assert!(body.contains("Chat context (session abc)"));
        assert!(body.contains("hello world"));
        // allowed_tools is a string array → comma-joined in the table.
        assert!(body.contains("search_web, fetch_url"));
        // structured keys are not dumped as raw rows.
        assert!(!body.contains("| console_logs |"));
    }

    #[test]
    fn issue_title_is_capped() {
        let long: String = "x".repeat(300);
        assert_eq!(issue_title(&long).chars().count(), 200);
        assert_eq!(issue_title("  hi  "), "hi");
    }
}
