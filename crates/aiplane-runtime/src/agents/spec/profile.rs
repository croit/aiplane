// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! What an agent is told to do, as its owner edits it on a personal agent's
//! **Task** tab (`docs/dots.md` → "Personal agent profile"): the task, tone
//! and topics of its draft spec.

use serde_json::{Value, json};

use super::model::AgentSpec;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Profile {
    pub task: String,
    pub tone: String,
    pub topics: Vec<String>,
}

/// The profile of `draft`; a field the draft leaves unset reads empty.
pub fn read_profile(draft: &str) -> Result<Profile, serde_json::Error> {
    let spec = AgentSpec::from_value(&serde_json::from_str(draft)?)?;
    let instructions = &spec.main.instructions;
    Ok(Profile {
        task: instructions.orchestration.clone().unwrap_or_default(),
        tone: instructions.response.clone().unwrap_or_default(),
        topics: spec
            .scope
            .as_ref()
            .map(|scope| scope.topics.clone())
            .unwrap_or_default(),
    })
}

/// `draft` with `profile` in place: a blank field is removed, every other
/// field of the draft is kept.
pub fn replace_profile(draft: &str, profile: &Profile) -> Result<String, serde_json::Error> {
    let mut spec: Value = serde_json::from_str(draft)?;
    AgentSpec::from_value(&spec)?;
    if !spec["main"].is_object() {
        spec["main"] = json!({});
    }
    let mut instructions = spec["main"]["instructions"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    for (key, value) in [
        ("orchestration", &profile.task),
        ("response", &profile.tone),
    ] {
        if value.is_empty() {
            instructions.remove(key);
        } else {
            instructions.insert(key.into(), json!(value));
        }
    }
    let main = spec["main"]
        .as_object_mut()
        .expect("set to an object above");
    if instructions.is_empty() {
        main.remove("instructions");
    } else {
        main.insert("instructions".into(), Value::Object(instructions));
    }
    let mut scope = spec["scope"].as_object().cloned().unwrap_or_default();
    if profile.topics.is_empty() {
        scope.remove("topics");
    } else {
        scope.insert("topics".into(), json!(profile.topics));
    }
    let root = spec.as_object_mut().expect("a typed spec is an object");
    if scope.is_empty() {
        root.remove("scope");
    } else {
        root.insert("scope".into(), Value::Object(scope));
    }
    AgentSpec::from_value(&spec)?;
    Ok(spec.to_string())
}

/// The draft a team agent copied from a personal agent starts with: only
/// what it is told to do. *Chosen (privacy):* tools stay behind because a
/// team agent acts on its own grants; memory and accounts never leave their
/// owner.
pub fn team_copy(personal_draft: &str) -> Result<String, serde_json::Error> {
    replace_profile("{}", &read_profile(personal_draft)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(task: &str, tone: &str, topics: &[&str]) -> Profile {
        Profile {
            task: task.into(),
            tone: tone.into(),
            topics: topics.iter().map(|t| t.to_string()).collect(),
        }
    }

    #[test]
    fn a_profile_round_trips_and_keeps_every_other_field() {
        let draft = r#"{"main":{"tools":["recall"],"tool_resources":{"recall":{"permission":"always_allow"}}},"scope":{"topics":["old"],"strict":true}}"#;
        let saved = replace_profile(
            draft,
            &profile("Watch Ceph.", "Short.", &["Ceph", "Proxmox"]),
        )
        .unwrap();
        assert_eq!(
            read_profile(&saved).unwrap(),
            profile("Watch Ceph.", "Short.", &["Ceph", "Proxmox"])
        );
        let spec: Value = serde_json::from_str(&saved).unwrap();
        assert_eq!(spec["main"]["tools"], json!(["recall"]));
        assert_eq!(
            spec["main"]["tool_resources"]["recall"]["permission"],
            "always_allow"
        );
        assert_eq!(spec["scope"]["strict"], true);
    }

    #[test]
    fn a_blank_profile_removes_its_fields_and_nothing_else() {
        let draft = r#"{"main":{"tools":[],"instructions":{"orchestration":"x","response":"y"}},"scope":{"topics":["a"]}}"#;
        let saved = replace_profile(draft, &Profile::default()).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&saved).unwrap(),
            json!({"main":{"tools":[]}})
        );
        assert_eq!(read_profile(&saved).unwrap(), Profile::default());
        assert_eq!(read_profile("{}").unwrap(), Profile::default());
    }

    #[test]
    fn a_team_copy_holds_only_what_the_agent_is_told() {
        let draft = r#"{"main":{"tools":["remember"],"tool_resources":{"remember":{"permission":"always_allow"}},"instructions":{"orchestration":"Watch Ceph."}},"scope":{"topics":["Ceph"],"strict":true}}"#;
        assert_eq!(
            serde_json::from_str::<Value>(&team_copy(draft).unwrap()).unwrap(),
            json!({"main":{"instructions":{"orchestration":"Watch Ceph."}},"scope":{"topics":["Ceph"]}})
        );
    }

    #[test]
    fn a_draft_that_is_no_spec_is_refused() {
        assert!(read_profile("[]").is_err());
        assert!(
            replace_profile(
                r#"{"main":{"instructions":{"unknown":1}}}"#,
                &Profile::default()
            )
            .is_err()
        );
    }
}
