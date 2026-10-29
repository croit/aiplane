// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! A person's conversation with a persona's system prompt and tool surface.
//! Built-in personas provide their own tools; personal Dots select from the
//! owner's granted tools and revalidate access before each invocation.
//!
//! The turn is still the person's: their pools, budget, usage and history.
//! Only what the model is told and offered changes, which is why it is a
//! [`TurnPolicy`](crate::openai_driver) variant and not an agent run. The
//! persona's tools act with the person's rights and check them themselves;
//! the source is built for the turn by whoever knows the person, so nothing
//! here names what they do.

use std::sync::Arc;

mod live;
pub mod personal_dot;

use crate::agents::approval::Permissions;
use crate::agents::bind::{BoundTool, ToolBinds, without_bound};
use crate::agents::spec::AgentSpec;
use crate::server::tools::{GrantedToolSource, Tool, ToolSource};
use shared::api::ToolDef;

pub struct ChatPersona {
    instructions: String,
    tools: Arc<dyn ToolSource>,
    user_context: bool,
    access: Option<(Vec<String>, Permissions, ToolBinds)>,
}

impl ChatPersona {
    pub fn new(instructions: impl Into<String>, tools: Arc<dyn ToolSource>) -> Self {
        Self {
            instructions: instructions.into(),
            tools,
            user_context: false,
            access: None,
        }
    }

    pub fn with_user_context(mut self) -> Self {
        self.user_context = true;
        self
    }

    pub fn with_tool_access(mut self, spec: &AgentSpec) -> Self {
        self.access = Some((
            spec.main.tools.clone(),
            Permissions::from_spec(spec),
            ToolBinds::from_spec(spec),
        ));
        self
    }

    pub(crate) fn source<'a>(
        &'a self,
        granted: &'a dyn ToolSource,
        state: Option<&Arc<crate::server::AppState>>,
    ) -> PersonaToolSource<'a> {
        match &self.access {
            Some((selected, permissions, binds)) => PersonaToolSource {
                binds: Some(binds),
                source: GrantedToolSource::new(granted, selected.iter().cloned()),
                permissions: Some(permissions),
                state: state.cloned(),
            },
            None => PersonaToolSource {
                binds: None,
                source: GrantedToolSource::new(self.tools(), self.offer()),
                permissions: None,
                state: None,
            },
        }
    }

    pub fn offered(&self, granted: &[String]) -> Vec<String> {
        match &self.access {
            Some((selected, _, _)) => selected
                .iter()
                .filter(|id| granted.contains(id))
                .cloned()
                .collect(),
            None => self.offer(),
        }
    }

    /// What the system message says after the turn-discipline rule.
    pub fn instructions(&self) -> &str {
        &self.instructions
    }

    /// The only source the turn's calls resolve against.
    pub fn tools(&self) -> &dyn ToolSource {
        self.tools.as_ref()
    }

    pub fn includes_user_context(&self) -> bool {
        self.user_context
    }

    /// Every tool of the persona, offered on every round, in a stable order.
    pub fn offer(&self) -> Vec<String> {
        let mut ids = self.tools.ids();
        ids.sort();
        ids
    }
}

pub(crate) struct PersonaToolSource<'a> {
    source: GrantedToolSource<'a>,
    permissions: Option<&'a Permissions>,
    binds: Option<&'a ToolBinds>,
    state: Option<Arc<crate::server::AppState>>,
}

impl ToolSource for PersonaToolSource<'_> {
    fn personal_call_audit_is_deferred(&self) -> bool {
        self.state.is_some() && self.permissions.is_some()
    }

    fn get(&self, id: &str) -> Option<Arc<dyn Tool>> {
        let tool = self.source.get(id)?;
        let gated = match self.permissions {
            Some(permissions) => match &self.state {
                Some(state) => {
                    return Some(Arc::new(live::LivePersonalDotTool {
                        state: state.clone(),
                        initial_bindings: self
                            .binds
                            .map(|binds| binds.for_tool(id, &tool.schema()))
                            .transpose()
                            .ok()?
                            .unwrap_or_default(),
                        initial: tool,
                    }));
                }
                None => permissions.gate_dot(tool),
            },
            None => tool,
        };
        match self.binds {
            Some(binds) => {
                let bound = binds.for_tool(id, &gated.schema()).ok()?;
                if bound.is_empty() {
                    Some(gated)
                } else {
                    Some(Arc::new(BoundTool::new(gated, bound, None, Arc::default())))
                }
            }
            None => Some(gated),
        }
    }

    fn defs_for(&self, allowed: &[String]) -> Vec<ToolDef> {
        self.source
            .defs_for(allowed)
            .into_iter()
            .filter_map(|definition| match self.binds {
                Some(binds) => binds
                    .for_tool(&definition.function.name, &definition)
                    .ok()
                    .map(|bound| without_bound(definition, &bound)),
                None => Some(definition),
            })
            .collect()
    }

    fn ids(&self) -> Vec<String> {
        let mut ids = self.source.ids();
        ids.sort();
        ids
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::tools::ToolRegistry;
    use crate::server::tools::echo::Echo;
    use crate::server::tools::time::CurrentTimestamp;

    #[test]
    fn a_persona_offers_exactly_its_own_tools_in_a_stable_order() {
        let persona = ChatPersona::new(
            "You plan agents.",
            Arc::new(ToolRegistry::new().with(CurrentTimestamp).with(Echo)),
        );
        assert_eq!(persona.offer(), ["company_echo", "get_current_timestamp"]);
        assert_eq!(persona.instructions(), "You plan agents.");
        assert!(!persona.includes_user_context());
        assert!(persona.tools().contains("company_echo"));
        assert!(!persona.tools().contains("fetch_url"));
    }

    #[test]
    fn a_persona_can_include_the_signed_in_users_context() {
        let persona = ChatPersona::new("You are a personal agent.", Arc::new(ToolRegistry::new()))
            .with_user_context();
        assert!(persona.includes_user_context());
    }

    #[tokio::test]
    async fn a_personal_dot_hides_and_enforces_saved_constant_tool_arguments() {
        let db = aiplane_core::server::db::open(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        let spec=AgentSpec::from_value(&serde_json::json!({"main":{"tools":["company_echo"],"tool_resources":{"company_echo":{"permission":"always_allow","bind":{"message":{"const":"Reviewed collection"}}}}}})).unwrap();
        let persona =
            ChatPersona::new("Agent", Arc::new(ToolRegistry::new())).with_tool_access(&spec);
        let registry = ToolRegistry::new().with(Echo);
        let source = persona.source(&registry, None);
        let definitions = source.defs_for(&["company_echo".into()]);
        assert!(
            definitions[0].function.parameters["properties"]
                .get("message")
                .is_none()
        );
        let result = source
            .get("company_echo")
            .unwrap()
            .run(
                crate::server::tools::ToolContext::for_test(db),
                serde_json::json!({"message":"Different collection"}),
            )
            .await
            .unwrap();
        assert_eq!(result, serde_json::json!({"message":"Reviewed collection"}));
    }

    #[test]
    fn personal_dot_selection_intersects_the_current_granted_source() {
        use crate::agents::spec::AgentSpec;
        use crate::server::tools::GrantedToolSource;
        let spec = AgentSpec::from_value(&serde_json::json!({"main": {
            "tools": ["company_echo", "get_current_timestamp"]
        }}))
        .unwrap();
        let persona =
            ChatPersona::new("Agent", Arc::new(ToolRegistry::new())).with_tool_access(&spec);
        let registry = ToolRegistry::new().with(Echo).with(CurrentTimestamp);
        let granted = GrantedToolSource::new(&registry, ["company_echo".to_string()]);
        let source = persona.source(&granted, None);
        assert_eq!(source.ids(), ["company_echo"]);
        assert!(source.get("company_echo").is_some());
        assert!(source.get("get_current_timestamp").is_none());
        assert_eq!(source.defs_for(&spec.main.tools).len(), 1);
        let revoked = GrantedToolSource::new(&registry, []);
        assert!(persona.source(&revoked, None).ids().is_empty());
    }

    #[test]
    fn personal_dot_with_empty_selection_has_no_tools() {
        let persona = ChatPersona::new("Agent", Arc::new(ToolRegistry::new().with(Echo)))
            .with_tool_access(crate::agents::spec::AgentSpec::empty());
        let registry = ToolRegistry::new().with(Echo);
        let source = persona.source(&registry, None);
        assert!(source.ids().is_empty());
        assert!(source.get("company_echo").is_none());
        assert!(source.defs_for(&["company_echo".into()]).is_empty());
    }

    #[tokio::test]
    async fn personal_dot_unknown_tool_asks_and_explicit_allow_runs() {
        use crate::agents::spec::AgentSpec;
        use crate::server::tools::ToolContext;
        use crate::suspend::{Suspend, extract_suspend};
        let db = aiplane_core::server::db::open(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        let ctx = ToolContext {
            suspend: Suspend::Available,
            ..ToolContext::for_test(db)
        };
        let registry = ToolRegistry::new().with(Echo);
        for (permission, asks) in [
            (serde_json::Value::Null, true),
            (serde_json::json!("always_allow"), false),
            (serde_json::json!("always_ask"), true),
        ] {
            let spec = AgentSpec::from_value(&serde_json::json!({"main": {
                "tools": ["company_echo"],
                "tool_resources": {"company_echo": {"permission": permission}}
            }}))
            .unwrap();
            let persona =
                ChatPersona::new("Agent", Arc::new(ToolRegistry::new())).with_tool_access(&spec);
            let body = persona
                .source(&registry, None)
                .get("company_echo")
                .unwrap()
                .run(ctx.clone(), serde_json::json!({"message": "hello"}))
                .await
                .unwrap();
            assert_eq!(extract_suspend(&body).is_some(), asks);
        }
    }
}
