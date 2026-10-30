// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The wire types of `/api/v0/admin/classification`.

use aiplane_core::server::classification::admin::{ClassInput, ClassRecord};
use aiplane_core::server::classification::{Action, Level};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LevelBody {
    pub id: String,
    pub rank: u32,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub color: String,
    #[serde(default)]
    pub marking_text: String,
    #[serde(default)]
    pub mip_label_id: String,
    #[serde(default)]
    pub filename_tag: String,
}

impl From<&Level> for LevelBody {
    fn from(level: &Level) -> Self {
        Self {
            id: level.id.clone(),
            rank: level.rank,
            name: level.name.clone(),
            description: level.description.clone(),
            color: level.color.clone(),
            marking_text: level.marking_text.clone(),
            mip_label_id: level.mip_label_id.clone(),
            filename_tag: level.filename_tag.clone(),
        }
    }
}

impl From<LevelBody> for Level {
    fn from(level: LevelBody) -> Self {
        Self {
            id: level.id,
            rank: level.rank,
            name: level.name,
            description: level.description,
            color: level.color,
            marking_text: level.marking_text,
            mip_label_id: level.mip_label_id,
            filename_tag: level.filename_tag,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ClassBody {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub level_id: String,
    #[serde(default)]
    pub patterns: Vec<String>,
    #[serde(default)]
    pub rule_question: String,
    #[serde(default)]
    pub rule_positive: String,
    #[serde(default)]
    pub rule_negative: String,
    pub rule_threshold: f64,
    /// `raise`, `confirm` or `block`.
    pub action: String,
}

impl From<&ClassInput> for ClassBody {
    fn from(class: &ClassInput) -> Self {
        Self {
            id: class.id.clone(),
            name: class.name.clone(),
            description: class.description.clone(),
            level_id: class.level_id.clone(),
            patterns: class.patterns.clone(),
            rule_question: class.rule_question.clone(),
            rule_positive: class.rule_positive.clone(),
            rule_negative: class.rule_negative.clone(),
            rule_threshold: class.rule_threshold,
            action: class.action.as_str().to_owned(),
        }
    }
}

impl ClassBody {
    pub fn into_input(self, action: Action) -> ClassInput {
        ClassInput {
            id: self.id,
            name: self.name,
            description: self.description,
            level_id: self.level_id,
            patterns: self.patterns,
            rule_question: self.rule_question,
            rule_positive: self.rule_positive,
            rule_negative: self.rule_negative,
            rule_threshold: self.rule_threshold,
            action,
        }
    }
}

/// The state of a class's uploaded word list.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct TermsView {
    pub count: u64,
    pub uploaded_by: Option<String>,
    pub uploaded_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct ClassView {
    #[serde(flatten)]
    pub class: ClassBody,
    pub terms: TermsView,
}

impl From<&ClassRecord> for ClassView {
    fn from(record: &ClassRecord) -> Self {
        Self {
            class: (&record.class).into(),
            terms: TermsView {
                count: record.terms.count,
                uploaded_by: record.terms.uploaded_by.clone(),
                uploaded_at: record.terms.uploaded_at.clone(),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MarkingBody {
    /// `mip`, `tscp`, `sensitivity` or `stamp`.
    pub kind: String,
    pub value: String,
    pub level_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AssignmentBody {
    /// `pool`, `tool`, `mcp`, `connector` or `collection`.
    pub kind: String,
    pub id: String,
    pub level_id: String,
    /// `hide` or `gate`.
    pub over: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct OutputBody {
    pub visible: bool,
    pub metadata: bool,
    pub filename: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PolicyBody {
    pub default_level: String,
    /// `never` or `with_reason`.
    pub lowering: String,
    /// `ask` or `block`.
    pub gate_on_match: String,
    /// Whether a person's approval may send a turn to a model that is not
    /// cleared for the conversation's level.
    pub user_override: bool,
    pub link_share_max_level: String,
    pub unknown_label_level: String,
    pub unreadable_level: String,
    #[serde(default)]
    pub system_one_model: String,
    /// `monitor` or `enforce`.
    pub mode: String,
    #[serde(default)]
    pub mip_tenant_id: String,
    pub output: OutputBody,
}

/// The policy fields a template sets; the others stay as the operator has them.
#[derive(Debug, Clone, Default, Serialize, schemars::JsonSchema)]
pub struct TemplatePolicy {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_level: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lowering: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gate_on_match: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_share_max_level: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unknown_label_level: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unreadable_level: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<OutputBody>,
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct TemplateView {
    /// `iso`, `tisax`, `bsi` or `tlp`.
    pub id: &'static str,
    pub name: String,
    pub description: String,
    pub levels: Vec<LevelBody>,
    pub classes: Vec<ClassBody>,
    pub markings: Vec<MarkingBody>,
    pub policy: TemplatePolicy,
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct DestinationView {
    /// `pool`, `tool`, `mcp`, `connector` or `collection`.
    pub kind: &'static str,
    pub id: String,
    pub name: String,
    pub description: String,
    /// `null`: unrated, the lowest level applies.
    pub level_id: Option<String>,
    pub over: &'static str,
    /// Data given to this destination leaves AIplane.
    pub egress: bool,
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct SystemOneModelView {
    pub id: String,
    /// `null` while no pool serving it is rated.
    pub level_id: Option<String>,
    pub cleared_for_highest: bool,
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct ClassificationState {
    /// More than one level exists.
    pub active: bool,
    pub levels: Vec<LevelBody>,
    pub classes: Vec<ClassView>,
    pub markings: Vec<MarkingBody>,
    pub destinations: Vec<DestinationView>,
    pub policy: PolicyBody,
    pub templates: Vec<TemplateView>,
    pub system_one_models: Vec<SystemOneModelView>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct SaveBody {
    pub levels: Vec<LevelBody>,
    pub classes: Vec<ClassBody>,
    #[serde(default)]
    pub markings: Vec<MarkingBody>,
    #[serde(default)]
    pub assignments: Vec<AssignmentBody>,
    pub policy: PolicyBody,
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct TermsPreview {
    pub count: usize,
    pub duplicates: usize,
    pub too_short: Vec<String>,
    /// The first ten terms.
    pub sample: Vec<String>,
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct TermsUploaded {
    #[serde(flatten)]
    pub preview: TermsPreview,
    pub uploaded_at: String,
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct InspectedMarking {
    pub kind: &'static str,
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct InspectResult {
    pub markings: Vec<InspectedMarking>,
    pub unreadable: bool,
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct TestResult {
    pub level_id: String,
    pub causes: Vec<String>,
    pub classes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct EventView {
    pub session_id: String,
    pub at: String,
    pub from_level: Option<String>,
    pub to_level: String,
    pub cause: String,
    pub actor: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct EventsView {
    pub events: Vec<EventView>,
}
