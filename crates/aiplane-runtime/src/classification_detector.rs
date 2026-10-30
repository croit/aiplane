//! The System One detector: for a request and the model it is about to reach,
//! one question per data class the model is not cleared for.
//!
//! A class matches when its answer reaches the class's threshold. The
//! strongest action of the matched classes decides (block over confirm over
//! raise). In a person's chat every match also raises the conversation to the
//! class's level; `/v1` and agent turns stay stateless and never ratchet.

use std::time::Duration;

use aiplane_core::server::capped_read;
use aiplane_core::server::classification::{self, Action, ClassificationMode, Scheme};
use aiplane_core::server::upstreams::{PoolAccess, PoolKind};
use serde_json::{Value, json};

use crate::server::AppState;
use crate::server::tools::ChatFeedback;
use crate::server::tools::feedback::AskReply;

/// What the caller of a request does with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Confirm,
    Deny,
}

/// A data class the detector recognised in a request.
#[derive(Clone, Debug, PartialEq)]
pub struct Matched {
    pub class_id: String,
    pub level_id: String,
    pub action: Action,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Detection {
    pub matches: Vec<Matched>,
}

impl Detection {
    pub fn strongest(&self) -> Option<Action> {
        self.matches.iter().map(|m| m.action).max()
    }

    /// Without a conversation to raise, a class whose action is `raise` has
    /// nothing to do but keep the request away from a model that is not
    /// cleared for it.
    pub fn stateless_decision(&self) -> Decision {
        match self.strongest() {
            None => Decision::Allow,
            Some(Action::Confirm) => Decision::Confirm,
            Some(Action::Raise | Action::Block) => Decision::Deny,
        }
    }
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

struct Question {
    class_id: String,
    level_id: String,
    action: Action,
    threshold: f64,
    instructions: String,
}

fn questions_for(scheme: &Scheme, target_rank: u32) -> Vec<Question> {
    if !scheme.is_active() {
        return Vec::new();
    }
    scheme
        .classes_above(target_rank)
        .filter_map(|(class, level)| {
            let rule = class.rule.as_ref()?;
            let mut instructions = rule.question.clone();
            if !rule.positive.trim().is_empty() {
                instructions.push_str(&format!("\nExample that matches: {}", rule.positive));
            }
            if !rule.negative.trim().is_empty() {
                instructions.push_str(&format!("\nExample that does not match: {}", rule.negative));
            }
            Some(Question {
                class_id: class.id.clone(),
                level_id: level.id.clone(),
                action: class.action,
                threshold: rule.threshold,
                instructions,
            })
        })
        .collect()
}

/// Ask the System One model about every class above `target_rank` that has a
/// rule. No questions means no call and no match.
pub async fn detect(
    state: &AppState,
    request: &Value,
    target_rank: u32,
) -> Result<Detection, String> {
    let questions = questions_for(&state.scheme(), target_rank);
    if questions.is_empty() {
        return Ok(Detection::default());
    }
    let config = state.config();
    let model = config.classification.system_one_model.trim();
    if model.is_empty() {
        return Err(
            "data classification has rules to apply but no System One model is configured".into(),
        );
    }
    let body_questions: serde_json::Map<String, Value> = questions
        .iter()
        .map(|q| {
            (
                q.class_id.clone(),
                json!({"type": "noul", "instructions": q.instructions}),
            )
        })
        .collect();
    let acquired = state
        .upstreams
        .route(model, PoolKind::SystemOne)
        .map_err(|error| format!("routing the classification model: {error}"))?;
    let url = format!(
        "{}/systemone",
        acquired.backend().base_url.trim_end_matches('/')
    );
    let body = json!({
        "model": acquired.resolved_model(),
        "state": { "messages": request.get("messages").cloned().unwrap_or(Value::Null) },
        "questions": body_questions,
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
        .map_err(|error| format!("calling the classification model: {error}"))?;
    let status = response.status();
    let answer: Value = capped_read::read_capped_json(response, capped_read::MODEL_ANSWER_BYTES)
        .await
        .map_err(|error| format!("reading the classification model's response: {error}"))?;
    if !status.is_success() {
        return Err(format!("the classification model returned HTTP {status}"));
    }
    let detection = parse_matches(&answer, &questions)?;
    tracing::info!(
        classification_model = %model,
        target_rank,
        checked = ?questions.iter().map(|q| q.class_id.as_str()).collect::<Vec<_>>(),
        matched = ?detection.matches.iter().map(|m| m.class_id.as_str()).collect::<Vec<_>>(),
        mode = config.classification.mode.as_str(),
        "data classification evaluated request"
    );
    Ok(detection)
}

fn parse_matches(answer: &Value, questions: &[Question]) -> Result<Detection, String> {
    let mut matches = Vec::new();
    for question in questions {
        let score = answer
            .pointer(&format!("/answers/{}/noul", question.class_id))
            .and_then(Value::as_f64)
            .filter(|value| value.is_finite() && (0.0..=1.0).contains(value))
            .ok_or_else(|| {
                format!(
                    "the classification model's response is missing a valid `{}` answer",
                    question.class_id
                )
            })?;
        if score >= question.threshold {
            matches.push(Matched {
                class_id: question.class_id.clone(),
                level_id: question.level_id.clone(),
                action: question.action,
            });
        }
    }
    Ok(Detection { matches })
}

/// A detector failure refuses the request in enforce mode and is only logged
/// in monitor mode, where `None` stands for "nothing was evaluated".
fn settle(
    mode: ClassificationMode,
    result: Result<Detection, String>,
) -> Result<Option<Detection>, String> {
    match (mode, result) {
        (_, Ok(detection)) => Ok(Some(detection)),
        (ClassificationMode::Monitor, Err(error)) => {
            tracing::warn!(error = %error, "data classification could not evaluate the request in monitor mode");
            Ok(None)
        }
        (ClassificationMode::Enforce, Err(error)) => Err(error),
    }
}

fn model_rank(state: &AppState, model: &str, access: &PoolAccess) -> Option<u32> {
    state
        .upstreams
        .models_with_level_for_kind_for(PoolKind::Chat, access)
        .into_iter()
        .find_map(|(id, rank)| (id == model).then_some(rank))
}

/// The stateless check for `/v1`: nothing is stored, the decision follows the
/// matched classes. Monitor mode never acts.
pub async fn evaluate_for_model(
    state: &AppState,
    request: &Value,
    model: &str,
    access: &PoolAccess,
) -> Result<Decision, String> {
    let Some(rank) = model_rank(state, model, access) else {
        return Ok(Decision::Allow);
    };
    let mode = state.config().classification.mode;
    let Some(detection) = settle(mode, detect(state, request, rank).await)? else {
        return Ok(Decision::Allow);
    };
    Ok(if mode == ClassificationMode::Enforce {
        detection.stateless_decision()
    } else {
        Decision::Allow
    })
}

/// The check before a chat turn reaches `model`. In a person's chat each match
/// raises the conversation to the class's level (cause `system_one:<class>`).
pub(crate) async fn evaluate_for_turn(
    state: &AppState,
    request: &Value,
    model: &str,
    access: &PoolAccess,
    context: &crate::server::tools::ToolContext,
) -> Result<Decision, String> {
    let Some(rank) = model_rank(state, model, access) else {
        return Ok(Decision::Allow);
    };
    let scheme = state.scheme();
    if !questions_for(&scheme, rank).is_empty() {
        let authority = context
            .agent
            .clone()
            .map(crate::server::model_route::ModelAuthority::live);
        if let Some(authority) = &authority {
            authority.check_turn(
                context.session_id.as_deref(),
                context.assistant_turn_id.as_deref(),
            )?;
        }
        crate::server::model_route::validate_model_authority(
            state,
            &context.principal,
            authority.as_ref(),
        )
        .await?;
    }
    let mode = state.config().classification.mode;
    let Some(detection) = settle(mode, detect(state, request, rank).await)? else {
        return Ok(Decision::Allow);
    };
    if mode == ClassificationMode::Monitor || detection.matches.is_empty() {
        return Ok(Decision::Allow);
    }
    // Person chats only: an agent's model comes from its spec, so ratcheting
    // its conversation would silently narrow the agent rather than offer the
    // person a choice.
    if context.agent.is_none()
        && let Some(session_id) = context.session_id.as_deref()
    {
        let actor = context.principal.subject_id();
        for matched in &detection.matches {
            if let Err(error) = classification::raise(
                &state.db,
                &scheme,
                session_id,
                &matched.level_id,
                &format!("system_one:{}", matched.class_id),
                actor,
            )
            .await
            {
                tracing::warn!(
                    session_id,
                    class = %matched.class_id,
                    error = %error,
                    "could not raise the conversation after a classification match"
                );
            }
        }
        return Ok(match detection.strongest() {
            Some(Action::Block) => Decision::Deny,
            Some(Action::Confirm) => Decision::Confirm,
            _ => Decision::Allow,
        });
    }
    Ok(detection.stateless_decision())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn organization_turns_keep_the_operator_model_and_refuse_revoked_scope_in_monitor() {
        use crate::agent_run::Actor;
        use crate::server::headless::tests::{organization_draft_fixture, state_for};
        use aiplane_core::server::upstreams::{
            BackendConfig, UpstreamPoolConfig, UpstreamRegistry,
        };
        use std::collections::HashMap;
        use wiremock::{
            Mock, MockServer, ResponseTemplate,
            matchers::{method, path},
        };
        let upstream = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/systemone"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "answers":{"pii":{"type":"noul","noul":0.0}}
            })))
            .mount(&upstream)
            .await;
        let original = state_for(&upstream.uri()).await;
        let (profile, params) = organization_draft_fixture(&original).await;
        let pools: HashMap<_, _> = [
            ("chat", PoolKind::Chat, params.model.as_str()),
            ("guard", PoolKind::SystemOne, "operator-guard"),
        ]
        .into_iter()
        .map(|(name, kind, model)| {
            (
                name.to_owned(),
                UpstreamPoolConfig {
                    kind,
                    strategy: Default::default(),
                    fallback_offline: None,
                    models: Vec::new(),
                    level_rank: 0,
                    enforce_limits: true,
                    voices: Default::default(),
                    offer_voices: Vec::new(),
                    allowed_groups: Vec::new(),
                    backend: vec![BackendConfig {
                        priority: Default::default(),
                        name: name.into(),
                        base_url: upstream.uri(),
                        models: vec![model.to_owned()],
                        api_key_env: None,
                        api_key: None,
                        weight: 1,
                        max_inflight: 8,
                        health_path: "/models".into(),
                        alias: Default::default(),
                        enabled: true,
                        extra_headers: Default::default(),
                        supports_edit: false,
                    }],
                },
            )
        })
        .collect();
        let mut config = (*original.config()).clone();
        config.classification.system_one_model = "operator-guard".into();
        config.classification.mode = ClassificationMode::Monitor;
        let state = crate::server::AppState::new(
            config,
            original.db.clone(),
            UpstreamRegistry::new(&pools).unwrap(),
            original.tools(),
            original.rbac.clone(),
        );
        sqlx::raw_sql(
            "INSERT INTO classification_levels (id, rank, name) VALUES ('internal', 1, 'Internal');
             INSERT INTO classification_data_classes (id, name, level_id, rule_question, action)
             VALUES ('pii', 'Personal data', 'internal', 'Does this contain personal data?', 'block');",
        )
        .execute(&state.db)
        .await
        .unwrap();
        state.reload_scheme().await;
        let Actor::Agent(run) = &params.actor else {
            panic!("agent expected")
        };
        let context = crate::server::tools::ToolContext {
            principal: run.principal(),
            agent: Some(run.clone()),
            session_id: Some(params.session_id),
            assistant_turn_id: Some(params.assistant_turn_id),
            ..crate::server::tools::ToolContext::for_test(state.db.clone())
        };
        let access = PoolAccess::for_system_models(&profile.principal, [&*params.model]);
        assert!(!profile.principal.grants.has(
            aiplane_core::server::principal::GrantKind::Model,
            "operator-guard"
        ));
        assert_eq!(
            evaluate_for_turn(
                &state,
                &json!({"messages":[]}),
                &params.model,
                &access,
                &context
            )
            .await
            .unwrap(),
            Decision::Allow
        );
        assert_eq!(upstream.received_requests().await.unwrap().len(), 1);
        sqlx::query("UPDATE agent_owners SET active=0 WHERE principal_id=?")
            .bind(&profile.principal.id)
            .execute(&state.db)
            .await
            .unwrap();
        assert!(
            evaluate_for_turn(
                &state,
                &json!({"messages":[]}),
                &params.model,
                &access,
                &context
            )
            .await
            .is_err()
        );
        assert_eq!(upstream.received_requests().await.unwrap().len(), 1);
    }

    fn question(class_id: &str, threshold: f64, action: Action) -> Question {
        Question {
            class_id: class_id.into(),
            level_id: "internal".into(),
            action,
            threshold,
            instructions: String::new(),
        }
    }

    #[test]
    fn a_score_at_the_threshold_matches_and_below_does_not() {
        let questions = [question("pii", 0.5, Action::Block)];
        let hit = parse_matches(&json!({"answers": {"pii": {"noul": 0.5}}}), &questions).unwrap();
        assert_eq!(hit.matches.len(), 1);
        let miss = parse_matches(&json!({"answers": {"pii": {"noul": 0.49}}}), &questions).unwrap();
        assert!(miss.matches.is_empty());
    }

    #[test]
    fn missing_required_answer_is_an_error() {
        let questions = [
            question("pii", 0.5, Action::Block),
            question("nda", 0.5, Action::Block),
        ];
        let answer = json!({"answers": {"pii": {"type": "noul", "noul": 0.9}}});
        assert!(parse_matches(&answer, &questions).is_err());
    }

    #[test]
    fn block_wins_over_confirm_over_raise() {
        let detection = |actions: &[Action]| Detection {
            matches: actions
                .iter()
                .map(|action| Matched {
                    class_id: "c".into(),
                    level_id: "internal".into(),
                    action: *action,
                })
                .collect(),
        };
        assert_eq!(
            detection(&[Action::Raise, Action::Block, Action::Confirm]).strongest(),
            Some(Action::Block)
        );
        assert_eq!(
            detection(&[Action::Raise, Action::Confirm]).stateless_decision(),
            Decision::Confirm
        );
        assert_eq!(
            detection(&[Action::Raise]).stateless_decision(),
            Decision::Deny
        );
        assert_eq!(detection(&[]).stateless_decision(), Decision::Allow);
    }

    #[test]
    fn monitor_mode_swallows_a_detector_failure_and_enforce_refuses() {
        assert_eq!(
            settle(ClassificationMode::Monitor, Err("down".into())),
            Ok(None)
        );
        assert!(settle(ClassificationMode::Enforce, Err("down".into())).is_err());
    }

    #[test]
    fn a_single_level_scheme_asks_no_questions() {
        assert!(questions_for(&Scheme::default(), 0).is_empty());
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
