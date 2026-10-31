// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! From markings to a level: the operator's mapping applied to what a file
//! carries.

use aiplane_core::server::classification::{
    ClassificationConfig, MarkingKind, MarkingRule, Scheme,
};

use super::filename::tagged_levels;
use super::markings::{Marking, Markings, normalize_guid};

/// What the resolver needs: the scheme, the marking rows and the policy's
/// fallback levels.
#[derive(Debug, Clone, Copy)]
pub struct Mapping<'a> {
    pub scheme: &'a Scheme,
    pub rules: &'a [MarkingRule],
    pub policy: &'a ClassificationConfig,
}

/// Why a level was reached. Content-free: it names the kind of marking and,
/// for a Purview label, the label GUID, never the file's text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cause(String);

impl Cause {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Resolved {
    /// The highest level any marking meant. `None` when the file carries no
    /// marking at all.
    pub level_id: Option<String>,
    pub causes: Vec<Cause>,
}

/// Map markings, the file name's tag and the unreadable flag to a level. When
/// several disagree the highest rank wins.
pub fn resolve(markings: &Markings, filename: &str, mapping: &Mapping<'_>) -> Resolved {
    let mut hits: Vec<(String, String)> = Vec::new();
    for marking in &markings.found {
        match rule_level(marking, mapping.rules) {
            Some(level_id) => hits.push((level_id, cause_of(marking))),
            None => hits.push((
                mapping
                    .scheme
                    .resolve(&mapping.policy.unknown_label_level)
                    .id
                    .clone(),
                "marking:unmapped".into(),
            )),
        }
    }
    for level in tagged_levels(filename, mapping.scheme) {
        hits.push((level.id.clone(), "marking:filename".into()));
    }
    if markings.unreadable {
        hits.push((
            mapping
                .scheme
                .resolve(&mapping.policy.unreadable_level)
                .id
                .clone(),
            "marking:unreadable".into(),
        ));
    }

    let rank = |level_id: &str| mapping.scheme.rank_of(level_id).unwrap_or(0);
    let Some(top) = hits.iter().map(|(level_id, _)| rank(level_id)).max() else {
        return Resolved::default();
    };
    let level_id = hits
        .iter()
        .find(|(level_id, _)| rank(level_id) == top)
        .map(|(level_id, _)| level_id.clone());
    let mut causes: Vec<Cause> = Vec::new();
    for (_, cause) in hits {
        if !causes.iter().any(|existing| existing.0 == cause) {
            causes.push(Cause(cause));
        }
    }
    Resolved { level_id, causes }
}

fn cause_of(marking: &Marking) -> String {
    match marking.kind {
        MarkingKind::Mip => format!("marking:mip:{}", marking.value),
        MarkingKind::Tscp => "marking:tscp".into(),
        MarkingKind::Sensitivity => "marking:sensitivity".into(),
        MarkingKind::Stamp => "marking:stamp".into(),
    }
}

/// A Purview label maps by GUID first and by name second; other markings by
/// their value. Comparison ignores case, braces around a GUID and
/// surrounding blanks.
fn rule_level(marking: &Marking, rules: &[MarkingRule]) -> Option<String> {
    let find = |kind: MarkingKind, wanted: &str| {
        let wanted = wanted.trim().to_lowercase();
        rules
            .iter()
            .filter(|rule| rule.kind == kind)
            .find(|rule| {
                let value = rule.value.trim().to_lowercase();
                value == wanted || normalize_guid(&value) == wanted
            })
            .map(|rule| rule.level_id.clone())
    };
    match marking.kind {
        MarkingKind::Mip => find(MarkingKind::Mip, &normalize_guid(&marking.value)).or_else(|| {
            marking
                .name
                .as_deref()
                .and_then(|name| find(MarkingKind::Mip, name))
        }),
        kind => find(kind, &marking.value),
    }
}
