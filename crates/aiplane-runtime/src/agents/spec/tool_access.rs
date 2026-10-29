// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use serde_json::json;

use std::collections::BTreeMap;

use super::{
    AgentSpec,
    model::{ActionRule, Permission},
};

pub struct ToolAccessSelection {
    pub id: String,
    pub permission: Permission,
    pub actions: BTreeMap<String, ActionRule>,
}

pub fn replace_tool_access(
    draft: &str,
    selections: &[ToolAccessSelection],
) -> Result<String, serde_json::Error> {
    let mut spec: serde_json::Value = serde_json::from_str(draft)?;
    AgentSpec::from_value(&spec)?;
    spec["main"]["tools"] = json!(
        selections
            .iter()
            .map(|selection| &selection.id)
            .collect::<Vec<_>>()
    );
    let mut resources = spec["main"]["tool_resources"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    resources.retain(|id, _| selections.iter().any(|selection| &selection.id == id));
    for selection in selections {
        let resource = resources
            .entry(selection.id.clone())
            .or_insert_with(|| json!({}));
        resource["permission"] = json!(match selection.permission {
            Permission::AlwaysAllow => "always_allow",
            Permission::AlwaysAsk => "always_ask",
        });
        resource["actions"] = serde_json::to_value(&selection.actions)?;
    }
    spec["main"]["tool_resources"] = json!(resources);
    AgentSpec::from_value(&spec)?;
    Ok(spec.to_string())
}

/// The part of `draft` the tool selection edits — its tools and their
/// rules — as one canonical text: a revision over it changes when the
/// selection does, not when another part of the draft is saved.
pub fn tool_access_text(draft: &str) -> Result<String, serde_json::Error> {
    let spec: serde_json::Value = serde_json::from_str(draft)?;
    AgentSpec::from_value(&spec)?;
    Ok(json!([spec["main"]["tools"], spec["main"]["tool_resources"]]).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    #[test]
    fn access_replacement_preserves_instructions_bindings_and_timeouts() {
        let draft = json!({
            "profile":{"display":"Harald"},
            "main":{
                "instructions":{"response":"Use brief answers"},
                "tools":["read", "removed"],
                "tool_resources":{
                    "read":{"bind":{"project":{"const":"example"}},"approval_timeout":"5m","permission":"always_ask"},
                    "removed":{"permission":"always_allow"}
                }
            }
        });
        let saved = replace_tool_access(
            &draft.to_string(),
            &[
                ToolAccessSelection {
                    id: "read".into(),
                    permission: Permission::AlwaysAllow,
                    actions: Default::default(),
                },
                ToolAccessSelection {
                    id: "write".into(),
                    permission: Permission::AlwaysAsk,
                    actions: Default::default(),
                },
            ],
        )
        .unwrap();
        let saved: Value = serde_json::from_str(&saved).unwrap();
        assert_eq!(saved["profile"], draft["profile"]);
        assert_eq!(saved["main"]["instructions"], draft["main"]["instructions"]);
        assert_eq!(saved["main"]["tools"], json!(["read", "write"]));
        assert_eq!(
            saved["main"]["tool_resources"]["read"]["bind"],
            draft["main"]["tool_resources"]["read"]["bind"]
        );
        assert_eq!(
            saved["main"]["tool_resources"]["read"]["approval_timeout"],
            "5m"
        );
        assert_eq!(
            saved["main"]["tool_resources"]["read"]["permission"],
            "always_allow"
        );
        assert_eq!(
            saved["main"]["tool_resources"]["write"]["permission"],
            "always_ask"
        );
        assert!(saved["main"]["tool_resources"].get("removed").is_none());
    }

    #[test]
    fn nested_actions_are_replaced_without_changing_other_resource_fields() {
        let draft = r#"{"main":{"tools":["browse"],"tool_resources":{"browse":{"permission":"always_ask","approval_timeout":"5m","bind":{"project":{"const":"example"}},"actions":{"old":"deny"}}}}}"#;
        let saved = replace_tool_access(
            draft,
            &[ToolAccessSelection {
                id: "browse".into(),
                permission: Permission::AlwaysAsk,
                actions: [
                    ("read".into(), ActionRule::Allow),
                    ("fill".into(), ActionRule::Deny),
                ]
                .into(),
            }],
        )
        .unwrap();
        let saved: Value = serde_json::from_str(&saved).unwrap();
        assert_eq!(
            saved["main"]["tool_resources"]["browse"]["actions"],
            json!({"read":"allow","fill":"deny"})
        );
        assert_eq!(
            saved["main"]["tool_resources"]["browse"]["approval_timeout"],
            "5m"
        );
        assert_eq!(
            saved["main"]["tool_resources"]["browse"]["bind"]["project"]["const"],
            "example"
        );
        let cleared = replace_tool_access(
            &saved.to_string(),
            &[ToolAccessSelection {
                id: "browse".into(),
                permission: Permission::AlwaysAsk,
                actions: Default::default(),
            }],
        )
        .unwrap();
        let cleared: Value = serde_json::from_str(&cleared).unwrap();
        assert_eq!(
            cleared["main"]["tool_resources"]["browse"]["actions"],
            json!({})
        );
    }

    #[test]
    fn empty_selection_denies_every_tool_and_removes_resources() {
        let saved = replace_tool_access(r#"{"main":{"tools":["read"],"tool_resources":{"read":{"permission":"always_allow"}}}}"#, &[]).unwrap();
        let saved: Value = serde_json::from_str(&saved).unwrap();
        assert_eq!(saved["main"]["tools"], json!([]));
        assert_eq!(saved["main"]["tool_resources"], json!({}));
    }

    #[test]
    fn invalid_existing_drafts_are_rejected() {
        for draft in [
            "invalid",
            r#"{"main":{"unknown":true}}"#,
            r#"{"main":{"tool_resources":{"read":{"permission":"invalid"}}}}"#,
            r#"{"main":{"tool_resources":{"read":{"actions":{"read":"invalid"}}}}}"#,
        ] {
            assert!(replace_tool_access(draft, &[]).is_err(), "{draft}");
        }
    }
}
