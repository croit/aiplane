// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The starting points the editor offers: the level scheme of a common
//! classification standard, with data classes and the markings its documents
//! usually carry. A template is data the operator then edits; none of it is a
//! certification. German stamps and level texts are domain content: they are
//! what a German document carries, not interface strings.

use super::types::{ClassBody, LevelBody, MarkingBody, OutputBody, TemplatePolicy, TemplateView};

/// A level: id, name, description, colour, visible marking, filename tag.
struct L(
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
);

fn levels(rows: &[L]) -> Vec<LevelBody> {
    rows.iter()
        .enumerate()
        .map(
            |(rank, L(id, name, description, color, marking_text, tag))| LevelBody {
                id: (*id).into(),
                rank: rank as u32,
                name: (*name).into(),
                description: (*description).into(),
                color: (*color).into(),
                marking_text: (*marking_text).into(),
                mip_label_id: String::new(),
                filename_tag: (*tag).into(),
            },
        )
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

/// The levels the five data classes sit at: personal data, special
/// categories, trade secrets, payment data, tax identification number.
struct ClassLevels {
    personal: &'static str,
    special: &'static str,
    secrets: &'static str,
    payment: &'static str,
    tax: &'static str,
}

fn classes(at: &ClassLevels) -> Vec<ClassBody> {
    let class = |id: &str,
                 name: &str,
                 description: &str,
                 level_id: &str,
                 patterns: &[&str],
                 question: &str,
                 positive: &str,
                 negative: &str,
                 threshold: f64,
                 action: &str| ClassBody {
        id: id.into(),
        name: name.into(),
        description: description.into(),
        level_id: level_id.into(),
        patterns: patterns.iter().map(|p| (*p).into()).collect(),
        rule_question: question.into(),
        rule_positive: positive.into(),
        rule_negative: negative.into(),
        rule_threshold: threshold,
        action: action.into(),
    };
    vec![
        class(
            "personal_data",
            "Personal data",
            "Information relating to an identified or identifiable natural person (GDPR Art. 4).",
            at.personal,
            &["email"],
            "Does the text contain personal data in the sense of GDPR Article 4(1), that is any \
             information relating to an identified or identifiable natural person, such as a name \
             together with an address, a telephone number, a date of birth, an employee or \
             customer number, or a record about a named person?",
            "A customer list with names and e-mail addresses. An employee's contract with name and \
             salary. A support ticket from a named person with their home address.",
            "A product description. A press release. Source code without user data. A person's \
             name in a published byline.",
            0.6,
            "raise",
        ),
        class(
            "special_categories",
            "Special categories of personal data",
            "Health, biometric, genetic, religious, political, trade-union, sexual-orientation or \
             ethnic-origin data about a person (GDPR Art. 9).",
            at.special,
            &[],
            "Does the text contain special categories of personal data in the sense of GDPR \
             Article 9 about an identifiable person: health data (diagnoses, sick leave, \
             medication, disability), genetic or biometric data, racial or ethnic origin, \
             political opinions, religious or philosophical beliefs, trade-union membership, or \
             sex life or sexual orientation?",
            "A sick note naming the employee and the diagnosis. A works-council membership list. \
             A medical report for a named patient.",
            "A general article about a disease. Statistics without any individual. A policy about \
             sick-leave procedure.",
            0.6,
            "raise",
        ),
        class(
            "trade_secrets",
            "Trade secrets and NDA-covered content",
            "Information the company protects as a secret or has agreed to keep confidential.",
            at.secrets,
            &[],
            "Does the text contain trade secrets or information covered by a non-disclosure \
             agreement, such as unreleased product plans, source code of proprietary products, \
             pricing and margin calculations, customer contract terms, merger plans or research \
             results that are not public?",
            "An NDA with a partner and the technical details shared under it. A price calculation \
             with margins. An unreleased roadmap.",
            "A published datasheet. Public documentation of an open-source project. A \
             marketing brochure.",
            0.6,
            "confirm",
        ),
        class(
            "payment_data",
            "Payment data",
            "Bank account and payment card numbers.",
            at.payment,
            &["iban", "payment_card"],
            "Does the text contain payment data: a bank account number (IBAN), a payment card \
             number with its expiry date or security code, or account credentials for a payment \
             service?",
            "An invoice with the customer's IBAN. A card number pasted into a support request.",
            "The text of a payment policy that names no account. A price list.",
            0.7,
            "raise",
        ),
        class(
            "tax_id",
            "Tax identification number",
            "The German personal tax identification number (Steuer-ID).",
            at.tax,
            &["de_tax_id"],
            "",
            "",
            "",
            0.5,
            "raise",
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

fn iso() -> TemplateView {
    TemplateView {
        id: "iso",
        name: "ISO/IEC 27001 (four levels)",
        description: "Public, internal, confidential and strictly confidential: the scheme most \
                      ISO 27001 information classification policies use.",
        levels: levels(&[
            L(
                "public",
                "Public",
                "May be shared with anyone.",
                "success",
                "",
                "",
            ),
            L(
                "internal",
                "Internal",
                "For staff and trusted partners; not for the public.",
                "info",
                "INTERNAL",
                "internal",
            ),
            L(
                "confidential",
                "Confidential",
                "Limited to those who need it; disclosure harms the business.",
                "warning",
                "CONFIDENTIAL",
                "confidential",
            ),
            L(
                "strictly_confidential",
                "Strictly confidential",
                "A named circle only; disclosure causes severe damage.",
                "error",
                "STRICTLY CONFIDENTIAL",
                "strictly_confidential",
            ),
        ]),
        classes: classes(&ClassLevels {
            personal: "internal",
            special: "strictly_confidential",
            secrets: "confidential",
            payment: "confidential",
            tax: "confidential",
        }),
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

fn tisax() -> TemplateView {
    TemplateView {
        id: "tisax",
        name: "TISAX / VDA ISA",
        description: "The four protection classes of the VDA information security assessment: \
                      public, internal, confidential and strictly confidential, with the German \
                      stamps automotive documents carry.",
        levels: levels(&[
            L(
                "public",
                "Public",
                "Öffentlich: may be published.",
                "success",
                "",
                "",
            ),
            L(
                "internal",
                "Internal",
                "Intern: for employees and contractors bound by agreement.",
                "info",
                "INTERN",
                "intern",
            ),
            L(
                "confidential",
                "Confidential",
                "Vertraulich: a defined group of recipients.",
                "warning",
                "VERTRAULICH",
                "vertraulich",
            ),
            L(
                "strictly_confidential",
                "Strictly confidential",
                "Streng vertraulich: named recipients only.",
                "error",
                "STRENG VERTRAULICH",
                "streng_vertraulich",
            ),
        ]),
        classes: classes(&ClassLevels {
            personal: "internal",
            special: "strictly_confidential",
            secrets: "confidential",
            payment: "confidential",
            tax: "confidential",
        }),
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

fn bsi() -> TemplateView {
    TemplateView {
        id: "bsi",
        name: "BSI IT-Grundschutz protection requirement",
        description: "The three protection requirement categories of BSI IT-Grundschutz: normal, \
                      high and very high.",
        levels: levels(&[
            L(
                "normal",
                "Normal",
                "Schutzbedarf normal: damage is limited and manageable.",
                "success",
                "SCHUTZBEDARF NORMAL",
                "",
            ),
            L(
                "high",
                "High",
                "Schutzbedarf hoch: damage can be considerable.",
                "warning",
                "SCHUTZBEDARF HOCH",
                "hoch",
            ),
            L(
                "very_high",
                "Very high",
                "Schutzbedarf sehr hoch: damage can threaten the existence of the organisation.",
                "error",
                "SCHUTZBEDARF SEHR HOCH",
                "sehr_hoch",
            ),
        ]),
        classes: classes(&ClassLevels {
            personal: "high",
            special: "very_high",
            secrets: "high",
            payment: "high",
            tax: "high",
        }),
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

fn tlp() -> TemplateView {
    TemplateView {
        id: "tlp",
        name: "TLP 2.0 (Traffic Light Protocol)",
        description: "The five sharing labels of FIRST's Traffic Light Protocol 2.0: clear, green, \
                      amber, amber+strict and red.",
        levels: levels(&[
            L(
                "clear",
                "TLP:CLEAR",
                "No limit on disclosure.",
                "neutral",
                "",
                "",
            ),
            L(
                "green",
                "TLP:GREEN",
                "The community, not the public.",
                "success",
                "TLP:GREEN",
                "tlp_green",
            ),
            L(
                "amber",
                "TLP:AMBER",
                "The organisation and the clients that need it.",
                "warning",
                "TLP:AMBER",
                "tlp_amber",
            ),
            L(
                "amber_strict",
                "TLP:AMBER+STRICT",
                "The organisation only.",
                "warning",
                "TLP:AMBER+STRICT",
                "tlp_amber_strict",
            ),
            L(
                "red",
                "TLP:RED",
                "Named recipients only; no further sharing.",
                "error",
                "TLP:RED",
                "tlp_red",
            ),
        ]),
        classes: classes(&ClassLevels {
            personal: "amber",
            special: "amber_strict",
            secrets: "amber",
            payment: "amber",
            tax: "amber",
        }),
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

pub fn all() -> Vec<TemplateView> {
    vec![iso(), tisax(), bsi(), tlp()]
}
