// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The rules a saved scheme must satisfy. Every refusal names the field and
//! says what to change.

use std::collections::{HashMap, HashSet};

use aiplane_core::server::classification::admin::Replacement;
use aiplane_core::server::classification::{
    Action, Assignment, DestinationKind, Level, MarkingKind, MarkingRule, Over,
};
use aiplane_features::server::classification::PATTERN_IDS;

use super::types::SaveBody;

/// A refused save: the field and the sentence for the person fixing it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invalid {
    pub field: String,
    pub message: String,
}

fn invalid<T>(field: impl Into<String>, message: impl Into<String>) -> Result<T, Invalid> {
    Err(Invalid {
        field: field.into(),
        message: message.into(),
    })
}

/// What the save is checked against besides itself.
pub struct Context<'a> {
    /// Every destination that exists, as `(kind, id)`; `None` skips the check.
    pub destinations: Option<&'a HashSet<(String, String)>>,
    /// For each System One model, the pools serving it.
    pub system_one_pools: &'a HashMap<String, Vec<String>>,
}

const COLORS: &[&str] = &["neutral", "info", "success", "warning", "error"];
const LOWERING: &[&str] = &["never", "with_reason"];
const GATE_ON_MATCH: &[&str] = &["ask", "block"];
const MODES: &[&str] = &["monitor", "enforce"];

fn is_slug(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn guid(value: &str) -> Option<String> {
    let normal = value.trim().trim_matches(['{', '}']).to_ascii_lowercase();
    let groups: Vec<&str> = normal.split('-').collect();
    let shape = [8, 4, 4, 4, 12];
    (groups.len() == 5
        && groups
            .iter()
            .zip(shape)
            .all(|(g, n)| g.len() == n && g.bytes().all(|b| b.is_ascii_hexdigit())))
    .then_some(normal)
}

fn one_of(field: &str, value: &str, allowed: &[&str]) -> Result<(), Invalid> {
    if allowed.contains(&value) {
        return Ok(());
    }
    invalid(
        field,
        format!(
            "{field}: `{value}` is not allowed; use one of {}",
            allowed.join(", ")
        ),
    )
}

/// Check `body` and turn it into what the database stores.
pub fn validate(body: &SaveBody, context: &Context<'_>) -> Result<Replacement, Invalid> {
    if body.levels.is_empty() {
        return invalid(
            "levels",
            "levels: at least one level is needed; add a level, for example `public` at rank 0",
        );
    }
    let mut ranks: Vec<u32> = body.levels.iter().map(|l| l.rank).collect();
    ranks.sort_unstable();
    if ranks
        .iter()
        .enumerate()
        .any(|(i, rank)| *rank as usize != i)
    {
        return invalid(
            "levels",
            format!(
                "levels: the ranks are {ranks:?}; they must be 0 to {} without gaps or repeats, lowest sensitivity first",
                body.levels.len() - 1
            ),
        );
    }
    let mut levels: Vec<Level> = Vec::new();
    let mut ids: HashSet<&str> = HashSet::new();
    let mut tags: HashSet<&str> = HashSet::new();
    for (i, level) in body.levels.iter().enumerate() {
        let at = format!("levels[{i}]");
        if !is_slug(&level.id) {
            return invalid(
                format!("{at}.id"),
                format!(
                    "{at}.id: `{}` is not an id; use lowercase letters, digits and underscores",
                    level.id
                ),
            );
        }
        if !ids.insert(&level.id) {
            return invalid(
                format!("{at}.id"),
                format!(
                    "{at}.id: `{}` is used by two levels; give each level its own id",
                    level.id
                ),
            );
        }
        if level.name.trim().is_empty() {
            return invalid(
                format!("{at}.name"),
                format!("{at}.name: a level needs a name"),
            );
        }
        one_of(&format!("{at}.color"), &level.color, COLORS)?;
        if !level.filename_tag.is_empty() {
            if !is_slug(&level.filename_tag) {
                return invalid(
                    format!("{at}.filename_tag"),
                    format!(
                        "{at}.filename_tag: `{}` is not a tag; use lowercase letters, digits and underscores",
                        level.filename_tag
                    ),
                );
            }
            if !tags.insert(&level.filename_tag) {
                return invalid(
                    format!("{at}.filename_tag"),
                    format!(
                        "{at}.filename_tag: `{}` is used by two levels; a file name could not tell them apart",
                        level.filename_tag
                    ),
                );
            }
        }
        let mut stored: Level = level.clone().into();
        if !level.mip_label_id.trim().is_empty() {
            let Some(normal) = guid(&level.mip_label_id) else {
                return invalid(
                    format!("{at}.mip_label_id"),
                    format!(
                        "{at}.mip_label_id: `{}` is not a label id; paste the label's GUID from Microsoft Purview, or leave it empty",
                        level.mip_label_id
                    ),
                );
            };
            stored.mip_label_id = normal;
        } else {
            stored.mip_label_id = String::new();
        }
        levels.push(stored);
    }
    let level_exists = |field: &str, value: &str| -> Result<(), Invalid> {
        if ids.contains(value) {
            return Ok(());
        }
        invalid(
            field,
            format!("{field}: there is no level `{value}`; pick one of the levels in this scheme"),
        )
    };

    let mut classes = Vec::new();
    let mut class_ids: HashSet<&str> = HashSet::new();
    for (i, class) in body.classes.iter().enumerate() {
        let at = format!("classes[{i}]");
        if !is_slug(&class.id) {
            return invalid(
                format!("{at}.id"),
                format!(
                    "{at}.id: `{}` is not an id; use lowercase letters, digits and underscores",
                    class.id
                ),
            );
        }
        if !class_ids.insert(&class.id) {
            return invalid(
                format!("{at}.id"),
                format!("{at}.id: `{}` is used by two classes", class.id),
            );
        }
        if class.name.trim().is_empty() {
            return invalid(
                format!("{at}.name"),
                format!("{at}.name: a class needs a name"),
            );
        }
        level_exists(&format!("{at}.level_id"), &class.level_id)?;
        if !(0.0..=1.0).contains(&class.rule_threshold) {
            return invalid(
                format!("{at}.rule_threshold"),
                format!(
                    "{at}.rule_threshold: {} is outside 0 to 1",
                    class.rule_threshold
                ),
            );
        }
        for pattern in &class.patterns {
            if !PATTERN_IDS.contains(&pattern.as_str()) {
                return invalid(
                    format!("{at}.patterns"),
                    format!(
                        "{at}.patterns: `{pattern}` is not a built-in pattern; use {}",
                        PATTERN_IDS.join(", ")
                    ),
                );
            }
        }
        let Some(action) = Action::parse(&class.action) else {
            return invalid(
                format!("{at}.action"),
                format!(
                    "{at}.action: `{}` is not an action; use raise, confirm or block",
                    class.action
                ),
            );
        };
        classes.push(class.clone().into_input(action));
    }

    let mut markings = Vec::new();
    let mut marking_keys: HashSet<(&str, &str)> = HashSet::new();
    for (i, marking) in body.markings.iter().enumerate() {
        let at = format!("markings[{i}]");
        let Some(kind) = MarkingKind::parse(&marking.kind) else {
            return invalid(
                format!("{at}.kind"),
                format!(
                    "{at}.kind: `{}` is not a marking kind; use mip, tscp, sensitivity or stamp",
                    marking.kind
                ),
            );
        };
        if marking.value.trim().is_empty() {
            return invalid(
                format!("{at}.value"),
                format!("{at}.value: a marking needs the text or id it is recognised by"),
            );
        }
        level_exists(&format!("{at}.level_id"), &marking.level_id)?;
        if !marking_keys.insert((&marking.kind, &marking.value)) {
            return invalid(
                format!("{at}.value"),
                format!(
                    "{at}.value: the {} marking `{}` is listed twice",
                    marking.kind, marking.value
                ),
            );
        }
        markings.push(MarkingRule {
            kind,
            value: marking.value.clone(),
            level_id: marking.level_id.clone(),
        });
    }

    let mut assignments = Vec::new();
    let mut assignment_keys: HashSet<(&str, &str)> = HashSet::new();
    for (i, assignment) in body.assignments.iter().enumerate() {
        let at = format!("assignments[{i}]");
        let Some(kind) = DestinationKind::parse(&assignment.kind) else {
            return invalid(
                format!("{at}.kind"),
                format!(
                    "{at}.kind: `{}` is not a destination kind; use pool, tool, mcp, connector or collection",
                    assignment.kind
                ),
            );
        };
        let Some(over) = Over::parse(&assignment.over) else {
            return invalid(
                format!("{at}.over"),
                format!(
                    "{at}.over: `{}` is not allowed; use hide or gate",
                    assignment.over
                ),
            );
        };
        level_exists(&format!("{at}.level_id"), &assignment.level_id)?;
        if !assignment_keys.insert((&assignment.kind, &assignment.id)) {
            return invalid(
                format!("{at}.id"),
                format!(
                    "{at}.id: the {} `{}` is rated twice",
                    assignment.kind, assignment.id
                ),
            );
        }
        if let Some(known) = context.destinations
            && !known.contains(&(assignment.kind.clone(), assignment.id.clone()))
        {
            return invalid(
                format!("{at}.id"),
                format!(
                    "{at}.id: there is no {} `{}`; remove the rating or pick an existing one",
                    assignment.kind, assignment.id
                ),
            );
        }
        assignments.push(Assignment {
            kind,
            id: assignment.id.clone(),
            level_id: assignment.level_id.clone(),
            over,
        });
    }

    let policy = &body.policy;
    level_exists("policy.default_level", &policy.default_level)?;
    level_exists("policy.link_share_max_level", &policy.link_share_max_level)?;
    level_exists("policy.unknown_label_level", &policy.unknown_label_level)?;
    level_exists("policy.unreadable_level", &policy.unreadable_level)?;
    one_of("policy.lowering", &policy.lowering, LOWERING)?;
    one_of("policy.gate_on_match", &policy.gate_on_match, GATE_ON_MATCH)?;
    one_of("policy.mode", &policy.mode, MODES)?;

    let model = policy.system_one_model.trim();
    let needs_model = body
        .classes
        .iter()
        .any(|c| !c.rule_question.trim().is_empty());
    let uses_gate = assignments.iter().any(|a| a.over == Over::Gate);
    if model.is_empty() {
        if uses_gate {
            return invalid(
                "policy.system_one_model",
                "policy.system_one_model: a destination is set to gate, which asks the System One model about what is sent; choose a model, or set the destination to hide",
            );
        }
    } else if needs_model || uses_gate {
        let highest = levels
            .iter()
            .max_by_key(|l| l.rank)
            .map(|l| l.rank)
            .unwrap_or(0);
        let Some(pools) = context.system_one_pools.get(model) else {
            return invalid(
                "policy.system_one_model",
                format!(
                    "policy.system_one_model: no System One pool serves `{model}`; choose one of the listed models"
                ),
            );
        };
        let rank_of = |pool: &str| {
            assignments
                .iter()
                .find(|a| a.kind == DestinationKind::Pool && a.id == pool)
                .and_then(|a| levels.iter().find(|l| l.id == a.level_id))
                .map_or(0, |l| l.rank)
        };
        if pools.iter().map(|p| rank_of(p)).min().unwrap_or(0) < highest {
            return invalid(
                "policy.system_one_model",
                format!(
                    "policy.system_one_model: `{model}` is not cleared for the highest level, yet it reads the text it classifies; rate its pool at the highest level or choose another model"
                ),
            );
        }
    }

    let output = &policy.output;
    let flag = |value: bool| if value { "true" } else { "false" }.to_owned();
    Ok(Replacement {
        levels,
        classes,
        markings,
        assignments,
        policy: vec![
            (
                "classification.default_level".into(),
                policy.default_level.clone(),
            ),
            ("classification.lowering".into(), policy.lowering.clone()),
            (
                "classification.gate_on_match".into(),
                policy.gate_on_match.clone(),
            ),
            (
                "classification.user_override".into(),
                policy.user_override.to_string(),
            ),
            (
                "classification.link_share_max_level".into(),
                policy.link_share_max_level.clone(),
            ),
            (
                "classification.unknown_label_level".into(),
                policy.unknown_label_level.clone(),
            ),
            (
                "classification.unreadable_level".into(),
                policy.unreadable_level.clone(),
            ),
            ("classification.system_one_model".into(), model.to_owned()),
            ("classification.mode".into(), policy.mode.clone()),
            (
                "classification.mip_tenant_id".into(),
                policy.mip_tenant_id.trim().to_owned(),
            ),
            ("classification.output.visible".into(), flag(output.visible)),
            (
                "classification.output.metadata".into(),
                flag(output.metadata),
            ),
            (
                "classification.output.filename".into(),
                flag(output.filename),
            ),
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::super::templates;
    use super::super::types::{AssignmentBody, LevelBody, OutputBody, PolicyBody, TemplateView};
    use super::*;
    use session_core::i18n::Lang;

    fn context() -> (HashMap<String, Vec<String>>, ()) {
        (
            HashMap::from([("guard".to_owned(), vec!["guard-pool".to_owned()])]),
            (),
        )
    }

    fn body_of(template: &TemplateView) -> SaveBody {
        let last = template.levels.last().unwrap().id.clone();
        let first = template.levels[0].id.clone();
        let policy = &template.policy;
        SaveBody {
            levels: template.levels.clone(),
            classes: template.classes.clone(),
            markings: template.markings.clone(),
            assignments: Vec::new(),
            policy: PolicyBody {
                default_level: policy.default_level.clone().unwrap_or(first.clone()),
                lowering: policy.lowering.clone().unwrap_or("never".into()),
                gate_on_match: policy.gate_on_match.clone().unwrap_or("ask".into()),
                user_override: true,
                link_share_max_level: policy.link_share_max_level.clone().unwrap_or(first),
                unknown_label_level: policy.unknown_label_level.clone().unwrap_or(last.clone()),
                unreadable_level: policy.unreadable_level.clone().unwrap_or(last),
                system_one_model: String::new(),
                mode: "enforce".into(),
                mip_tenant_id: String::new(),
                output: policy.output.clone().unwrap_or(OutputBody {
                    visible: false,
                    metadata: false,
                    filename: false,
                }),
            },
        }
    }

    fn check(body: &SaveBody) -> Result<Replacement, Invalid> {
        let (pools, ()) = context();
        validate(
            body,
            &Context {
                destinations: None,
                system_one_pools: &pools,
            },
        )
    }

    fn refused(body: &SaveBody) -> Invalid {
        check(body).expect_err("the save is refused")
    }

    fn iso() -> SaveBody {
        body_of(&templates::all(Lang::En)[0])
    }

    #[test]
    fn every_template_passes_the_rules_it_is_saved_under() {
        for template in templates::all(Lang::En) {
            let replacement = check(&body_of(&template)).unwrap_or_else(|e| {
                panic!("{} is refused: {} ({})", template.id, e.message, e.field)
            });
            assert_eq!(replacement.levels.len(), template.levels.len());
            assert_eq!(replacement.classes.len(), template.classes.len());
        }
        assert_eq!(templates::all(Lang::En).len(), 4);
    }

    #[test]
    fn a_template_has_the_levels_its_standard_defines() {
        let ids = |i: usize| -> Vec<String> {
            templates::all(Lang::En)[i]
                .levels
                .iter()
                .map(|l| l.id.clone())
                .collect()
        };
        assert_eq!(
            ids(0),
            [
                "public",
                "internal",
                "confidential",
                "strictly_confidential"
            ]
        );
        assert_eq!(ids(2), ["normal", "high", "very_high"]);
        assert_eq!(ids(3), ["clear", "green", "amber", "amber_strict", "red"]);
    }

    #[test]
    fn a_scheme_needs_a_level() {
        let mut body = iso();
        body.levels.clear();
        assert_eq!(refused(&body).field, "levels");
    }

    #[test]
    fn ranks_must_be_contiguous_from_zero() {
        let mut body = iso();
        body.levels[2].rank = 5;
        let error = refused(&body);
        assert_eq!(error.field, "levels");
        assert!(error.message.contains("0 to 3"), "{}", error.message);
    }

    #[test]
    fn ids_are_unique_slugs() {
        let mut body = iso();
        body.levels[1].id = "Internal Use".into();
        assert_eq!(refused(&body).field, "levels[1].id");
        let mut body = iso();
        body.levels[1].id = "public".into();
        assert_eq!(refused(&body).field, "levels[1].id");
        let mut body = iso();
        body.classes[1].id = body.classes[0].id.clone();
        assert_eq!(refused(&body).field, "classes[1].id");
    }

    #[test]
    fn filename_tags_are_unique_slugs() {
        let mut body = iso();
        body.levels[2].filename_tag = body.levels[1].filename_tag.clone();
        assert_eq!(refused(&body).field, "levels[2].filename_tag");
        let mut body = iso();
        body.levels[2].filename_tag = "not a tag".into();
        assert_eq!(refused(&body).field, "levels[2].filename_tag");
    }

    #[test]
    fn a_label_id_must_be_a_guid_and_is_stored_normalised() {
        let mut body = iso();
        body.levels[2].mip_label_id = "nope".into();
        assert_eq!(refused(&body).field, "levels[2].mip_label_id");
        body.levels[2].mip_label_id = "{3F2504E0-4F89-11D3-9A0C-0305E82C3301}".into();
        let stored = check(&body).unwrap();
        assert_eq!(
            stored.levels[2].mip_label_id,
            "3f2504e0-4f89-11d3-9a0c-0305e82c3301"
        );
    }

    #[test]
    fn references_to_levels_must_exist() {
        let mut body = iso();
        body.classes[0].level_id = "gone".into();
        assert_eq!(refused(&body).field, "classes[0].level_id");
        let mut body = iso();
        body.markings[0].level_id = "gone".into();
        assert_eq!(refused(&body).field, "markings[0].level_id");
        let mut body = iso();
        body.policy.link_share_max_level = "gone".into();
        assert_eq!(refused(&body).field, "policy.link_share_max_level");
        let mut body = iso();
        body.assignments.push(AssignmentBody {
            kind: "tool".into(),
            id: "search_web".into(),
            level_id: "gone".into(),
            over: "hide".into(),
        });
        assert_eq!(refused(&body).field, "assignments[0].level_id");
    }

    #[test]
    fn thresholds_patterns_and_actions_are_checked() {
        let mut body = iso();
        body.classes[0].rule_threshold = 1.5;
        assert_eq!(refused(&body).field, "classes[0].rule_threshold");
        let mut body = iso();
        body.classes[0].patterns = vec!["ssn".into()];
        assert_eq!(refused(&body).field, "classes[0].patterns");
        let mut body = iso();
        body.classes[0].action = "shout".into();
        assert_eq!(refused(&body).field, "classes[0].action");
    }

    #[test]
    fn a_missing_destination_is_refused_when_destinations_are_known() {
        let mut body = iso();
        body.assignments.push(AssignmentBody {
            kind: "pool".into(),
            id: "ghost".into(),
            level_id: "internal".into(),
            over: "hide".into(),
        });
        let known = HashSet::from([("pool".to_owned(), "real".to_owned())]);
        let (pools, ()) = context();
        let error = validate(
            &body,
            &Context {
                destinations: Some(&known),
                system_one_pools: &pools,
            },
        )
        .unwrap_err();
        assert_eq!(error.field, "assignments[0].id");
    }

    #[test]
    fn the_system_one_model_must_be_cleared_for_the_highest_level() {
        let mut body = iso();
        body.policy.system_one_model = "guard".into();
        let error = refused(&body);
        assert_eq!(error.field, "policy.system_one_model");
        assert!(error.message.contains("not cleared"), "{}", error.message);
        body.assignments.push(AssignmentBody {
            kind: "pool".into(),
            id: "guard-pool".into(),
            level_id: "strictly_confidential".into(),
            over: "hide".into(),
        });
        assert!(check(&body).is_ok());
        body.policy.system_one_model = "elsewhere".into();
        assert!(refused(&body).message.contains("no System One pool"));
    }

    #[test]
    fn a_gate_needs_a_model_and_a_gate_with_an_uncleared_model_is_refused() {
        let mut body = iso();
        body.classes.clear();
        body.assignments.push(AssignmentBody {
            kind: "tool".into(),
            id: "search_web".into(),
            level_id: "internal".into(),
            over: "gate".into(),
        });
        assert!(refused(&body).message.contains("gate"));
        body.policy.system_one_model = "guard".into();
        assert!(refused(&body).message.contains("not cleared"));
    }

    #[test]
    fn a_policy_choice_outside_its_options_is_refused() {
        let mut body = iso();
        body.policy.mode = "off".into();
        assert_eq!(refused(&body).field, "policy.mode");
        let mut body = iso();
        body.levels[0] = LevelBody {
            color: "pink".into(),
            ..body.levels[0].clone()
        };
        assert_eq!(refused(&body).field, "levels[0].color");
    }
}
