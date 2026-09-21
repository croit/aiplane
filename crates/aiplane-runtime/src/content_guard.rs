use std::time::Duration;

use serde_json::{Value, json};

use aiplane_core::server::config::{ContentGuardMode, ContentGuardPolicy};
use aiplane_core::server::upstreams::{Compliance, PoolAccess, PoolKind};

use crate::server::AppState;
use crate::server::tools::ChatFeedback;
use crate::server::tools::feedback::AskReply;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Allow,
    Confirm,
    Deny,
}

pub async fn confirm_in_chat(feedback: Option<&ChatFeedback>, turn_id: Option<&str>) -> bool {
    let (Some(feedback), Some(turn_id)) = (feedback, turn_id) else {
        return false;
    };
    if feedback.broadcast.receiver_count() == 0 {
        return false;
    }
    let reply = feedback.ask_hub.register(turn_id);
    let _ = feedback
        .broadcast
        .send(session_core::workers::TurnUpdate::Prompt(
            std::sync::Arc::new(session_core::workers::ToolPromptEvent::Show(
                session_core::workers::ToolPrompt {
                    turn_id: turn_id.to_string(),
                    kind: session_core::workers::ToolPromptKind::ContentGuard,
                    question: String::new(),
                    options: vec![
                        session_core::workers::PromptOption {
                            label: "approve".into(),
                            description: None,
                            preview: None,
                        },
                        session_core::workers::PromptOption {
                            label: "deny".into(),
                            description: None,
                            preview: None,
                        },
                    ],
                    header: None,
                    multi_select: false,
                },
            )),
        ));
    let approved = matches!(
        tokio::time::timeout(Duration::from_secs(180), reply).await,
        Ok(Ok(AskReply::Answered { choices, .. })) if choices.iter().any(|choice| choice == "approve")
    );
    feedback.ask_hub.cancel(turn_id);
    let _ = feedback
        .broadcast
        .send(session_core::workers::TurnUpdate::Prompt(
            std::sync::Arc::new(session_core::workers::ToolPromptEvent::Hide {
                turn_id: turn_id.to_string(),
            }),
        ));
    approved
}

pub async fn evaluate(
    state: &AppState,
    request: &Value,
    compliance: Compliance,
) -> Result<Action, String> {
    let mode = state.config().content_guard.mode;
    match evaluate_checked(state, request, compliance).await {
        Ok(action) => Ok(action),
        Err(error) if mode == ContentGuardMode::Monitor => {
            tracing::warn!(error = %error, "content guard could not evaluate request in monitor mode");
            Ok(Action::Allow)
        }
        Err(error) => Err(error),
    }
}

pub async fn evaluate_for_model(
    state: &AppState,
    request: &Value,
    model: &str,
    access: &PoolAccess,
) -> Result<Action, String> {
    let Some(compliance) = state
        .upstreams
        .models_with_compliance_for_kind_for(PoolKind::Chat, access)
        .into_iter()
        .find_map(|(id, compliance)| (id == model).then_some(compliance))
    else {
        return Ok(Action::Allow);
    };
    evaluate(state, request, compliance).await
}

async fn evaluate_checked(
    state: &AppState,
    request: &Value,
    compliance: Compliance,
) -> Result<Action, String> {
    let config = state.config();
    let guard = &config.content_guard;
    if !guard.enabled || compliance.is_all_clear() {
        return Ok(Action::Allow);
    }
    if guard.model.trim().is_empty() {
        return Err("content guard is enabled but no System One guard model is configured".into());
    }
    let (needs_gdpr, needs_nda) = required_checks(compliance);
    let mut questions = serde_json::Map::new();
    if needs_gdpr {
        questions.insert("gdpr".into(), json!({
            "type": "noul",
            "instructions": "Does this content contain personal data that must not be sent to a non-GDPR-compliant processor?"
        }));
    }
    if needs_nda {
        questions.insert("nda".into(), json!({
            "type": "noul",
            "instructions": "Does this content contain confidential or NDA-protected information that must not be sent to a processor without NDA coverage?"
        }));
    }
    let acquired = state
        .upstreams
        .route(&guard.model, PoolKind::SystemOne)
        .map_err(|error| format!("routing content guard model: {error}"))?;
    let model = acquired.resolved_model().to_string();
    let url = format!(
        "{}/systemone",
        acquired.backend().base_url.trim_end_matches('/')
    );
    let body = json!({
        "model": model,
        "state": { "messages": request.get("messages").cloned().unwrap_or(Value::Null) },
        "questions": questions,
    });
    let mut call = state
        .http
        .post(url)
        .json(&body)
        .timeout(Duration::from_secs(5));
    if let Some(key) = acquired.backend().api_key.as_deref() {
        call = call.bearer_auth(key);
    }
    let response = call
        .send()
        .await
        .map_err(|error| format!("calling content guard model: {error}"))?;
    let status = response.status();
    let answer: Value = response
        .json()
        .await
        .map_err(|error| format!("reading content guard response: {error}"))?;
    if !status.is_success() {
        return Err(format!("content guard model returned HTTP {status}"));
    }
    let (gdpr, nda) = parse_matches(&answer, needs_gdpr, needs_nda)?;
    let action = action_for(guard.mode, gdpr, nda, guard.gdpr_action, guard.nda_action);
    tracing::info!(
        guard_model = %guard.model,
        destination_gdpr = compliance.gdpr,
        destination_nda = compliance.nda,
        checked_gdpr = needs_gdpr,
        checked_nda = needs_nda,
        matched_gdpr = gdpr,
        matched_nda = nda,
        mode = guard.mode.as_str(),
        action = ?action,
        "content guard evaluated request"
    );
    Ok(action)
}

fn parse_matches(
    answer: &Value,
    needs_gdpr: bool,
    needs_nda: bool,
) -> Result<(bool, bool), String> {
    let matched = |name: &str, required: bool| -> Result<bool, String> {
        if !required {
            return Ok(false);
        }
        let value = answer
            .pointer(&format!("/answers/{name}/noul"))
            .and_then(Value::as_f64)
            .filter(|value| value.is_finite() && (0.0..=1.0).contains(value))
            .ok_or_else(|| format!("content guard response is missing a valid `{name}` answer"))?;
        Ok(value >= 0.5)
    };
    Ok((matched("gdpr", needs_gdpr)?, matched("nda", needs_nda)?))
}

pub fn required_checks(compliance: Compliance) -> (bool, bool) {
    (!compliance.gdpr, !compliance.nda)
}

fn action_for(
    mode: ContentGuardMode,
    gdpr: bool,
    nda: bool,
    gdpr_action: ContentGuardPolicy,
    nda_action: ContentGuardPolicy,
) -> Action {
    if mode != ContentGuardMode::Enforce || (!gdpr && !nda) {
        return Action::Allow;
    }
    [gdpr.then_some(gdpr_action), nda.then_some(nda_action)]
        .into_iter()
        .flatten()
        .fold(Action::Allow, |current, action| match (current, action) {
            (Action::Deny, _) | (_, ContentGuardPolicy::Deny) => Action::Deny,
            (Action::Confirm, _) | (_, ContentGuardPolicy::Confirm) => Action::Confirm,
            _ => Action::Allow,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monitor_never_changes_dispatch() {
        assert_eq!(
            action_for(
                ContentGuardMode::Monitor,
                true,
                true,
                ContentGuardPolicy::Deny,
                ContentGuardPolicy::Deny,
            ),
            Action::Allow
        );
    }

    #[test]
    fn deny_wins_over_confirmation() {
        assert_eq!(
            action_for(
                ContentGuardMode::Enforce,
                true,
                true,
                ContentGuardPolicy::Confirm,
                ContentGuardPolicy::Deny,
            ),
            Action::Deny
        );
    }

    #[test]
    fn confirmation_is_returned_when_it_is_the_only_matching_action() {
        assert_eq!(
            action_for(
                ContentGuardMode::Enforce,
                false,
                true,
                ContentGuardPolicy::Deny,
                ContentGuardPolicy::Confirm,
            ),
            Action::Confirm
        );
    }

    #[test]
    fn pool_compliance_selects_only_missing_checks() {
        assert_eq!(
            required_checks(Compliance {
                gdpr: true,
                nda: true
            }),
            (false, false)
        );
        assert_eq!(
            required_checks(Compliance {
                gdpr: true,
                nda: false
            }),
            (false, true)
        );
        assert_eq!(
            required_checks(Compliance {
                gdpr: false,
                nda: true
            }),
            (true, false)
        );
        assert_eq!(
            required_checks(Compliance {
                gdpr: false,
                nda: false
            }),
            (true, true)
        );
    }

    #[test]
    fn missing_required_answer_is_an_error() {
        let answer = json!({"answers": {"gdpr": {"type": "noul", "noul": 0.9}}});
        assert!(parse_matches(&answer, true, true).is_err());
    }

    #[tokio::test]
    async fn chat_confirmation_accepts_only_the_approve_choice() {
        let (broadcast, mut events) = tokio::sync::broadcast::channel(4);
        let feedback = ChatFeedback::for_test(broadcast);
        let hub = feedback.ask_hub.clone();
        let confirmation = confirm_in_chat(Some(&feedback), Some("turn-1"));
        let answer = async {
            let event = events.recv().await.unwrap();
            assert!(matches!(
                event,
                session_core::workers::TurnUpdate::Prompt(_)
            ));
            while !hub.resolve(
                "turn-1",
                AskReply::Answered {
                    choices: vec!["approve".into()],
                    text: None,
                },
            ) {
                tokio::task::yield_now().await;
            }
        };
        let (approved, ()) = tokio::join!(confirmation, answer);
        assert!(approved);
    }
}
