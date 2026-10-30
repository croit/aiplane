// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

/// Whether a conversation's level may ever go down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lowering {
    #[default]
    Never,
    WithReason,
}

impl Lowering {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Never => "never",
            Self::WithReason => "with_reason",
        }
    }

    pub fn parse(raw: &str) -> Self {
        match raw.trim() {
            "with_reason" => Self::WithReason,
            _ => Self::Never,
        }
    }
}

/// What a gated tool call does when its arguments match a data class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GateOnMatch {
    #[default]
    Ask,
    Block,
}

impl GateOnMatch {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::Block => "block",
        }
    }

    pub fn parse(raw: &str) -> Self {
        match raw.trim() {
            "block" => Self::Block,
            _ => Self::Ask,
        }
    }
}

/// `Monitor` logs what a detector would do and never acts on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClassificationMode {
    Monitor,
    #[default]
    Enforce,
}

impl ClassificationMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Monitor => "monitor",
            Self::Enforce => "enforce",
        }
    }

    pub fn parse(raw: &str) -> Self {
        match raw.trim() {
            "monitor" => Self::Monitor,
            _ => Self::Enforce,
        }
    }
}

/// Which parts of a generated file carry its level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OutputMarking {
    pub visible: bool,
    pub metadata: bool,
    pub filename: bool,
}

/// The operator's classification policy, from the settings rows. Level
/// settings hold level ids; an empty or unknown id reads as the scheme's
/// lowest level (see `Scheme::resolve`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ClassificationConfig {
    pub default_level: String,
    pub lowering: Lowering,
    pub gate_on_match: GateOnMatch,
    pub link_share_max_level: String,
    pub unknown_label_level: String,
    pub unreadable_level: String,
    pub system_one_model: String,
    pub mode: ClassificationMode,
    pub output: OutputMarking,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_values_fall_back_to_the_safe_choice() {
        assert_eq!(Lowering::parse("whenever"), Lowering::Never);
        assert_eq!(Lowering::parse("with_reason"), Lowering::WithReason);
        assert_eq!(GateOnMatch::parse(""), GateOnMatch::Ask);
        assert_eq!(GateOnMatch::parse("block"), GateOnMatch::Block);
        assert_eq!(
            ClassificationMode::parse("junk"),
            ClassificationMode::Enforce
        );
        assert_eq!(
            ClassificationMode::parse("monitor"),
            ClassificationMode::Monitor
        );
    }

    #[test]
    fn the_defaults_are_never_lower_ask_and_enforce() {
        let config = ClassificationConfig::default();
        assert_eq!(config.lowering, Lowering::Never);
        assert_eq!(config.gate_on_match, GateOnMatch::Ask);
        assert_eq!(config.mode, ClassificationMode::Enforce);
    }
}
