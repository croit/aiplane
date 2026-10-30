// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The starting points the editor offers: the level scheme of a common
//! classification standard, with data classes and the markings its documents
//! usually carry. A template is data the operator then edits; none of it is a
//! certification. Names and descriptions come from the Fluent catalogs in the
//! administrator's language and become the scheme's own data once applied.
//! Stamps, marking values and filename tags are what real documents carry and
//! stay as they are; so do the rule questions, which are prompts measured
//! against an English-language System One model.

use session_core::i18n::{Lang, t};

use super::types::{ClassBody, LevelBody, MarkingBody, OutputBody, TemplatePolicy, TemplateView};

/// A level: id, colour, visible marking, filename tag. Name and description
/// are the catalog keys `cls-tpl-<template>-level-<id>-name` / `-description`.
struct L(&'static str, &'static str, &'static str, &'static str);

fn levels(template: &str, lang: Lang, rows: &[L]) -> Vec<LevelBody> {
    rows.iter()
        .enumerate()
        .map(|(rank, L(id, color, marking_text, tag))| {
            let key = |field: &str| t(lang, &format!("cls-tpl-{template}-level-{id}-{field}"));
            LevelBody {
                id: (*id).into(),
                rank: rank as u32,
                name: key("name"),
                description: key("description"),
                color: (*color).into(),
                marking_text: (*marking_text).into(),
                mip_label_id: String::new(),
                filename_tag: (*tag).into(),
            }
        })
        .collect()
}

fn markings(rows: &[(&str, &str, &str)]) -> Vec<MarkingBody> {
    rows.iter()
        .map(|(kind, value, level_id)| MarkingBody {
            kind: (*kind).into(),
            value: (*value).into(),
            level_id: (*level_id).into(),
        })
        .collect()
}

/// The levels the six data classes sit at: personal data, special
/// categories, trade secrets, payment data, tax identification number,
/// credentials and keys.
struct ClassLevels {
    personal: &'static str,
    special: &'static str,
    secrets: &'static str,
    payment: &'static str,
    tax: &'static str,
    credentials: &'static str,
}

fn classes(lang: Lang, at: &ClassLevels) -> Vec<ClassBody> {
    let class = |id: &str,
                 level_id: &str,
                 patterns: &[&str],
                 question: &str,
                 positive: &str,
                 negative: &str,
                 threshold: f64,
                 action: &str| ClassBody {
        id: id.into(),
        name: t(lang, &format!("cls-class-{id}-name")),
        description: t(lang, &format!("cls-class-{id}-description")),
        level_id: level_id.into(),
        patterns: patterns.iter().map(|p| (*p).into()).collect(),
        rule_question: question.into(),
        rule_positive: positive.into(),
        rule_negative: negative.into(),
        rule_threshold: threshold,
        action: action.into(),
    };
    // The rule questions are short and plain on purpose: measured against the
    // TypeSafe System One model, the long legal wording (GDPR article texts)
    // scored a named person's sick note at 0.36 and a payment reminder as a
    // trade secret at 0.65; these separate both at 0.6.
    vec![
        class(
            "personal_data",
            at.personal,
            &["email"],
            "Is a specific, identifiable person described in this text?",
            "A customer list with names and e-mail addresses.",
            "A product description.",
            0.6,
            "raise",
        ),
        class(
            "special_categories",
            at.special,
            &[],
            "Does the text mention the health, illness or diagnosis of a specific person, or \
             their religion, ethnicity, sexuality or political views?",
            "A sick note naming the employee and the diagnosis.",
            "A general article about a disease.",
            0.6,
            "raise",
        ),
        class(
            "trade_secrets",
            at.secrets,
            &[],
            "Is this text sharing a company secret or NDA-protected information?",
            "The formula of an unreleased product shared under an NDA.",
            "A published datasheet.",
            0.6,
            "confirm",
        ),
        class(
            "payment_data",
            at.payment,
            &["iban", "payment_card"],
            "Does this text contain an IBAN, a credit card number or payment login data?",
            "An invoice with the customer's IBAN.",
            "A price list.",
            0.6,
            "raise",
        ),
        class("tax_id", at.tax, &["de_tax_id"], "", "", "", 0.5, "raise"),
        class(
            "credentials",
            at.credentials,
            &["credentials"],
            "",
            "",
            "",
            0.5,
            "confirm",
        ),
    ]
}

fn policy(
    default_level: &str,
    link_share_max_level: &str,
    unknown_label_level: &str,
    unreadable_level: &str,
) -> TemplatePolicy {
    TemplatePolicy {
        default_level: Some(default_level.into()),
        lowering: Some("never".into()),
        gate_on_match: Some("ask".into()),
        link_share_max_level: Some(link_share_max_level.into()),
        unknown_label_level: Some(unknown_label_level.into()),
        unreadable_level: Some(unreadable_level.into()),
        mode: None,
        output: Some(OutputBody {
            visible: true,
            metadata: true,
            filename: false,
        }),
    }
}

fn iso(lang: Lang) -> TemplateView {
    TemplateView {
        id: "iso",
        name: t(lang, "cls-tpl-iso-name"),
        description: t(lang, "cls-tpl-iso-description"),
        levels: levels(
            "iso",
            lang,
            &[
                L("public", "success", "", ""),
                L("internal", "info", "INTERNAL", "internal"),
                L("confidential", "warning", "CONFIDENTIAL", "confidential"),
                L(
                    "strictly_confidential",
                    "error",
                    "STRICTLY CONFIDENTIAL",
                    "strictly_confidential",
                ),
            ],
        ),
        classes: classes(
            lang,
            &ClassLevels {
                personal: "internal",
                special: "strictly_confidential",
                secrets: "confidential",
                payment: "confidential",
                tax: "confidential",
                credentials: "strictly_confidential",
            },
        ),
        markings: markings(&[
            ("mip", "Public", "public"),
            ("mip", "General", "internal"),
            ("mip", "Internal", "internal"),
            ("mip", "Confidential", "confidential"),
            ("mip", "Highly Confidential", "strictly_confidential"),
            ("sensitivity", "company-confidential", "confidential"),
            ("stamp", "INTERNAL", "internal"),
            ("stamp", "CONFIDENTIAL", "confidential"),
            ("stamp", "STRICTLY CONFIDENTIAL", "strictly_confidential"),
        ]),
        policy: policy(
            "public",
            "internal",
            "confidential",
            "strictly_confidential",
        ),
    }
}

fn tisax(lang: Lang) -> TemplateView {
    TemplateView {
        id: "tisax",
        name: t(lang, "cls-tpl-tisax-name"),
        description: t(lang, "cls-tpl-tisax-description"),
        levels: levels(
            "tisax",
            lang,
            &[
                L("public", "success", "", ""),
                L("internal", "info", "INTERN", "intern"),
                L("confidential", "warning", "VERTRAULICH", "vertraulich"),
                L(
                    "strictly_confidential",
                    "error",
                    "STRENG VERTRAULICH",
                    "streng_vertraulich",
                ),
            ],
        ),
        classes: classes(
            lang,
            &ClassLevels {
                personal: "internal",
                special: "strictly_confidential",
                secrets: "confidential",
                payment: "confidential",
                tax: "confidential",
                credentials: "strictly_confidential",
            },
        ),
        markings: markings(&[
            ("mip", "Öffentlich", "public"),
            ("mip", "Intern", "internal"),
            ("mip", "Vertraulich", "confidential"),
            ("mip", "Streng vertraulich", "strictly_confidential"),
            ("sensitivity", "company-confidential", "confidential"),
            ("stamp", "INTERN", "internal"),
            ("stamp", "VERTRAULICH", "confidential"),
            ("stamp", "STRENG VERTRAULICH", "strictly_confidential"),
            ("stamp", "CONFIDENTIAL", "confidential"),
            ("stamp", "STRICTLY CONFIDENTIAL", "strictly_confidential"),
        ]),
        policy: policy(
            "public",
            "internal",
            "confidential",
            "strictly_confidential",
        ),
    }
}

fn bsi(lang: Lang) -> TemplateView {
    TemplateView {
        id: "bsi",
        name: t(lang, "cls-tpl-bsi-name"),
        description: t(lang, "cls-tpl-bsi-description"),
        levels: levels(
            "bsi",
            lang,
            &[
                L("normal", "success", "SCHUTZBEDARF NORMAL", ""),
                L("high", "warning", "SCHUTZBEDARF HOCH", "hoch"),
                L("very_high", "error", "SCHUTZBEDARF SEHR HOCH", "sehr_hoch"),
            ],
        ),
        classes: classes(
            lang,
            &ClassLevels {
                personal: "high",
                special: "very_high",
                secrets: "high",
                payment: "high",
                tax: "high",
                credentials: "very_high",
            },
        ),
        markings: markings(&[
            ("mip", "Normal", "normal"),
            ("mip", "Hoch", "high"),
            ("mip", "Sehr hoch", "very_high"),
            ("stamp", "SCHUTZBEDARF NORMAL", "normal"),
            ("stamp", "SCHUTZBEDARF HOCH", "high"),
            ("stamp", "SCHUTZBEDARF SEHR HOCH", "very_high"),
        ]),
        policy: policy("normal", "normal", "high", "very_high"),
    }
}

fn tlp(lang: Lang) -> TemplateView {
    TemplateView {
        id: "tlp",
        name: t(lang, "cls-tpl-tlp-name"),
        description: t(lang, "cls-tpl-tlp-description"),
        levels: levels(
            "tlp",
            lang,
            &[
                L("clear", "neutral", "", ""),
                L("green", "success", "TLP:GREEN", "tlp_green"),
                L("amber", "warning", "TLP:AMBER", "tlp_amber"),
                L(
                    "amber_strict",
                    "warning",
                    "TLP:AMBER+STRICT",
                    "tlp_amber_strict",
                ),
                L("red", "error", "TLP:RED", "tlp_red"),
            ],
        ),
        classes: classes(
            lang,
            &ClassLevels {
                personal: "amber",
                special: "amber_strict",
                secrets: "amber",
                payment: "amber",
                tax: "amber",
                credentials: "red",
            },
        ),
        markings: markings(&[
            ("stamp", "TLP:CLEAR", "clear"),
            ("stamp", "TLP:GREEN", "green"),
            ("stamp", "TLP:AMBER", "amber"),
            ("stamp", "TLP:AMBER+STRICT", "amber_strict"),
            ("stamp", "TLP:RED", "red"),
        ]),
        policy: policy("clear", "green", "amber", "amber_strict"),
    }
}

pub fn all(lang: Lang) -> Vec<TemplateView> {
    vec![iso(lang), tisax(lang), bsi(lang), tlp(lang)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn template(lang: Lang, id: &str) -> TemplateView {
        all(lang).into_iter().find(|t| t.id == id).unwrap()
    }

    #[test]
    fn the_bsi_levels_carry_the_official_german_wording_in_german() {
        let names: Vec<String> = template(Lang::De, "bsi")
            .levels
            .into_iter()
            .map(|l| l.name)
            .collect();
        assert_eq!(names, ["normal", "hoch", "sehr hoch"]);
    }

    #[test]
    fn rule_questions_and_markings_do_not_depend_on_the_language() {
        for lang in [Lang::De, Lang::Fr, Lang::Es, Lang::Ru, Lang::Zh] {
            for (english, translated) in all(Lang::En).into_iter().zip(all(lang)) {
                assert_eq!(english.id, translated.id);
                for (a, b) in english.classes.iter().zip(&translated.classes) {
                    assert_eq!(a.rule_question, b.rule_question);
                    assert_eq!(a.rule_positive, b.rule_positive);
                    assert_eq!(a.rule_negative, b.rule_negative);
                }
                for (a, b) in english.levels.iter().zip(&translated.levels) {
                    assert_eq!(a.marking_text, b.marking_text);
                    assert_eq!(a.filename_tag, b.filename_tag);
                }
                assert_eq!(
                    serde_json::to_value(&english.markings).unwrap(),
                    serde_json::to_value(&translated.markings).unwrap()
                );
            }
        }
    }

    #[test]
    fn every_template_has_a_credentials_class_at_its_highest_level_without_a_question() {
        for template in all(Lang::En) {
            let top = template.levels.last().unwrap();
            let class = template
                .classes
                .iter()
                .find(|c| c.id == "credentials")
                .unwrap_or_else(|| panic!("{} has no credentials class", template.id));
            assert_eq!(class.level_id, top.id, "{}", template.id);
            assert_eq!(class.patterns, ["credentials"]);
            assert_eq!(class.action, "confirm");
            assert!(class.rule_question.is_empty());
        }
    }

    #[test]
    fn every_text_resolves_in_every_language() {
        for lang in [Lang::En, Lang::De, Lang::Fr, Lang::Es, Lang::Ru, Lang::Zh] {
            for template in all(lang) {
                let texts = std::iter::once(&template.name)
                    .chain(std::iter::once(&template.description))
                    .chain(
                        template
                            .levels
                            .iter()
                            .flat_map(|l| [&l.name, &l.description]),
                    )
                    .chain(
                        template
                            .classes
                            .iter()
                            .flat_map(|c| [&c.name, &c.description]),
                    );
                for text in texts {
                    assert!(!text.is_empty() && !text.starts_with("cls-"), "{text}");
                }
            }
        }
    }
}
