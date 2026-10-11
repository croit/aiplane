// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use serde_json::json;

use super::{AgentSpec, model::Permission};

pub fn replace_tool_access(
    draft: &str,
    selections: &[(String, Permission)],
) -> Result<String, serde_json::Error> {
    let mut spec: serde_json::Value = serde_json::from_str(draft)?;
    AgentSpec::from_value(&spec)?;
    spec["main"]["tools"] = json!(selections.iter().map(|(id, _)| id).collect::<Vec<_>>());
    let mut resources = spec["main"]["tool_resources"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    resources.retain(|id, _| selections.iter().any(|(selected, _)| selected == id));
    for (id, permission) in selections {
        let resource = resources.entry(id.clone()).or_insert_with(|| json!({}));
        resource["permission"] = json!(match permission {
            Permission::AlwaysAllow => "always_allow",
            Permission::AlwaysAsk => "always_ask",
        });
    }
    spec["main"]["tool_resources"] = json!(resources);
    AgentSpec::from_value(&spec)?;
    Ok(spec.to_string())
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
                ("read".into(), Permission::AlwaysAllow),
                ("write".into(), Permission::AlwaysAsk),
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
        ] {
            assert!(replace_tool_access(draft, &[]).is_err(), "{draft}");
        }
    }
}
