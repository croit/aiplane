// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Low,
    #[default]
    Medium,
    High,
}

impl Priority {
    pub fn effective(
        token: Option<Self>,
        groups: impl IntoIterator<Item = Self>,
        default: Self,
    ) -> Self {
        token
            .or_else(|| groups.into_iter().max())
            .unwrap_or(default)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PriorityMapping {
    pub field: String,
    pub low: i64,
    pub medium: i64,
    pub high: i64,
}

#[derive(Debug, thiserror::Error)]
#[error(
    "invalid backend priority mapping: {0}; choose a priority parameter and three ordered integer values"
)]
pub struct InvalidPriorityMapping(&'static str);

impl PriorityMapping {
    pub fn lower_first() -> Self {
        Self {
            field: "priority".into(),
            low: 1,
            medium: 0,
            high: -1,
        }
    }

    pub fn validate(&self) -> Result<(), InvalidPriorityMapping> {
        if self.field.is_empty()
            || !self
                .field
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
            || matches!(
                self.field.as_str(),
                "input"
                    | "query"
                    | "documents"
                    | "prompt"
                    | "size"
                    | "audio"
                    | "file"
                    | "voice"
                    | "questions"
                    | "state"
                    | "instructions"
                    | "model"
                    | "messages"
                    | "tools"
                    | "stream"
                    | "max_tokens"
                    | "max_completion_tokens"
                    | "temperature"
                    | "top_p"
                    | "n"
                    | "stop"
                    | "tool_choice"
                    | "response_format"
                    | "stream_options"
                    | "metadata"
                    | "user"
                    | "seed"
                    | "frequency_penalty"
                    | "presence_penalty"
                    | "logit_bias"
                    | "logprobs"
                    | "top_logprobs"
                    | "reasoning_effort"
                    | "chat_template_kwargs"
                    | "parallel_tool_calls"
            )
        {
            return Err(InvalidPriorityMapping(
                "the field replaces request content or is not a top-level parameter name",
            ));
        }
        if !((self.low < self.medium && self.medium < self.high)
            || (self.high < self.medium && self.medium < self.low))
        {
            return Err(InvalidPriorityMapping(
                "the values are not strictly ordered",
            ));
        }
        Ok(())
    }

    pub fn apply(&self, body: &mut Value, priority: Priority) {
        if let Some(object) = body.as_object_mut() {
            object.remove("priority");
            object.insert(
                self.field.clone(),
                Value::from(match priority {
                    Priority::Low => self.low,
                    Priority::Medium => self.medium,
                    Priority::High => self.high,
                }),
            );
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(
    tag = "mode",
    content = "mapping",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum BackendPriority {
    #[default]
    Auto,
    Gateway,
    Custom(PriorityMapping),
}

impl BackendPriority {
    pub fn mapping(&self, detected: Option<&PriorityMapping>) -> Option<PriorityMapping> {
        match self {
            Self::Auto => detected.cloned(),
            Self::Gateway => None,
            Self::Custom(mapping) => Some(mapping.clone()),
        }
    }

    pub fn apply(
        &self,
        body: &mut Value,
        priority: Priority,
        detected: Option<&PriorityMapping>,
    ) -> bool {
        if let Some(mapping) = self.mapping(detected) {
            let is_object = body.is_object();
            mapping.apply(body, priority);
            is_object
        } else {
            body.as_object_mut()
                .is_some_and(|object| object.remove("priority").is_some())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn token_override_can_lower_a_high_priority_user() {
        assert_eq!(
            Priority::effective(Some(Priority::Low), [Priority::High], Priority::Medium),
            Priority::Low
        );
        assert_eq!(
            Priority::effective(None, [Priority::Low, Priority::High], Priority::Medium),
            Priority::High
        );
        assert_eq!(
            Priority::effective(None, [], Priority::Medium),
            Priority::Medium
        );
        assert_eq!(
            Priority::effective(None, [Priority::Low], Priority::Medium),
            Priority::Low
        );
    }

    #[test]
    fn mappings_replace_client_values_and_remove_unsupported_priority() {
        let mapping = PriorityMapping::lower_first();
        let mut body = json!({"model": "m", "priority": -999});
        mapping.apply(&mut body, Priority::High);
        assert_eq!(body["priority"], -1);
        mapping.apply(&mut body, Priority::Low);
        assert_eq!(body["priority"], 1);
        BackendPriority::Gateway.apply(&mut body, Priority::High, None);
        assert!(body.get("priority").is_none());
        assert_eq!(body["model"], "m");
    }

    #[test]
    fn automatic_mapping_requires_reported_support_and_manual_wins() {
        let lower = PriorityMapping::lower_first();
        let higher = PriorityMapping {
            low: -1,
            medium: 0,
            high: 1,
            ..lower.clone()
        };
        assert_eq!(BackendPriority::Auto.mapping(None), None);
        assert_eq!(BackendPriority::Auto.mapping(Some(&higher)), Some(higher));
        assert_eq!(
            BackendPriority::Custom(lower.clone()).mapping(None),
            Some(lower)
        );
    }

    #[test]
    fn custom_mapping_cannot_overwrite_request_semantics() {
        for field in [
            "model",
            "messages",
            "tools",
            "stream",
            "max_tokens",
            "input",
            "questions",
            "state",
            "",
            "a.b",
        ] {
            let mapping = PriorityMapping {
                field: field.into(),
                ..PriorityMapping::lower_first()
            };
            assert!(mapping.validate().is_err(), "{field}");
        }
        assert!(PriorityMapping::lower_first().validate().is_ok());
        let equal = PriorityMapping {
            low: 0,
            high: 0,
            ..PriorityMapping::lower_first()
        };
        assert!(equal.validate().is_err());
    }
}
