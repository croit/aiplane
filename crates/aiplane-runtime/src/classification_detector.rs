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
use aiplane_features::server::classification::{TermMatcher, find_patterns};
use serde_json::{Value, json};
use session_core::i18n::{Lang, t};

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

/// One reason to hold a conversation at a level, whatever found it: a class
/// the System One model recognised, a pattern or a word of a class, or a
/// marking a file carries (no class, always `raise`). `cause` is the audited,
/// content-free reason.
#[derive(Clone, Debug, PartialEq)]
pub struct Finding {
    pub class_id: Option<String>,
    pub level_id: String,
    pub cause: String,
    pub action: Action,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Detection {
    pub matches: Vec<Matched>,
}

/// The word lists of the data classes, compiled once per scheme reload.
#[derive(Clone, Default)]
pub struct TermMatchers(std::collections::HashMap<String, TermMatcher>);

impl TermMatchers {
    pub async fn load(db: &aiplane_core::server::db::Pool) -> Self {
        let lists = match classification::class_terms(db).await {
            Ok(lists) => lists,
            Err(error) => {
                tracing::error!(error = %error, "reading the classification word lists; none will match");
                return Self::default();
            }
        };
        Self(
            lists
                .into_iter()
                .filter_map(|(class_id, terms)| match TermMatcher::new(&terms) {
                    Ok(matcher) => Some((class_id, matcher)),
                    Err(error) => {
                        tracing::error!(class = %class_id, error = %error, "a classification word list could not be compiled; it will not match");
                        None
                    }
                })
                .collect(),
        )
    }

    fn matches(&self, class_id: &str, text: &str) -> bool {
        self.0
            .get(class_id)
            .is_some_and(|matcher| matcher.matches(text))
    }
}

/// The classes above `target_rank` that `text` matches by a built-in pattern
/// or a word of the class's list. Needs no model and asks nothing.
pub fn static_findings(
    scheme: &Scheme,
    terms: &TermMatchers,
    text: &str,
    target_rank: u32,
) -> Vec<Finding> {
    let mut out = Vec::new();
    for (class, level) in scheme.classes_above(target_rank) {
        let ids: Vec<&str> = class.patterns.iter().map(String::as_str).collect();
        if !find_patterns(text, &ids).is_empty() {
            out.push(Finding {
                class_id: Some(class.id.clone()),
                level_id: level.id.clone(),
                cause: format!("pattern:{}", class.id),
                action: class.action,
            });
        }
        if terms.matches(&class.id, text) {
            out.push(Finding {
                class_id: Some(class.id.clone()),
                level_id: level.id.clone(),
                cause: format!("terms:{}", class.id),
                action: class.action,
            });
        }
    }
    out
}

pub fn strongest(findings: &[Finding]) -> Option<Action> {
    findings.iter().map(|finding| finding.action).max()
}

pub fn stateless_decision(findings: &[Finding]) -> Decision {
    match strongest(findings) {
        None => Decision::Allow,
        Some(Action::Confirm) => Decision::Confirm,
        Some(Action::Raise | Action::Block) => Decision::Deny,
    }
}

impl Detection {
    /// Without a conversation to raise, a class whose action is `raise` has
    /// nothing to do but keep the request away from a model that is not
    /// cleared for it.
    pub fn stateless_decision(&self) -> Decision {
        stateless_decision(&self.findings())
    }

    pub fn findings(&self) -> Vec<Finding> {
        self.matches
            .iter()
            .map(|matched| Finding {
                class_id: Some(matched.class_id.clone()),
                level_id: matched.level_id.clone(),
                cause: format!("system_one:{}", matched.class_id),
                action: matched.action,
            })
            .collect()
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

/// Why the System One model could not answer: nothing is configured to ask,
/// or asking failed.
#[derive(Debug, PartialEq)]
pub enum DetectorError {
    NotConfigured,
    Failed(String),
}

impl std::fmt::Display for DetectorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotConfigured => f.write_str(
                "data classification has rules to apply but no System One model is configured",
            ),
            Self::Failed(reason) => f.write_str(reason),
        }
    }
}

impl From<String> for DetectorError {
    fn from(reason: String) -> Self {
        Self::Failed(reason)
    }
}

/// Ask the System One model about every class above `target_rank` that has a
/// rule. No questions means no call and no match.
pub async fn detect(
    state: &AppState,
    request: &Value,
    target_rank: u32,
) -> Result<Detection, DetectorError> {
    let questions = questions_for(&state.scheme(), target_rank);
    if questions.is_empty() {
        return Ok(Detection::default());
    }
    let config = state.config();
    let model = config.classification.system_one_model.trim();
    if model.is_empty() {
        return Err(DetectorError::NotConfigured);
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
        return Err(format!("the classification model returned HTTP {status}").into());
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
    result: Result<Detection, DetectorError>,
) -> Result<Option<Detection>, String> {
    match (mode, result) {
        (_, Ok(detection)) => Ok(Some(detection)),
        (ClassificationMode::Monitor, Err(error)) => {
            tracing::warn!(error = %error, "data classification could not evaluate the request in monitor mode");
            Ok(None)
        }
        (ClassificationMode::Enforce, Err(error)) => {
            tracing::warn!(error = %error, "data classification could not evaluate the request; refusing it");
            Err(t(
                Lang::En,
                match error {
                    DetectorError::NotConfigured => "cls-detector-not-configured",
                    DetectorError::Failed(_) => "cls-detector-unavailable",
                },
            ))
        }
    }
}

/// What the System One model finds in `text` among the classes above
/// `target_rank`. A failing model refuses the request in enforce mode and is
/// logged in monitor mode.
pub async fn system_one_findings(
    state: &AppState,
    text: &str,
    target_rank: u32,
) -> Result<Vec<Finding>, String> {
    let request = json!({"messages": [{"role": "user", "content": text}]});
    let mode = state.config().classification.mode;
    Ok(settle(mode, detect(state, &request, target_rank).await)?
        .map(|detection| detection.findings())
        .unwrap_or_default())
}

/// Everything a text can show about its level for the classes above
/// `target_rank`: patterns and word lists first, then the System One model.
/// Nothing is asked when the scheme is inactive.
pub async fn text_findings(
    state: &AppState,
    text: &str,
    target_rank: u32,
) -> Result<Vec<Finding>, String> {
    let scheme = state.scheme();
    if !scheme.is_active() {
        return Ok(Vec::new());
    }
    let mut findings = static_findings(&scheme, &state.term_matchers(), text, target_rank);
    findings.extend(system_one_findings(state, text, target_rank).await?);
    Ok(findings)
}

fn model_rank(state: &AppState, model: &str, access: &PoolAccess) -> Option<u32> {
    state
        .upstreams
        .models_with_level_for_kind_for(PoolKind::Chat, access)
        .into_iter()
        .find_map(|(id, rank)| (id == model).then_some(rank))
}

/// The text of a chat request's messages: string content and text parts.
fn request_text(request: &Value) -> String {
    let mut text = String::new();
    for message in request
        .get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        match message.get("content") {
            Some(Value::String(content)) => text.push_str(content),
            Some(Value::Array(parts)) => {
                for part in parts {
                    if let Some(part_text) = part.get("text").and_then(Value::as_str) {
                        text.push_str(part_text);
                        text.push('\n');
                    }
                }
            }
            _ => {}
        }
        text.push('\n');
    }
    text
}

/// Which part of a chat request carries content to classify.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    /// A person's turn: what they wrote since the last answer. The operator's
    /// system prompt is not theirs, and earlier messages were checked when
    /// they were sent (the level only rises).
    NewInput,
    /// An API request: everything the client sent except the model's answers.
    ClientSent,
}

fn checked_request(request: &Value, scope: Scope) -> Value {
    let messages = request
        .get("messages")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    fn role(message: &Value) -> &str {
        message.get("role").and_then(Value::as_str).unwrap_or("")
    }
    let checked: Vec<Value> = match scope {
        Scope::NewInput => {
            let start = messages
                .iter()
                .rposition(|message| role(message) == "assistant")
                .map_or(0, |last| last + 1);
            messages[start..]
                .iter()
                .filter(|message| role(message) == "user")
                .cloned()
                .collect()
        }
        Scope::ClientSent => messages
            .iter()
            .filter(|message| role(message) != "assistant")
            .cloned()
            .collect(),
    };
    json!({ "messages": checked })
}

/// Everything a request shows about its level for the classes above
/// `target_rank`: patterns and word lists first, then the System One model.
/// A check the System One model could not finish: what the patterns and word
/// lists found anyway, and why the model failed.
struct Unchecked {
    found: Vec<Finding>,
    error: String,
}

async fn request_findings(
    state: &AppState,
    request: &Value,
    scope: Scope,
    target_rank: u32,
) -> Result<Vec<Finding>, Unchecked> {
    let request = &checked_request(request, scope);
    if request["messages"].as_array().is_none_or(Vec::is_empty) {
        return Ok(Vec::new());
    }
    let mode = state.config().classification.mode;
    let mut findings = static_findings(
        &state.scheme(),
        &state.term_matchers(),
        &request_text(request),
        target_rank,
    );
    match settle(mode, detect(state, request, target_rank).await) {
        Ok(detection) => {
            findings.extend(detection.into_iter().flat_map(|d| d.findings()));
            Ok(findings)
        }
        Err(error) => Err(Unchecked {
            found: findings,
            error,
        }),
    }
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
    let findings = request_findings(state, request, Scope::ClientSent, rank)
        .await
        .map_err(|unchecked| unchecked.error)?;
    Ok(if mode == ClassificationMode::Enforce {
        stateless_decision(&findings)
    } else {
        Decision::Allow
    })
}

/// The check before a chat turn reaches `model`. In a person's chat each match
/// raises the conversation to the class's level (cause `system_one:<class>`),
/// so every class above the conversation's level is checked, whatever the
/// model is cleared for: the level outlives this turn's model (tools, sharing,
/// the next model picked). Elsewhere only the classes above the model matter.
pub(crate) async fn evaluate_for_turn(
    state: &AppState,
    request: &Value,
    model: &str,
    access: &PoolAccess,
    context: &crate::server::tools::ToolContext,
) -> Result<Decision, String> {
    let scheme = state.scheme();
    let person_session = context
        .session_id
        .as_deref()
        .filter(|_| context.agent.is_none());
    let rank = match person_session {
        Some(session_id) => match classification::level_id(&state.db, session_id).await {
            Ok(stored) => scheme.level_or_lowest(stored.as_deref()).rank,
            Err(error) => {
                return Err(format!(
                    "reading the conversation's classification level failed ({error}); try again"
                ));
            }
        },
        None => match model_rank(state, model, access) {
            Some(rank) => rank,
            None => return Ok(Decision::Allow),
        },
    };
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
    let scope = if person_session.is_some() {
        Scope::NewInput
    } else {
        Scope::ClientSent
    };
    let findings = match request_findings(state, request, scope, rank).await {
        Ok(findings) => findings,
        Err(Unchecked { found, error }) => {
            if !found.is_empty() {
                apply_findings(state, &scheme, context, &found).await;
            }
            return Err(error);
        }
    };
    if mode == ClassificationMode::Monitor || findings.is_empty() {
        if !findings.is_empty() {
            tracing::info!(
                causes = ?findings.iter().map(|finding| finding.cause.as_str()).collect::<Vec<_>>(),
                "data classification would act on this request in enforce mode"
            );
        }
        return Ok(Decision::Allow);
    }
    Ok(apply_findings(state, &scheme, context, &findings).await)
}

/// What the findings do to a turn. In a person's chat each one raises the
/// conversation to its level and the strongest action decides; anywhere else
/// nothing is stored and the decision follows the findings alone.
///
/// Person chats only: an agent's model comes from its spec, so ratcheting its
/// conversation would silently narrow the agent rather than offer the person
/// a choice.
pub(crate) async fn apply_findings(
    state: &AppState,
    scheme: &Scheme,
    context: &crate::server::tools::ToolContext,
    findings: &[Finding],
) -> Decision {
    if context.agent.is_none()
        && let Some(session_id) = context.session_id.as_deref()
    {
        let actor = context.principal.subject_id();
        for finding in findings {
            match classification::raise(
                &state.db,
                scheme,
                session_id,
                &finding.level_id,
                &finding.cause,
                actor,
            )
            .await
            {
                Ok(transition) => announce_raise(context, transition.as_ref()),
                Err(error) => tracing::warn!(
                    session_id,
                    cause = %finding.cause,
                    error = %error,
                    "could not raise the conversation after a classification match"
                ),
            }
        }
        return match strongest(findings) {
            Some(Action::Block) => Decision::Deny,
            Some(Action::Confirm) => Decision::Confirm,
            _ => Decision::Allow,
        };
    }
    stateless_decision(findings)
}

/// A raised level changes the badge, the model list and the share options of
/// the open chat, so the live stream tells the page to reload them.
pub(crate) fn announce_raise(
    context: &crate::server::tools::ToolContext,
    transition: Option<&classification::Transition>,
) {
    if let (Some(_), Some(feedback)) = (transition, context.chat_feedback.as_ref()) {
        let _ = feedback
            .broadcast
            .send(session_core::workers::TurnUpdate::SidebarChanged);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conversation() -> Value {
        json!({"messages": [
            {"role": "system", "content": "You talk to alice@example.com."},
            {"role": "user", "content": "pay DE89 3704 0044 0532 0130 00"},
            {"role": "assistant", "content": "Done."},
            {"role": "user", "content": [{"type": "text", "text": "what time is it"}]},
        ]})
    }

    #[test]
    fn a_turn_checks_only_what_the_person_wrote_since_the_last_answer() {
        assert_eq!(
            checked_request(&conversation(), Scope::NewInput),
            json!({"messages": [
                {"role": "user", "content": [{"type": "text", "text": "what time is it"}]},
            ]})
        );
    }

    #[test]
    fn a_later_round_of_a_turn_has_nothing_new_to_check() {
        let mut request = conversation();
        request["messages"].as_array_mut().unwrap().extend([
            json!({"role": "assistant", "content": null}),
            json!({"role": "tool", "content": "12:00"}),
        ]);
        assert_eq!(
            checked_request(&request, Scope::NewInput),
            json!({"messages": []})
        );
    }

    #[test]
    fn an_api_request_checks_everything_its_client_sent_but_the_answers() {
        let checked = checked_request(&conversation(), Scope::ClientSent);
        let roles: Vec<&str> = checked["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["role"].as_str().unwrap())
            .collect();
        assert_eq!(roles, ["system", "user", "user"]);
    }

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
                &json!({"messages":[{"role":"user","content":"hello"}]}),
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
                &json!({"messages":[{"role":"user","content":"hello"}]}),
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
            strongest(&detection(&[Action::Raise, Action::Block, Action::Confirm]).findings()),
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
            settle(ClassificationMode::Monitor, Err("down".to_string().into())),
            Ok(None)
        );
        assert!(settle(ClassificationMode::Enforce, Err("down".to_string().into())).is_err());
        let refusal = settle(
            ClassificationMode::Enforce,
            Err(DetectorError::NotConfigured),
        )
        .unwrap_err();
        assert_eq!(refusal, t(Lang::En, "cls-detector-not-configured"));
        assert_ne!(refusal, t(Lang::En, "cls-detector-unavailable"));
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
