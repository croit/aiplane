// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The classification tag in a file name: `{stem}-{tag}.{ext}`. The one shape
//! the output marking writes and the marking reader recognises.

use aiplane_core::server::classification::{Level, Scheme};

fn split_extension(filename: &str) -> (&str, &str) {
    match filename.rfind('.') {
        Some(dot) if dot > 0 => filename.split_at(dot),
        _ => (filename, ""),
    }
}

fn tag_of<'a>(stem: &str, level: &'a Level) -> Option<&'a str> {
    let tag = level.filename_tag.as_str();
    if tag.is_empty() {
        return None;
    }
    let suffix = format!("-{}", tag.to_lowercase());
    stem.to_lowercase().ends_with(&suffix).then_some(tag)
}

/// The levels whose tag ends the file name's stem, highest rank first.
pub(super) fn tagged_levels<'a>(filename: &str, scheme: &'a Scheme) -> Vec<&'a Level> {
    let (stem, _) = split_extension(filename);
    let mut found: Vec<&Level> = scheme
        .levels()
        .iter()
        .filter(|level| tag_of(stem, level).is_some())
        .collect();
    found.sort_by_key(|level| std::cmp::Reverse(level.rank));
    found
}

/// `filename` carrying `level`'s tag. A tag of any level already there is
/// replaced, never stacked.
pub(super) fn with_tag(filename: &str, level: &Level, scheme: &Scheme) -> String {
    let (stem, extension) = split_extension(filename);
    let stem_lower = stem.to_lowercase();
    let existing = scheme
        .levels()
        .iter()
        .filter_map(|other| tag_of(&stem_lower, other))
        .max_by_key(|tag| tag.chars().count());
    let bare = match existing {
        Some(tag) => {
            let cut = stem.chars().count() - tag.chars().count() - 1;
            stem.chars().take(cut).collect::<String>()
        }
        None => stem.to_string(),
    };
    format!("{bare}-{}{extension}", level.filename_tag)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level(id: &str, rank: u32, tag: &str) -> Level {
        Level {
            id: id.into(),
            rank,
            name: id.into(),
            description: String::new(),
            color: "neutral".into(),
            marking_text: String::new(),
            mip_label_id: String::new(),
            filename_tag: tag.into(),
        }
    }

    fn scheme() -> Scheme {
        Scheme::new(
            vec![
                level("public", 0, ""),
                level("internal", 1, "internal"),
                level("confidential", 2, "confidential"),
                level("strict", 3, "strictly-confidential"),
            ],
            Vec::new(),
        )
    }

    #[test]
    fn a_tag_is_added_before_the_extension() {
        let scheme = scheme();
        let confidential = scheme.level("confidential").unwrap();
        assert_eq!(
            with_tag("offer.json", confidential, &scheme),
            "offer-confidential.json"
        );
        assert_eq!(
            with_tag("notes", confidential, &scheme),
            "notes-confidential"
        );
        assert_eq!(
            with_tag("a.b.tar.gz", confidential, &scheme),
            "a.b.tar-confidential.gz"
        );
    }

    #[test]
    fn a_tag_of_any_level_is_replaced_never_stacked() {
        let scheme = scheme();
        let strict = scheme.level("strict").unwrap();
        assert_eq!(
            with_tag("offer-internal.docx", strict, &scheme),
            "offer-strictly-confidential.docx"
        );
        assert_eq!(
            with_tag(
                "offer-strictly-confidential.docx",
                scheme.level("internal").unwrap(),
                &scheme
            ),
            "offer-internal.docx"
        );
        assert_eq!(
            with_tag("Offer-CONFIDENTIAL.docx", strict, &scheme),
            "Offer-strictly-confidential.docx"
        );
    }

    #[test]
    fn the_highest_tagged_level_wins_when_tags_overlap() {
        let scheme = scheme();
        let found = tagged_levels("offer-strictly-confidential.docx", &scheme);
        assert_eq!(
            found.iter().map(|l| l.id.as_str()).collect::<Vec<_>>(),
            ["strict", "confidential"]
        );
        assert!(tagged_levels("offer.docx", &scheme).is_empty());
        assert!(tagged_levels("confidential.docx", &scheme).is_empty());
    }
}
