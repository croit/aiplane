// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! From the model's proposal to what the assistant offers: every piece
//! mapped to the spec fragment it stands for and checked with
//! [`spec::validate`] on the draft as it would be after the pieces before it
//! (`docs/agent-builder.md` → "Prompt assistant"). A piece that would add a problem
//! to the draft, or that names something this manager may not grant, is
//! dropped with the reason in words. Nothing here writes anything.

use std::collections::{BTreeSet, HashMap};

use aiplane_core::server::principal::{GrantKind, GrantSet};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

use super::handoffs::{self, Rule, Target};
use super::proposal::{
    AbilityProposal, HandoffProposal, IdentityProposal, KnowledgeProposal, ScopeProposal,
    SlotProposal, TestProposal, ToneProposal,
};
use super::proposal::{CONDITION_DETAILS, CONDITION_IDENTITY};
use super::{
    Candidates, HUMAN_TARGET, IDENTITY_METHODS, Knowledge, MAX_TESTS, RAG_LIST, RAG_SEARCH,
    SLOT_TYPES, is_knowledge_tool, tone,
};
use crate::agents::eval::{self, MAX_CASE_NAME_CHARS};
use crate::agents::spec::{self, SpecContext, SpecIssue, Stage};

const MAX_IDENT_LEN: usize = 48;
const MAX_MISSING_KNOWLEDGE: usize = 5;
const MAX_SUBJECT_CHARS: usize = 120;
/// The setup's `text` shape (`SLOT_SHAPES` in `web/src/lib/agent-setup.ts`).
const TEXT_MAX: u64 = 200;
const LONG_TEXT_MAX: u64 = 2000;

/// What the agent's draft is checked against: its grants as stored now,
/// every agent and every live spec, as on a save.
pub struct ReviewContext<'a> {
    pub agent_id: &'a str,
    pub action_catalogues: &'a std::collections::BTreeMap<String, BTreeSet<String>>,
    pub grants: &'a GrantSet,
    pub agents: &'a HashMap<String, bool>,
    pub live_specs: &'a HashMap<String, Value>,
    pub candidates: &'a Candidates,
    /// As [`SpecContext::allow_private`].
    pub allow_private: bool,
}

/// The offer, one entry per assistant step, and what was left out.
#[derive(Debug, Clone, Default, Serialize, schemars::JsonSchema)]
pub struct Suggestion {
    pub steps: Steps,
    pub dropped: Vec<Dropped>,
}

#[derive(Debug, Clone, Default, Serialize, schemars::JsonSchema)]
pub struct Steps {
    pub task: Option<TaskStep>,
    pub tone: Option<ToneStep>,
    pub scope: Option<ScopeStep>,
    pub abilities: Vec<AbilityStep>,
    pub knowledge: Vec<KnowledgeStep>,
    /// Subjects the agent needs that no knowledge base this manager may
    /// grant covers: an admin has to add one.
    pub missing_knowledge: Vec<String>,
    pub slots: Vec<SlotStep>,
    pub identity: Option<IdentityStep>,
    pub handoffs: Vec<HandoffStep>,
    pub tests: Vec<TestStep>,
}

impl Steps {
    /// The steps with something to offer, for the activity event.
    pub fn offered(&self) -> Vec<&'static str> {
        [
            ("task", self.task.is_some()),
            ("tone", self.tone.is_some()),
            ("scope", self.scope.is_some()),
            ("abilities", !self.abilities.is_empty()),
            ("knowledge", !self.knowledge.is_empty()),
            ("slots", !self.slots.is_empty()),
            ("identity", self.identity.is_some()),
            ("handoffs", !self.handoffs.is_empty()),
            ("tests", !self.tests.is_empty()),
        ]
        .into_iter()
        .filter_map(|(step, has)| has.then_some(step))
        .collect()
    }
}

/// `main.instructions.orchestration`.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct TaskStep {
    pub orchestration: String,
}

/// `main.instructions.response`, as the setup's task-and-tone step holds
/// it: tone chips by id ([`tone::TONES`]), the answer language (`visitor`
/// or a code; `None`: nothing said) and `response`, the further text no
/// chip or language stands for.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct ToneStep {
    pub response: String,
    pub chips: Vec<String>,
    pub language: Option<String>,
}

/// `scope.topics`, `scope.refusal`, `scope.strict`.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct ScopeStep {
    pub topics: Vec<String>,
    pub refusal: String,
    pub strict: bool,
}

/// A tool for `main.tools`, which the UI grants to the agent when applied.
/// `name` is the title its ability card shows.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct AbilityStep {
    pub id: String,
    pub name: String,
    pub why: String,
}

/// A knowledge base (RAG collection `id`) to search, which the UI switches
/// on like its card: the collection and `rag_search` granted, the search
/// bound to it ([`set_knowledge`]).
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct KnowledgeStep {
    pub id: String,
    pub name: String,
    pub why: String,
}

/// `state.<name>` = `def`.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct SlotStep {
    pub name: String,
    pub label: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub def: Value,
}

/// A recommendation only: the method needs a connector or a key the
/// identity step asks for, so no spec fragment comes with it.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct IdentityStep {
    pub method: String,
    pub why: String,
}

/// `routes.<name>` = `route`: one rule of the setup's hand-off step
/// ("when it is about `topic` (and all details are collected, and the
/// identity is confirmed), hand over to `target`"), in the shape
/// [`handoffs::write`] gives it. `identity` on a draft without an identity
/// check stands for the check the same offer recommends: applying the
/// hand-off sets that up first (`applySuggestedRules`), and `route` has no
/// identity leaf until then.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct HandoffStep {
    pub name: String,
    pub topic: String,
    pub target: String,
    pub target_name: String,
    pub details: bool,
    pub identity: bool,
    pub route: Value,
}

/// A test case in the shape `POST /api/v0/agents/{id}/tests` takes.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct TestStep {
    pub name: String,
    pub kind: String,
    pub script: Value,
    pub expect: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, schemars::JsonSchema)]
pub struct Dropped {
    pub step: &'static str,
    /// The piece within the step (a slot's or a test's name), when there is
    /// one.
    pub item: Option<String>,
    pub reason: String,
}

/// Map `answer` (the model's JSON object) onto `base` (the draft the manager
/// is editing) and keep what holds.
pub fn review(answer: &Value, base: &Value, ctx: &ReviewContext<'_>) -> Suggestion {
    let mut r = Reviewer::new(base, ctx);
    r.steps(answer);
    r.identity_proposed = r
        .out
        .steps
        .identity
        .as_ref()
        .is_some_and(|i| i.method != "none");
    for handoff in r
        .field::<Vec<HandoffProposal>>(answer, "handoffs")
        .unwrap_or_default()
    {
        r.handoff(handoff);
    }
    for test in r
        .field::<Vec<TestProposal>>(answer, "tests")
        .unwrap_or_default()
    {
        r.test(test);
    }
    r.out
}

/// One rule of the setup's hand-off step, as an architect asks for it.
#[derive(Debug, serde::Deserialize)]
struct SetupHandoff {
    topic: String,
    /// An agent id, or `human`.
    target: String,
    /// Only once every detail the agent collects is set.
    #[serde(default)]
    details: bool,
    /// Only once the visitor's identity is confirmed.
    #[serde(default)]
    identity: bool,
}

/// An architect's changes applied to `base`: the draft to save, the grants
/// it needs, and what was applied or left out (`docs/agent-builder.md` → "Agent architect").
#[derive(Debug, Clone)]
pub struct Applied {
    pub suggestion: Suggestion,
    pub draft: Value,
    /// Made before the draft is saved, through the capped grant route.
    pub grants: Vec<(GrantKind, String)>,
    pub display: Option<String>,
    pub model: Option<String>,
    /// Whether the hand-offs changed.
    pub handoffs: bool,
}

/// Apply `changes` — the proposal's steps plus `display` (the agent's name)
/// and `model` — to `base`, one piece at a time, with the same
/// checks [`review`] makes. Slots and hand-offs are written in the shapes the
/// setup assistant reads (a slot's `order`; hand-offs as its rules, see
/// [`super::handoffs`]). Test cases are not part of a draft, so they are left
/// out with a reason.
pub fn apply_changes(changes: &Value, base: &Value, ctx: &ReviewContext<'_>) -> Applied {
    let mut r = Reviewer::new(base, ctx);
    r.ordered = true;
    let display = r
        .field::<String>(changes, "display")
        .and_then(|d| r.display(d));
    let model = r.field::<String>(changes, "model").and_then(|m| r.model(m));
    r.steps(changes);
    let wanted = r
        .field::<Vec<SetupHandoff>>(changes, "handoffs")
        .unwrap_or_default();
    let fallback = r.field::<bool>(changes, "fallback_to_person");
    let handoffs = (!wanted.is_empty() || fallback.is_some()) && r.setup_handoffs(wanted, fallback);
    if changes.get("tests").is_some_and(|t| !t.is_null()) {
        r.drop(
            "tests",
            None,
            "test cases are not part of the draft — the person saves them on the setup's last \
             step",
        );
    }
    Applied {
        suggestion: r.out,
        draft: r.draft,
        grants: r.granted,
        display,
        model,
        handoffs,
    }
}

struct Reviewer<'a> {
    ctx: &'a ReviewContext<'a>,
    draft: Value,
    /// What the offered abilities (and an architect's model choice) would
    /// grant: the draft is checked as if the manager had granted them, which
    /// applying the step does.
    granted: Vec<(GrantKind, String)>,
    known: BTreeSet<(String, String)>,
    tests: BTreeSet<String>,
    /// Give each new slot the setup's next position (`order`).
    ordered: bool,
    /// The offer recommends an identity check, which the setup sets up along
    /// with a hand-off that waits for it: such a hand-off keeps the
    /// condition although the draft has no check yet.
    identity_proposed: bool,
    out: Suggestion,
}

impl<'a> Reviewer<'a> {
    fn new(base: &Value, ctx: &'a ReviewContext<'a>) -> Self {
        let draft = if base.is_object() {
            base.clone()
        } else {
            json!({})
        };
        let mut r = Self {
            ctx,
            draft,
            granted: Vec::new(),
            known: BTreeSet::new(),
            tests: BTreeSet::new(),
            ordered: false,
            identity_proposed: false,
            out: Suggestion::default(),
        };
        r.known = r.issues(&r.draft);
        r
    }

    fn drop(&mut self, step: &'static str, item: Option<&str>, reason: impl Into<String>) {
        self.out.dropped.push(Dropped {
            step,
            item: item.map(str::to_string),
            reason: reason.into(),
        });
    }

    /// `answer[key]` as `T`; a step that does not read is dropped whole.
    fn field<T: DeserializeOwned>(&mut self, answer: &Value, key: &'static str) -> Option<T> {
        let value = answer.get(key)?;
        if value.is_null() {
            return None;
        }
        match serde_json::from_value(value.clone()) {
            Ok(v) => Some(v),
            Err(e) => {
                self.drop(
                    key,
                    None,
                    format!("the model's proposal for this step does not read: {e}"),
                );
                None
            }
        }
    }

    fn issues(&self, draft: &Value) -> BTreeSet<(String, String)> {
        let grants = GrantSet::new(
            self.ctx
                .grants
                .iter()
                .map(|(k, r)| (k, r.to_string()))
                .chain(self.granted.iter().cloned()),
        );
        let ctx = SpecContext {
            action_catalogues: self.ctx.action_catalogues,
            agent_id: self.ctx.agent_id,
            grants: &grants,
            agents: self.ctx.agents,
            live_specs: self.ctx.live_specs,
            model_defaults: &Default::default(),
            speech_voices: &HashMap::new(),
            allow_private: self.ctx.allow_private,
        };
        spec::validate(draft, &ctx, Stage::Draft)
            .into_iter()
            .map(|SpecIssue { path, message }| (path, message))
            .collect()
    }

    /// Keep `candidate` as the draft when it adds no problem; otherwise say
    /// which problems it would add.
    fn adopt(&mut self, candidate: Value) -> Result<(), String> {
        let found = self.issues(&candidate);
        let added: Vec<String> = found
            .difference(&self.known)
            .map(|(path, message)| {
                if path.is_empty() {
                    message.clone()
                } else {
                    format!("at `{path}`: {message}")
                }
            })
            .collect();
        if added.is_empty() {
            self.draft = candidate;
            self.known = found;
            Ok(())
        } else {
            Err(format!(
                "it would make the draft invalid — {}",
                added.join("; ")
            ))
        }
    }

    fn with(&self, pointer: &[&str], value: Value) -> Value {
        let mut draft = self.draft.clone();
        set_at(&mut draft, pointer, value);
        draft
    }

    /// Every draft step of `answer`, in the assistant's step order.
    fn steps(&mut self, answer: &Value) {
        if let Some(task) = self.field::<String>(answer, "task") {
            self.task(task);
        }
        if let Some(tone) = self.field::<ToneProposal>(answer, "tone") {
            self.tone(tone);
        }
        if let Some(scope) = self.field::<ScopeProposal>(answer, "scope") {
            self.scope(scope);
        }
        for ability in self
            .field::<Vec<AbilityProposal>>(answer, "abilities")
            .unwrap_or_default()
        {
            self.ability(ability);
        }
        if let Some(knowledge) = self.field::<Vec<KnowledgeProposal>>(answer, "knowledge") {
            self.knowledge(knowledge);
        }
        if let Some(missing) = self.field::<Vec<String>>(answer, "missing_knowledge") {
            self.out.steps.missing_knowledge = clean(&missing)
                .into_iter()
                .filter(|m| m.chars().count() <= MAX_SUBJECT_CHARS)
                .take(MAX_MISSING_KNOWLEDGE)
                .collect();
        }
        for slot in self
            .field::<Vec<SlotProposal>>(answer, "slots")
            .unwrap_or_default()
        {
            self.slot(slot);
        }
        if let Some(identity) = self.field::<IdentityProposal>(answer, "identity") {
            self.identity(identity);
        }
    }

    /// The setup's next free slot position.
    fn next_order(&self) -> u64 {
        self.draft
            .get("state")
            .and_then(Value::as_object)
            .into_iter()
            .flat_map(|s| s.values())
            .filter_map(|d| d.get("order").and_then(Value::as_u64))
            .map(|o| o + 1)
            .max()
            .unwrap_or(0)
    }

    /// A new hand-off rule, or `None` (with the reason dropped under `item`)
    /// when it has no topic or names a target this manager may not hand off
    /// to. A condition the draft cannot express yet is left out with a note.
    fn rule_for(
        &mut self,
        topic: &str,
        target: &str,
        details: bool,
        identity: bool,
        item: &str,
    ) -> Option<Rule> {
        let item = Some(item).filter(|i| !i.is_empty());
        if topic.is_empty() {
            self.drop("handoffs", item, "a hand-off needs a topic");
            return None;
        }
        let target = target.trim();
        let target = if target == HUMAN_TARGET {
            Target::Human
        } else if let Some(agent) = self
            .ctx
            .candidates
            .agents
            .iter()
            .find(|a| a.id == target && a.id != self.ctx.agent_id)
        {
            Target::Agent(agent.id.clone())
        } else {
            self.drop(
                "handoffs",
                item,
                format!(
                    "`{target}` is not an agent you may hand off to — pick one of the agents \
                     shared with you, or a person"
                ),
            );
            return None;
        };
        let bind = match &target {
            Target::Agent(id) => {
                let (bind, missing) =
                    handoffs::derive_bind(self.ctx.live_specs.get(id), &self.draft);
                if !missing.is_empty() {
                    self.drop(
                        "handoffs",
                        item,
                        format!(
                            "kept, but `{id}` needs {} from a confirmed identity, which this \
                             agent has no check for yet — the setup's checklist asks for it",
                            missing.join(", ")
                        ),
                    );
                }
                bind
            }
            Target::Human => Map::new(),
        };
        let has_identity =
            handoffs::identity_writer(&self.draft).is_some() || self.identity_proposed;
        if identity && !has_identity {
            self.drop(
                "handoffs",
                item,
                "kept without the identity condition: the agent has no identity check yet",
            );
        }
        let has_details = !handoffs::detail_slots(&self.draft).is_empty();
        if details && !has_details {
            self.drop(
                "handoffs",
                item,
                "kept without the details condition: the agent collects no details yet",
            );
        }
        Some(Rule {
            route: None,
            topic: topic.to_string(),
            details: details && has_details,
            identity: identity && has_identity,
            target,
            bind,
        })
    }

    /// Add or change the setup's hand-off rules ("when it is about `topic`,
    /// hand over to `target`") and its fallback to a person, as one step.
    fn setup_handoffs(&mut self, wanted: Vec<SetupHandoff>, fallback: Option<bool>) -> bool {
        let mut h = handoffs::read(&self.draft);
        for w in wanted {
            let topic = w.topic.trim().to_string();
            let Some(rule) = self.rule_for(&topic, &w.target, w.details, w.identity, &topic) else {
                continue;
            };
            match h
                .rules
                .iter_mut()
                .find(|r| r.topic.trim().eq_ignore_ascii_case(&topic))
            {
                Some(existing) => {
                    existing.target = rule.target;
                    existing.details = rule.details;
                    existing.identity = rule.identity;
                    existing.bind = rule.bind;
                }
                None => h.rules.push(rule),
            }
        }
        if let Some(f) = fallback {
            h.fallback = f;
        }
        let mut candidate = self.draft.clone();
        handoffs::write(&mut candidate, &h);
        if candidate == self.draft {
            return false;
        }
        match self.adopt(candidate) {
            Ok(()) => true,
            Err(reason) => {
                self.drop("handoffs", None, reason);
                false
            }
        }
    }

    fn display(&mut self, display: String) -> Option<String> {
        let display = display.trim().to_string();
        if display.is_empty() {
            self.drop("display", None, "the name is empty");
            return None;
        }
        let candidate = self.with(&["profile", "display"], json!(display));
        match self.adopt(candidate) {
            Ok(()) => Some(display),
            Err(reason) => {
                self.drop("display", None, reason);
                None
            }
        }
    }

    fn model(&mut self, model: String) -> Option<String> {
        let model = model.trim().to_string();
        if !self.ctx.candidates.models.contains(&model) {
            self.drop(
                "model",
                Some(&model),
                "it is not a model you may use and grant — pick one of the chat models \
                 `list_grantable` names under `models.chat`",
            );
            return None;
        }
        let candidate = self.with(&["main", "model"], json!(model));
        self.granted.push((GrantKind::Model, model.clone()));
        match self.adopt(candidate) {
            Ok(()) => Some(model),
            Err(reason) => {
                self.granted.pop();
                self.drop("model", Some(&model), reason);
                None
            }
        }
    }

    fn task(&mut self, task: String) {
        let task = task.trim().to_string();
        if task.is_empty() {
            return self.drop("task", None, "the model proposed no task");
        }
        let candidate = self.with(&["main", "instructions", "orchestration"], json!(task));
        match self.adopt(candidate) {
            Ok(()) => {
                self.out.steps.task = Some(TaskStep {
                    orchestration: task,
                })
            }
            Err(reason) => self.drop("task", None, reason),
        }
    }

    fn tone(&mut self, tone: ToneProposal) {
        let extra = tone.response.trim().to_string();
        let mut chips: Vec<String> = Vec::new();
        let mut unknown: Vec<String> = Vec::new();
        for chip in tone
            .chips
            .iter()
            .map(|c| c.trim())
            .filter(|c| !c.is_empty())
        {
            if tone::TONES.iter().any(|(id, _)| *id == chip) {
                if !chips.iter().any(|c| c == chip) {
                    chips.push(chip.to_string());
                }
            } else {
                unknown.push(chip.to_string());
            }
        }
        if !unknown.is_empty() {
            self.drop(
                "tone",
                Some(&unknown.join(", ")),
                format!(
                    "not a tone the setup offers — the chips are: {}",
                    tone::tone_ids().join(", ")
                ),
            );
        }
        let language = tone
            .language
            .as_deref()
            .map(str::trim)
            .filter(|l| *l != tone::NO_LANGUAGE && !l.is_empty())
            .and_then(|l| {
                let known = tone::language_choices().iter().any(|c| c == l);
                if !known {
                    self.drop(
                        "tone",
                        Some(l),
                        format!(
                            "not an answer language the setup offers — use one of: {}",
                            tone::language_choices().join(", ")
                        ),
                    );
                }
                known.then(|| l.to_string())
            });
        let response = tone::response_text(&chips, language.as_deref(), &extra);
        if response.is_empty() {
            return self.drop("tone", None, "the model proposed no tone");
        }
        let candidate = self.with(&["main", "instructions", "response"], json!(response));
        match self.adopt(candidate) {
            Ok(()) => {
                self.out.steps.tone = Some(ToneStep {
                    response: extra,
                    chips,
                    language,
                })
            }
            Err(reason) => self.drop("tone", None, reason),
        }
    }

    fn scope(&mut self, scope: ScopeProposal) {
        let topics: Vec<String> = scope
            .topics
            .iter()
            .map(|t| t.trim())
            .filter(|t| !t.is_empty())
            .map(str::to_string)
            .collect();
        let refusal = scope.refusal.trim().to_string();
        let mut section = self
            .draft
            .get("scope")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        section.insert("topics".into(), json!(topics));
        section.insert("refusal".into(), json!(refusal));
        section.insert("strict".into(), json!(scope.strict));
        let candidate = self.with(&["scope"], Value::Object(section));
        match self.adopt(candidate) {
            Ok(()) => {
                self.out.steps.scope = Some(ScopeStep {
                    topics,
                    refusal,
                    strict: scope.strict,
                })
            }
            Err(reason) => self.drop("scope", None, reason),
        }
    }

    fn ability(&mut self, ability: AbilityProposal) {
        let id = ability.id.trim();
        if is_knowledge_tool(id) {
            return self.drop(
                "abilities",
                Some(id),
                "knowledge search comes with a knowledge base — propose the knowledge base \
                 instead",
            );
        }
        let Some(known) = self.ctx.candidates.abilities.iter().find(|a| a.id == id) else {
            return self.drop(
                "abilities",
                Some(id),
                "it is not an ability you may grant — only tools you hold yourself can be given \
                 to an agent",
            );
        };
        if self.out.steps.abilities.iter().any(|a| a.id == id) {
            return;
        }
        let mut tools: Vec<Value> = self
            .draft
            .pointer("/main/tools")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if !tools.iter().any(|t| t.as_str() == Some(id)) {
            tools.push(json!(id));
        }
        let candidate = self.with(&["main", "tools"], Value::Array(tools));
        self.granted.push((GrantKind::Tool, id.to_string()));
        match self.adopt(candidate) {
            Ok(()) => self.out.steps.abilities.push(AbilityStep {
                id: id.to_string(),
                name: known.name.clone(),
                why: ability.why.trim().to_string(),
            }),
            Err(reason) => {
                self.granted.pop();
                self.drop("abilities", Some(id), reason)
            }
        }
    }

    /// The collections this draft's grants, as stored and as offered so
    /// far, let it search, by name.
    fn granted_collections(&self) -> Vec<String> {
        let ids: Vec<String> = self
            .ctx
            .grants
            .iter()
            .map(|(k, r)| (k, r.to_string()))
            .chain(self.granted.iter().cloned())
            .filter(|(k, _)| *k == GrantKind::RagCollection)
            .map(|(_, r)| r)
            .collect();
        let mut names: Vec<String> = Vec::new();
        for id in ids {
            let name = self
                .ctx
                .candidates
                .knowledge
                .iter()
                .find(|k| k.id == id)
                .map_or(id, |k| k.name.clone());
            if !names.contains(&name) {
                names.push(name);
            }
        }
        names
    }

    /// Switch on the knowledge bases `wanted` names, all together, the way
    /// the abilities step's knowledge cards do.
    fn knowledge(&mut self, wanted: Vec<KnowledgeProposal>) {
        let offers = |id: &str| self.ctx.candidates.abilities.iter().any(|a| a.id == id);
        let (can_search, may_list) = (offers(RAG_SEARCH), offers(RAG_LIST));
        let mut picked: Vec<(Knowledge, String)> = Vec::new();
        for w in wanted {
            let name = w.name.trim();
            let Some(k) = self
                .ctx
                .candidates
                .knowledge
                .iter()
                .find(|k| k.name.trim().eq_ignore_ascii_case(name))
            else {
                self.drop(
                    "knowledge",
                    Some(name),
                    "it is not a knowledge base you may give an agent — pick one of those \
                     the abilities step lists",
                );
                continue;
            };
            if !can_search {
                self.drop(
                    "knowledge",
                    Some(name),
                    "you may not grant knowledge search, so the agent could not search it — \
                     ask an admin for the knowledge search tool",
                );
                continue;
            }
            if !picked.iter().any(|(p, _)| p.id == k.id) {
                picked.push((k.clone(), w.why.trim().to_string()));
            }
        }
        if picked.is_empty() {
            return;
        }
        let before = self.granted.len();
        for (k, _) in &picked {
            self.granted.push((GrantKind::RagCollection, k.id.clone()));
        }
        let names = self.granted_collections();
        let can_list = names.len() > 1 && may_list;
        self.granted.push((GrantKind::Tool, RAG_SEARCH.to_string()));
        if can_list {
            self.granted.push((GrantKind::Tool, RAG_LIST.to_string()));
        }
        let mut candidate = self.draft.clone();
        set_knowledge(&mut candidate, &names, can_list);
        match self.adopt(candidate) {
            Ok(()) => {
                for (k, why) in picked {
                    self.out.steps.knowledge.push(KnowledgeStep {
                        id: k.id,
                        name: k.name,
                        why,
                    });
                }
            }
            Err(reason) => {
                self.granted.truncate(before);
                for (k, _) in picked {
                    self.drop("knowledge", Some(&k.name), reason.clone());
                }
            }
        }
    }

    fn slot(&mut self, slot: SlotProposal) {
        let Some(name) = ident(&slot.name) else {
            return self.drop(
                "slots",
                Some(&slot.name),
                "its name has no letters to make a slot name from",
            );
        };
        if self.draft.pointer(&format!("/state/{name}")).is_some() {
            return self.drop(
                "slots",
                Some(&name),
                format!("the draft already collects `{name}`; the existing definition stays"),
            );
        }
        let Some(mut def) = slot_def(&slot.kind, &slot.choices) else {
            return self.drop(
                "slots",
                Some(&name),
                format!(
                    "`{}` is not a kind of information the assistant knows — use one of: {}",
                    slot.kind,
                    SLOT_TYPES.join(", ")
                ),
            );
        };
        let label = slot.label.trim().to_string();
        if !label.is_empty() {
            def["description"] = json!(label);
        }
        if self.ordered {
            def["order"] = json!(self.next_order());
        }
        let mut candidate = self.draft.clone();
        handoffs::with_details(&mut candidate, |d| {
            set_at(d, &["state", &name], def.clone());
        });
        match self.adopt(candidate) {
            Ok(()) => self.out.steps.slots.push(SlotStep {
                name,
                label,
                kind: slot.kind,
                def,
            }),
            Err(reason) => self.drop("slots", Some(&name), reason),
        }
    }

    fn identity(&mut self, identity: IdentityProposal) {
        let method = identity.method.trim();
        if !IDENTITY_METHODS.contains(&method) {
            return self.drop(
                "identity",
                Some(method),
                format!(
                    "`{method}` is not an identity check — use one of: {}",
                    IDENTITY_METHODS.join(", ")
                ),
            );
        }
        self.out.steps.identity = Some(IdentityStep {
            method: method.to_string(),
            why: identity.why.trim().to_string(),
        });
    }

    /// A proposed rule for the setup's hand-off step, added to the rules the
    /// draft has; one about a topic that has a rule already is left out.
    fn handoff(&mut self, handoff: HandoffProposal) {
        let item = handoff.name.trim().to_string();
        let topic = handoff.topic.trim().to_string();
        let mut h = handoffs::read(&self.draft);
        if h.rules
            .iter()
            .any(|r| r.topic.trim().eq_ignore_ascii_case(&topic))
        {
            return self.drop(
                "handoffs",
                Some(&item),
                format!("the draft already hands off requests about “{topic}”; it stays as it is"),
            );
        }
        let condition = handoff.condition.as_deref().map(str::trim);
        let Some(rule) = self.rule_for(
            &topic,
            &handoff.target,
            condition == Some(CONDITION_DETAILS),
            condition == Some(CONDITION_IDENTITY),
            &item,
        ) else {
            return;
        };
        let target_name = match &rule.target {
            Target::Human => "a person".to_string(),
            Target::Agent(id) => self
                .ctx
                .candidates
                .agents
                .iter()
                .find(|a| &a.id == id)
                .map_or_else(|| id.clone(), |a| a.name.clone()),
        };
        let (details, identity) = (rule.details, rule.identity);
        h.rules.push(rule);
        let mut candidate = self.draft.clone();
        handoffs::write(&mut candidate, &h);
        let Some(name) = handoffs::read(&candidate)
            .rules
            .into_iter()
            .find(|r| r.topic == topic)
            .and_then(|r| r.route)
        else {
            return self.drop("handoffs", Some(&item), "it does not read back as a rule");
        };
        let route = candidate["routes"][&name].clone();
        match self.adopt(candidate) {
            Ok(()) => self.out.steps.handoffs.push(HandoffStep {
                name,
                topic,
                target: handoff.target.trim().to_string(),
                target_name,
                details,
                identity,
                route,
            }),
            Err(reason) => self.drop("handoffs", Some(&item), reason),
        }
    }

    fn test(&mut self, test: TestProposal) {
        let name = test.name.trim().to_string();
        if self.out.steps.tests.len() >= MAX_TESTS {
            return self.drop(
                "tests",
                Some(&name),
                format!("the assistant offers at most {MAX_TESTS} test cases"),
            );
        }
        if name.is_empty() || name.chars().count() > MAX_CASE_NAME_CHARS {
            return self.drop(
                "tests",
                Some(&name),
                format!("a test case needs a name of 1 to {MAX_CASE_NAME_CHARS} characters"),
            );
        }
        if !self.tests.insert(name.clone()) {
            return self.drop(
                "tests",
                Some(&name),
                "another proposed test case has this name",
            );
        }
        let mut contains: Vec<String> = clean(&test.answer_contains);
        let not_contains = clean(&test.answer_not_contains);
        let out_of_scope = match test.kind.as_str() {
            "out_of_scope" => true,
            "in_scope" => false,
            other => {
                return self.drop(
                    "tests",
                    Some(&name),
                    format!(
                        "`{other}` is not a kind of test case — use `in_scope` or `out_of_scope`"
                    ),
                );
            }
        };
        if out_of_scope {
            let Some(refusal) = self
                .draft
                .pointer("/scope/refusal")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|r| !r.is_empty())
            else {
                return self.drop(
                    "tests",
                    Some(&name),
                    "an out-of-scope case expects the refusal, and the draft has none — accept \
                     the scope step first",
                );
            };
            contains = vec![refusal.to_string()];
        }
        let script: Vec<Value> = test
            .messages
            .iter()
            .map(|m| m.trim())
            .filter(|m| !m.is_empty())
            .map(|m| json!({ "say": m }))
            .collect();
        let mut expect = Map::new();
        expect.insert("finished".into(), json!(true));
        let mut answer = Map::new();
        if !contains.is_empty() {
            answer.insert("contains".into(), json!(contains));
        }
        if !not_contains.is_empty() {
            answer.insert("not_contains".into(), json!(not_contains));
        }
        if !answer.is_empty() {
            expect.insert("answer".into(), Value::Object(answer));
        }
        let script = Value::Array(script);
        let expect = Value::Object(expect);
        match eval::parse_case(&script, &expect) {
            Ok(_) => self.out.steps.tests.push(TestStep {
                name,
                kind: test.kind,
                script,
                expect,
            }),
            Err(issues) => {
                let reason = issues
                    .iter()
                    .map(|i| format!("at `{}`: {}", i.path, i.message))
                    .collect::<Vec<_>>()
                    .join("; ");
                self.drop(
                    "tests",
                    Some(&name),
                    format!("it is not a valid test case — {reason}"),
                )
            }
        }
    }
}

/// Knowledge search wired to the collections `names`, as `setKnowledge` in
/// `web/src/lib/agent-setup.ts` does it: one collection is bound as a
/// constant; several leave the choice to the model, and add the listing
/// tool when `can_list`.
pub fn set_knowledge(draft: &mut Value, names: &[String], can_list: bool) {
    let mut tools: Vec<Value> = draft
        .pointer("/main/tools")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut add = |id: &str| {
        if !tools.iter().any(|t| t.as_str() == Some(id)) {
            tools.push(json!(id));
        }
    };
    add(RAG_SEARCH);
    if names.len() > 1 && can_list {
        add(RAG_LIST);
    }
    if let [one] = names {
        tools.retain(|t| t.as_str() != Some(RAG_LIST));
        if let Some(resources) = draft
            .pointer_mut("/main/tool_resources")
            .and_then(Value::as_object_mut)
        {
            resources.remove(RAG_LIST);
        }
        set_at(
            draft,
            &["main", "tool_resources", RAG_SEARCH, "bind", "collection"],
            json!({ "const": one }),
        );
    } else if let Some(bind) = draft
        .pointer_mut(&format!("/main/tool_resources/{RAG_SEARCH}/bind"))
        .and_then(Value::as_object_mut)
    {
        bind.remove("collection");
    }
    set_at(draft, &["main", "tools"], Value::Array(tools));
}

fn clean(texts: &[String]) -> Vec<String> {
    texts
        .iter()
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect()
}

/// `value` at `pointer` in `doc`, creating the objects on the way (and
/// replacing whatever non-object stood there).
fn set_at(doc: &mut Value, pointer: &[&str], value: Value) {
    let Some((last, parents)) = pointer.split_last() else {
        *doc = value;
        return;
    };
    let mut at = doc;
    for key in parents {
        if !at.is_object() {
            *at = json!({});
        }
        at = at
            .as_object_mut()
            .expect("made an object above")
            .entry(key.to_string())
            .or_insert_with(|| json!({}));
    }
    if !at.is_object() {
        *at = json!({});
    }
    at.as_object_mut()
        .expect("made an object above")
        .insert(last.to_string(), value);
}

/// `text` as a spec name: lowercase letters, digits and `_`, starting with
/// a letter.
fn ident(text: &str) -> Option<String> {
    let mut out = String::new();
    for c in text.trim().chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('_') {
            out.push('_');
        }
    }
    let out: String = out
        .trim_start_matches(|c: char| !c.is_ascii_lowercase())
        .trim_end_matches('_')
        .chars()
        .take(MAX_IDENT_LEN)
        .collect();
    let out = out.trim_end_matches('_').to_string();
    (!out.is_empty()).then_some(out)
}

/// The slot a friendly type stands for; the model may set it.
fn slot_def(kind: &str, choices: &[String]) -> Option<Value> {
    let def = match kind {
        "text" => json!({ "type": "string", "max_length": TEXT_MAX }),
        "long_text" => json!({ "type": "string", "max_length": LONG_TEXT_MAX }),
        "email" => json!({ "type": "email" }),
        "number" => json!({ "type": "number" }),
        "whole_number" => json!({ "type": "integer" }),
        "yes_no" => json!({ "type": "boolean" }),
        "choice" => json!({ "type": "enum", "values": clean(choices) }),
        _ => return None,
    };
    let mut def = def;
    def["set_by"] = json!(["llm"]);
    Some(def)
}

#[cfg(test)]
mod tests;
