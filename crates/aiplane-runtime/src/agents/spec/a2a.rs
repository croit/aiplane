// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The `publish.a2a` part of the spec validator (`docs/agent-a2a.md` → "Serving an agent over A2A"): the opt-in to serve the agent over A2A, and the skills its agent
//! card advertises. Without `skills` the card derives them from the agent's
//! description and its described routes ([`crate::agents::a2a::skills`]).

use std::collections::BTreeSet;

use serde_json::Value;

use super::{Check, index, join, type_name};

const A2A_KEYS: &[&str] = &["enabled", "skills"];
const SKILL_KEYS: &[&str] = &["id", "name", "description", "tags", "examples"];
/// What a card lists; more is a catalogue, not a card.
pub(crate) const MAX_SKILLS: usize = 20;
const MAX_LIST: usize = 20;
const MAX_NAME_CHARS: usize = 120;
const MAX_TEXT_CHARS: usize = 2_000;

/// An A2A skill id: what a client may send back to name the skill, so it is
/// kept to a slug.
pub(crate) fn is_skill_id(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && s.len() <= 64
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

impl Check<'_> {
    pub(super) fn a2a(&mut self, v: &Value) {
        let path = "publish.a2a";
        let Some(map) = self.object(v, path, A2A_KEYS) else {
            return;
        };
        match map.get("enabled") {
            Some(Value::Bool(_)) => {}
            Some(other) => self.issue(
                &join(path, "enabled"),
                format!(
                    "must be true or false, not {} — true serves this agent over A2A",
                    type_name(other)
                ),
            ),
            None => self.issue(
                &join(path, "enabled"),
                "say whether the agent is served over A2A: `enabled: true` or `false`",
            ),
        }
        if let Some(skills) = map.get("skills") {
            self.a2a_skills(skills, &join(path, "skills"));
        }
    }

    fn a2a_skills(&mut self, v: &Value, path: &str) {
        let Value::Array(items) = v else {
            self.issue(
                path,
                format!("must be an array of skills, not {}", type_name(v)),
            );
            return;
        };
        if items.is_empty() || items.len() > MAX_SKILLS {
            self.issue(
                path,
                format!(
                    "list between 1 and {MAX_SKILLS} skills, or leave `skills` out to derive them \
                     from the agent's description and routes"
                ),
            );
        }
        let mut ids = BTreeSet::new();
        for (i, item) in items.iter().enumerate() {
            let at = index(path, i);
            let Some(skill) = self.object(item, &at, SKILL_KEYS) else {
                continue;
            };
            match skill.get("id").and_then(Value::as_str) {
                Some(id) if is_skill_id(id) => {
                    if !ids.insert(id) {
                        self.issue(&join(&at, "id"), format!("skill `{id}` is listed twice"));
                    }
                }
                Some(id) => self.issue(
                    &join(&at, "id"),
                    format!(
                        "`{id}` is not a skill id — use lowercase letters, digits, `-` and `_`, \
                         at most 64 characters"
                    ),
                ),
                None => self.issue(&join(&at, "id"), "a skill needs an `id`, e.g. `billing`"),
            }
            self.a2a_text(skill.get("name"), &join(&at, "name"), MAX_NAME_CHARS);
            self.a2a_text(
                skill.get("description"),
                &join(&at, "description"),
                MAX_TEXT_CHARS,
            );
            for key in ["tags", "examples"] {
                if let Some(list) = skill.get(key) {
                    let lp = join(&at, key);
                    let entries = self.string_list(list, &lp);
                    if entries.len() > MAX_LIST {
                        self.issue(&lp, format!("list at most {MAX_LIST} {key}"));
                    }
                    for (p, s) in entries {
                        if s.trim().is_empty() || s.chars().count() > MAX_TEXT_CHARS {
                            self.issue(
                                &p,
                                format!("must be between 1 and {MAX_TEXT_CHARS} characters"),
                            );
                        }
                    }
                }
            }
        }
    }

    fn a2a_text(&mut self, v: Option<&Value>, path: &str, max: usize) {
        let Some(v) = v else {
            self.issue(path, "a skill needs a `name` and a `description`");
            return;
        };
        if let Some(s) = self.string(v, path)
            && (s.trim().is_empty() || s.chars().count() > max)
        {
            self.issue(path, format!("must be between 1 and {max} characters"));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use aiplane_core::server::principal::GrantSet;
    use serde_json::json;

    use super::super::{SpecContext, SpecIssue, Stage, validate};

    fn check(a2a: Value) -> Vec<SpecIssue> {
        let grants = GrantSet::default();
        validate(
            &json!({ "publish": { "a2a": a2a } }),
            &SpecContext {
                action_catalogues: &Default::default(),
                agent_id: "self",
                grants: &grants,
                agents: &HashMap::new(),
                live_specs: &HashMap::new(),
                model_defaults: &Default::default(),
                speech_voices: &HashMap::new(),
                allow_private: false,
            },
            Stage::Draft,
        )
    }

    use super::*;

    fn paths(issues: &[SpecIssue]) -> Vec<&str> {
        issues.iter().map(|i| i.path.as_str()).collect()
    }

    #[test]
    fn the_opt_in_is_a_boolean_and_skills_are_optional() {
        assert!(check(json!({ "enabled": true })).is_empty());
        assert!(check(json!({ "enabled": false })).is_empty());
        assert_eq!(paths(&check(json!({}))), ["publish.a2a.enabled"]);
        assert_eq!(
            paths(&check(json!({ "enabled": "yes", "card": {} }))),
            ["publish.a2a.card", "publish.a2a.enabled"]
        );
    }

    #[test]
    fn listed_skills_need_an_id_a_name_and_a_description() {
        let ok = check(json!({ "enabled": true, "skills": [
            { "id": "billing", "name": "Billing", "description": "Invoices and payments",
              "tags": ["invoice"], "examples": ["Why was I charged twice?"] }
        ] }));
        assert!(ok.is_empty(), "{ok:?}");

        let bad = check(json!({ "enabled": true, "skills": [
            { "id": "Billing Desk", "name": "", "description": "x" },
            { "name": "No id", "description": "x", "tags": [""] },
            { "id": "dup", "name": "a", "description": "b" },
            { "id": "dup", "name": "a", "description": "b", "price": 3 }
        ] }));
        assert_eq!(
            paths(&bad),
            [
                "publish.a2a.skills[0].id",
                "publish.a2a.skills[0].name",
                "publish.a2a.skills[1].id",
                "publish.a2a.skills[1].tags[0]",
                "publish.a2a.skills[3].price",
                "publish.a2a.skills[3].id",
            ]
        );
        assert_eq!(
            paths(&check(json!({ "enabled": true, "skills": [] }))),
            ["publish.a2a.skills"]
        );
    }

    #[test]
    fn a_skill_id_is_a_slug() {
        for ok in ["billing", "tier-2", "faq_de", "9lives"] {
            assert!(is_skill_id(ok), "{ok}");
        }
        for bad in ["", "Billing", "-x", "a b", &"x".repeat(65)] {
            assert!(!is_skill_id(bad), "{bad}");
        }
    }
}
