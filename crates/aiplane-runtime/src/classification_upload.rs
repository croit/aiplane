//! What the files a person attached to a message say about its level.
//!
//! Runs once per turn, before any model sees the conversation: each attachment
//! of the message is read for its digital markings, its visible stamp, the
//! patterns and word lists of the data classes and, through the same detector
//! as typed text, the System One rules. Whatever is found raises the
//! conversation and the strongest class action decides the turn, exactly as
//! for a typed message ([`apply_findings`]).
//!
//! The scheme being inactive (one level) means nothing is fetched, parsed or
//! asked.

use aiplane_core::server::classification::{
    Action, ClassificationMode, Scheme, level_id, marking_rules,
};
use aiplane_features::server::chat_attachments::{self, AttachmentError, AttachmentRef};
use aiplane_features::server::classification::{
    Mapping, detection_text, read_markings, resolve, stamp_matches,
};

use crate::classification_detector::{
    Decision, Finding, apply_findings, static_findings, system_one_findings,
};
use crate::server::AppState;
use crate::server::tools::ToolContext;

/// A file larger than this is not downloaded to be read; it counts as a file
/// whose marking could not be read.
const MAX_INSPECTED_BYTES: u64 = 64 * 1024 * 1024;
/// How much text of one file the patterns and word lists look through.
const PATTERN_TEXT_BYTES: usize = 1024 * 1024;
/// How much text of all files together the System One model is asked about.
const SYSTEM_ONE_TEXT_BYTES: usize = 32 * 1024;

fn unreadable_finding(scheme: &Scheme, unreadable_level: &str) -> Finding {
    Finding {
        class_id: None,
        level_id: scheme.resolve(unreadable_level).id.clone(),
        cause: "marking:unreadable".into(),
        action: Action::Raise,
    }
}

/// The findings of one file's bytes that need no model.
fn file_findings(
    scheme: &Scheme,
    mapping: &Mapping<'_>,
    terms: &crate::classification_detector::TermMatchers,
    filename: &str,
    mime: &str,
    bytes: &[u8],
    above_rank: u32,
) -> (Vec<Finding>, Option<String>) {
    let mut findings = Vec::new();
    let markings = read_markings(filename, mime, bytes);
    let resolved = resolve(&markings, filename, mapping);
    if let Some(level) = resolved.level_id {
        for cause in resolved.causes {
            findings.push(Finding {
                class_id: None,
                level_id: level.clone(),
                cause: cause.as_str().to_string(),
                action: Action::Raise,
            });
        }
    }
    let text = detection_text(filename, mime, bytes, PATTERN_TEXT_BYTES);
    if let Some(text) = &text {
        for level in stamp_matches(text, mapping.rules) {
            findings.push(Finding {
                class_id: None,
                level_id: level,
                cause: "marking:stamp".into(),
                action: Action::Raise,
            });
        }
        findings.extend(static_findings(scheme, terms, text, above_rank));
    }
    (findings, text)
}

struct Inspected {
    findings: Vec<Finding>,
    texts: Vec<(String, String)>,
}

async fn inspect(
    state: &AppState,
    s3: &aiplane_core::server::config::S3Config,
    scheme: &Scheme,
    attachments: &[AttachmentRef],
    above_rank: u32,
) -> Result<Inspected, String> {
    let rules = marking_rules(&state.db)
        .await
        .map_err(|error| format!("reading the classification markings: {error}"))?;
    let config = state.config();
    let policy = &config.classification;
    let terms = state.term_matchers();
    let mut out = Inspected {
        findings: Vec::new(),
        texts: Vec::new(),
    };
    for attachment in attachments {
        if attachment.size > MAX_INSPECTED_BYTES {
            out.findings
                .push(unreadable_finding(scheme, &policy.unreadable_level));
            continue;
        }
        let fetched =
            match chat_attachments::fetch(s3, &attachment.turn_id, &attachment.filename).await {
                Ok(fetched) => fetched,
                Err(AttachmentError::NotFound(_)) => continue,
                Err(error) => {
                    return Err(format!(
                        "reading the attachment `{}` to classify it: {error}",
                        attachment.filename
                    ));
                }
            };
        let (scheme_owned, rules_owned, policy_owned, terms_owned, attachment_owned) = (
            scheme.clone(),
            rules.clone(),
            policy.clone(),
            terms.clone(),
            attachment.clone(),
        );
        let (findings, text) = tokio::task::spawn_blocking(move || {
            let mapping = Mapping {
                scheme: &scheme_owned,
                rules: &rules_owned,
                policy: &policy_owned,
            };
            file_findings(
                &scheme_owned,
                &mapping,
                &terms_owned,
                &attachment_owned.filename,
                &fetched.mime,
                &fetched.bytes,
                above_rank,
            )
        })
        .await
        .map_err(|error| format!("reading the attachment `{}`: {error}", attachment.filename))?;
        out.findings.extend(findings);
        if let Some(text) = text {
            out.texts.push((attachment.filename.clone(), text));
        }
    }
    Ok(out)
}

fn system_one_text(texts: &[(String, String)]) -> String {
    let mut out = String::new();
    for (filename, text) in texts {
        let room = SYSTEM_ONE_TEXT_BYTES.saturating_sub(out.len());
        if room == 0 {
            break;
        }
        out.push_str(&format!("[file {filename}]\n"));
        let take: String = text
            .char_indices()
            .take_while(|(at, _)| *at < room)
            .map(|(_, ch)| ch)
            .collect();
        out.push_str(&take);
        out.push_str("\n\n");
    }
    out
}

/// Inspect the attachments of the turn's message and apply what they show.
/// `Ok(Decision::Allow)` when the scheme is inactive, the turn is an agent's,
/// or the message carries no file.
pub(crate) async fn evaluate_attachments(
    state: &AppState,
    context: &ToolContext,
) -> Result<Decision, String> {
    let scheme = state.scheme();
    let (true, None, Some(session_id), Some(s3)) = (
        scheme.is_active(),
        context.agent.as_ref(),
        context.session_id.as_deref(),
        context.s3.as_deref(),
    ) else {
        return Ok(Decision::Allow);
    };
    let attachments = chat_attachments::round_attachments(&state.db, session_id)
        .await
        .map_err(|error| format!("listing the message's attachments to classify them: {error}"))?;
    if attachments.is_empty() {
        return Ok(Decision::Allow);
    }
    let stored = level_id(&state.db, session_id)
        .await
        .map_err(|error| format!("reading the conversation's level: {error}"))?;
    let rank = scheme.level_or_lowest(stored.as_deref()).rank;
    let mode = state.config().classification.mode;

    let inspected = match inspect(state, s3, &scheme, &attachments, rank).await {
        Ok(inspected) => inspected,
        Err(error) if mode == ClassificationMode::Monitor => {
            tracing::warn!(error = %error, "attachments could not be classified in monitor mode");
            return Ok(Decision::Allow);
        }
        Err(error) => return Err(error),
    };
    let mut findings = inspected.findings;
    let text = system_one_text(&inspected.texts);
    if !text.trim().is_empty() {
        findings.extend(system_one_findings(state, &text, rank).await?);
    }
    tracing::info!(
        session_id,
        files = attachments.len(),
        causes = ?findings.iter().map(|f| f.cause.as_str()).collect::<Vec<_>>(),
        mode = mode.as_str(),
        "data classification inspected the message's attachments"
    );
    if mode == ClassificationMode::Monitor || findings.is_empty() {
        return Ok(Decision::Allow);
    }
    Ok(apply_findings(state, &scheme, context, &findings).await)
}

/// What the saved scheme makes of one file or text, without a conversation.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DryRun {
    pub level_id: String,
    pub causes: Vec<String>,
    pub classes: Vec<String>,
}

/// The administrator's test: the findings an upload and a typed message would
/// give for `file` (name, mime, bytes) and `text`, judged against the lowest
/// level so every class counts. Nothing is stored. The System One model is
/// asked only when one is configured.
pub async fn dry_run(
    state: &AppState,
    file: Option<(&str, &str, &[u8])>,
    text: Option<&str>,
) -> Result<DryRun, String> {
    let scheme = state.scheme();
    let config = state.config();
    let terms = state.term_matchers();
    let mut findings = Vec::new();
    let mut texts: Vec<String> = Vec::new();
    if let Some((filename, mime, bytes)) = file {
        let rules = marking_rules(&state.db)
            .await
            .map_err(|error| format!("reading the classification markings: {error}"))?;
        let mapping = Mapping {
            scheme: &scheme,
            rules: &rules,
            policy: &config.classification,
        };
        let (file_findings, file_text) =
            file_findings(&scheme, &mapping, &terms, filename, mime, bytes, 0);
        findings.extend(file_findings);
        texts.extend(file_text);
    }
    if let Some(text) = text.filter(|text| !text.trim().is_empty()) {
        findings.extend(static_findings(&scheme, &terms, text, 0));
        texts.push(text.to_owned());
    }
    let joined = texts.join("\n\n");
    if !config.classification.system_one_model.trim().is_empty() && !joined.trim().is_empty() {
        let bounded: String = joined
            .char_indices()
            .take_while(|(at, _)| *at < SYSTEM_ONE_TEXT_BYTES)
            .map(|(_, ch)| ch)
            .collect();
        findings.extend(system_one_findings(state, &bounded, 0).await?);
    }
    let level_id = findings
        .iter()
        .filter_map(|finding| scheme.level(&finding.level_id))
        .max_by_key(|level| level.rank)
        .unwrap_or_else(|| scheme.lowest())
        .id
        .clone();
    let mut causes: Vec<String> = Vec::new();
    let mut classes: Vec<String> = Vec::new();
    for finding in &findings {
        if !causes.contains(&finding.cause) {
            causes.push(finding.cause.clone());
        }
        if let Some(class) = &finding.class_id
            && !classes.contains(class)
        {
            classes.push(class.clone());
        }
    }
    Ok(DryRun {
        level_id,
        causes,
        classes,
    })
}
