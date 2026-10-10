// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! A built-in chat persona: a person's conversation run on a fixed system
//! prompt and a small set of its own tools, in place of the person's chat
//! tools (`docs/agent-builder.md` → "Agent architect"; the agent architect is the
//! one persona today).
//!
//! The turn is still the person's: their pools, budget, usage and history.
//! Only what the model is told and offered changes, which is why it is a
//! [`TurnPolicy`](crate::openai_driver) variant and not an agent run. The
//! persona's tools act with the person's rights and check them themselves;
//! the source is built for the turn by whoever knows the person, so nothing
//! here names what they do.

use std::sync::Arc;

use crate::server::tools::ToolSource;

pub struct ChatPersona {
    instructions: String,
    tools: Arc<dyn ToolSource>,
    user_context: bool,
}

impl ChatPersona {
    pub fn new(instructions: impl Into<String>, tools: Arc<dyn ToolSource>) -> Self {
        Self {
            instructions: instructions.into(),
            tools,
            user_context: false,
        }
    }

    pub fn with_user_context(mut self) -> Self {
        self.user_context = true;
        self
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
        let persona = ChatPersona::new("You are a personal Dot.", Arc::new(ToolRegistry::new()))
            .with_user_context();
        assert!(persona.includes_user_context());
    }
}
