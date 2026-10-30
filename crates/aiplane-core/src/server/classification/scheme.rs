// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

/// One step of the operator's ordered scheme. A higher rank is more sensitive;
/// rank 0 is the lowest level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Level {
    pub id: String,
    pub rank: u32,
    pub name: String,
    pub description: String,
    /// A daisyUI semantic colour: `neutral`, `info`, `success`, `warning` or `error`.
    pub color: String,
    pub marking_text: String,
    pub mip_label_id: String,
    pub filename_tag: String,
}

/// What a detector does when it recognises a data class that the target is
/// not cleared for. Ordered by strength: `Raise` < `Confirm` < `Block`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Action {
    Raise,
    Confirm,
    Block,
}

impl Action {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "raise" => Some(Self::Raise),
            "confirm" => Some(Self::Confirm),
            "block" => Some(Self::Block),
            _ => None,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Raise => "raise",
            Self::Confirm => "confirm",
            Self::Block => "block",
        }
    }
}

/// The System One question that recognises a data class.
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub question: String,
    pub positive: String,
    pub negative: String,
    pub threshold: f64,
}

/// What a detector recognises, and the level the data belongs to. Only the
/// level is enforced; the class is the reason recorded in the audit.
#[derive(Debug, Clone, PartialEq)]
pub struct DataClass {
    pub id: String,
    pub name: String,
    pub level_id: String,
    /// Ids of the built-in patterns that mark content as this class.
    pub patterns: Vec<String>,
    pub rule: Option<Rule>,
    pub action: Action,
}

/// Where a digital marking sits: a Microsoft Purview label (GUID or name), a
/// TSCP business authorization category, an e-mail `Sensitivity` header, or a
/// visible stamp text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkingKind {
    Mip,
    Tscp,
    Sensitivity,
    Stamp,
}

impl MarkingKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Mip => "mip",
            Self::Tscp => "tscp",
            Self::Sensitivity => "sensitivity",
            Self::Stamp => "stamp",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "mip" => Some(Self::Mip),
            "tscp" => Some(Self::Tscp),
            "sensitivity" => Some(Self::Sensitivity),
            "stamp" => Some(Self::Stamp),
            _ => None,
        }
    }
}

/// One mapping from a marking value to the level it means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkingRule {
    pub kind: MarkingKind,
    pub value: String,
    pub level_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DestinationKind {
    Pool,
    Tool,
    Mcp,
    Connector,
    Collection,
}

impl DestinationKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pool => "pool",
            Self::Tool => "tool",
            Self::Mcp => "mcp",
            Self::Connector => "connector",
            Self::Collection => "collection",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "pool" => Some(Self::Pool),
            "tool" => Some(Self::Tool),
            "mcp" => Some(Self::Mcp),
            "connector" => Some(Self::Connector),
            "collection" => Some(Self::Collection),
            _ => None,
        }
    }
}

/// What happens to a destination whose level is below the conversation's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Over {
    Hide,
    Gate,
}

impl Over {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "hide" => Some(Self::Hide),
            "gate" => Some(Self::Gate),
            _ => None,
        }
    }
}

/// The maximum level of one destination (a pool, a tool, an MCP server, a
/// connector or a knowledge collection).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assignment {
    pub kind: DestinationKind,
    pub id: String,
    pub level_id: String,
    pub over: Over,
}

/// The operator's classification scheme as one immutable snapshot: levels in
/// rank order and the data classes. Hot paths read a snapshot instead of the
/// database.
#[derive(Debug, Clone, PartialEq)]
pub struct Scheme {
    levels: Vec<Level>,
    classes: Vec<DataClass>,
    assignments: Vec<Assignment>,
}

impl Default for Scheme {
    /// The scheme of a fresh installation: the single level `public`.
    fn default() -> Self {
        Self {
            levels: vec![Level {
                id: "public".into(),
                rank: 0,
                name: "Public".into(),
                description: String::new(),
                color: "success".into(),
                marking_text: String::new(),
                mip_label_id: String::new(),
                filename_tag: String::new(),
            }],
            classes: Vec::new(),
            assignments: Vec::new(),
        }
    }
}

impl Scheme {
    /// Levels are sorted by rank. An empty list falls back to the default
    /// single level, because a scheme always has a lowest level.
    pub fn new(mut levels: Vec<Level>, classes: Vec<DataClass>) -> Self {
        if levels.is_empty() {
            return Self::default();
        }
        levels.sort_by_key(|level| level.rank);
        Self {
            levels,
            classes,
            assignments: Vec::new(),
        }
    }

    pub fn with_assignments(mut self, assignments: Vec<Assignment>) -> Self {
        self.assignments = assignments;
        self
    }

    pub fn assignments(&self) -> &[Assignment] {
        &self.assignments
    }

    /// The assignment of one destination, if it has one.
    pub fn assignment(&self, kind: DestinationKind, id: &str) -> Option<&Assignment> {
        self.assignments
            .iter()
            .find(|assignment| assignment.kind == kind && assignment.id == id)
    }

    /// The level a destination is cleared for: its assignment, or the lowest
    /// level when it has none.
    pub fn destination_level(&self, kind: DestinationKind, id: &str) -> &Level {
        self.level_or_lowest(self.assignment(kind, id).map(|a| a.level_id.as_str()))
    }

    /// The feature is active exactly when there is more than one level.
    pub fn is_active(&self) -> bool {
        self.levels.len() > 1
    }

    pub fn levels(&self) -> &[Level] {
        &self.levels
    }

    pub fn classes(&self) -> &[DataClass] {
        &self.classes
    }

    pub fn lowest(&self) -> &Level {
        &self.levels[0]
    }

    pub fn highest(&self) -> &Level {
        &self.levels[self.levels.len() - 1]
    }

    pub fn level(&self, id: &str) -> Option<&Level> {
        self.levels.iter().find(|level| level.id == id)
    }

    pub fn rank_of(&self, id: &str) -> Option<u32> {
        self.level(id).map(|level| level.rank)
    }

    /// The level a stored id names, or the lowest level when the id is
    /// absent: a conversation that never reached a higher level is at the
    /// bottom of the scheme.
    pub fn level_or_lowest(&self, id: Option<&str>) -> &Level {
        id.and_then(|id| self.level(id))
            .unwrap_or_else(|| self.lowest())
    }

    /// The configured level for a setting that names one, or the lowest when
    /// it is empty or no longer exists.
    pub fn resolve(&self, configured: &str) -> &Level {
        self.level_or_lowest(Some(configured.trim()).filter(|id| !id.is_empty()))
    }

    /// The data classes more sensitive than `target_rank`: the ones a
    /// destination at that rank must not receive.
    pub fn classes_above(&self, target_rank: u32) -> impl Iterator<Item = (&DataClass, &Level)> {
        self.classes.iter().filter_map(move |class| {
            let level = self.level(&class.level_id)?;
            (level.rank > target_rank).then_some((class, level))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level(id: &str, rank: u32) -> Level {
        Level {
            id: id.into(),
            rank,
            name: id.into(),
            description: String::new(),
            color: "neutral".into(),
            marking_text: String::new(),
            mip_label_id: String::new(),
            filename_tag: String::new(),
        }
    }

    fn class(id: &str, level_id: &str) -> DataClass {
        DataClass {
            id: id.into(),
            name: id.into(),
            level_id: level_id.into(),
            patterns: Vec::new(),
            rule: None,
            action: Action::Raise,
        }
    }

    #[test]
    fn a_fresh_scheme_has_one_level_and_is_inactive() {
        let scheme = Scheme::default();
        assert!(!scheme.is_active());
        assert_eq!(scheme.lowest().id, "public");
        assert_eq!(scheme.highest().id, "public");
    }

    #[test]
    fn levels_are_ordered_by_rank_whatever_order_they_arrive_in() {
        let scheme = Scheme::new(
            vec![level("secret", 2), level("public", 0), level("internal", 1)],
            vec![],
        );
        assert!(scheme.is_active());
        assert_eq!(scheme.lowest().id, "public");
        assert_eq!(scheme.highest().id, "secret");
        assert_eq!(scheme.rank_of("internal"), Some(1));
        assert_eq!(scheme.rank_of("nope"), None);
    }

    #[test]
    fn an_absent_or_unknown_level_reads_as_the_lowest() {
        let scheme = Scheme::new(vec![level("public", 0), level("secret", 1)], vec![]);
        assert_eq!(scheme.level_or_lowest(None).id, "public");
        assert_eq!(scheme.level_or_lowest(Some("gone")).id, "public");
        assert_eq!(scheme.level_or_lowest(Some("secret")).id, "secret");
        assert_eq!(scheme.resolve("  ").id, "public");
        assert_eq!(scheme.resolve("secret").id, "secret");
    }

    #[test]
    fn only_classes_above_the_target_are_listed() {
        let scheme = Scheme::new(
            vec![level("public", 0), level("internal", 1), level("secret", 2)],
            vec![class("pii", "internal"), class("nda", "secret")],
        );
        let ids = |rank| {
            scheme
                .classes_above(rank)
                .map(|(c, _)| c.id.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(0), ["pii", "nda"]);
        assert_eq!(ids(1), ["nda"]);
        assert!(ids(2).is_empty());
    }

    #[test]
    fn an_unassigned_destination_is_at_the_lowest_level() {
        let scheme = Scheme::new(vec![level("public", 0), level("secret", 1)], vec![])
            .with_assignments(vec![Assignment {
                kind: DestinationKind::Tool,
                id: "mailer".into(),
                level_id: "secret".into(),
                over: Over::Gate,
            }]);
        assert_eq!(
            scheme.destination_level(DestinationKind::Tool, "mailer").id,
            "secret"
        );
        assert_eq!(
            scheme.destination_level(DestinationKind::Tool, "other").id,
            "public"
        );
        assert_eq!(
            scheme.destination_level(DestinationKind::Mcp, "mailer").id,
            "public"
        );
    }

    #[test]
    fn actions_order_by_strength() {
        assert!(Action::Block > Action::Confirm && Action::Confirm > Action::Raise);
    }
}
