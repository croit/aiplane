// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use std::collections::HashMap;

use sqlx::Row;

use super::scheme::{
    Action, Assignment, DataClass, DestinationKind, Level, MarkingKind, MarkingRule, Over, Rule,
    Scheme,
};
use crate::server::db::{DbError, Pool};

fn decode(column: &'static str, value: &str) -> DbError {
    DbError::Decode {
        column,
        source: anyhow::anyhow!("unknown value `{value}`"),
    }
}

/// Read the whole scheme: the snapshot hot paths use instead of the database.
pub async fn load_scheme(db: &Pool) -> Result<Scheme, DbError> {
    let levels = sqlx::query(
        "SELECT id, rank, name, description, color, marking_text, mip_label_id,
                filename_tag FROM classification_levels ORDER BY rank",
    )
    .fetch_all(db)
    .await?
    .iter()
    .map(|row| {
        Ok(Level {
            id: row.try_get("id")?,
            rank: row.try_get::<u32, _>("rank")?,
            name: row.try_get("name")?,
            description: row.try_get("description")?,
            color: row.try_get("color")?,
            marking_text: row.try_get("marking_text")?,
            mip_label_id: row.try_get("mip_label_id")?,
            filename_tag: row.try_get("filename_tag")?,
        })
    })
    .collect::<Result<Vec<_>, sqlx::Error>>()?;

    let mut classes = Vec::new();
    for row in sqlx::query(
        "SELECT id, name, level_id, patterns, rule_question, rule_positive, rule_negative,
                rule_threshold, action
           FROM classification_data_classes ORDER BY id",
    )
    .fetch_all(db)
    .await?
    {
        let action: String = row.try_get("action")?;
        let question: String = row.try_get("rule_question")?;
        let patterns: String = row.try_get("patterns")?;
        classes.push(DataClass {
            id: row.try_get("id")?,
            name: row.try_get("name")?,
            level_id: row.try_get("level_id")?,
            patterns: serde_json::from_str(&patterns).map_err(|source| DbError::Decode {
                column: "patterns",
                source: source.into(),
            })?,
            rule: (!question.trim().is_empty()).then(|| Rule {
                question,
                positive: row.get("rule_positive"),
                negative: row.get("rule_negative"),
                threshold: row.get("rule_threshold"),
            }),
            action: Action::parse(&action).ok_or_else(|| decode("action", &action))?,
        });
    }
    Ok(Scheme::new(levels, classes).with_assignments(assignments(db).await?))
}

/// The word list of every class that has one, as `class id -> terms`.
pub async fn class_terms(db: &Pool) -> Result<HashMap<String, Vec<String>>, DbError> {
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    for row in
        sqlx::query("SELECT class_id, term FROM classification_terms ORDER BY class_id, term")
            .fetch_all(db)
            .await?
    {
        out.entry(row.try_get("class_id")?)
            .or_default()
            .push(row.try_get("term")?);
    }
    Ok(out)
}

/// Every marking-to-level mapping.
pub async fn marking_rules(db: &Pool) -> Result<Vec<MarkingRule>, DbError> {
    let mut out = Vec::new();
    for row in sqlx::query(
        "SELECT kind, value, level_id FROM classification_markings ORDER BY kind, value",
    )
    .fetch_all(db)
    .await?
    {
        let kind: String = row.try_get("kind")?;
        out.push(MarkingRule {
            kind: MarkingKind::parse(&kind).ok_or_else(|| decode("kind", &kind))?,
            value: row.try_get("value")?,
            level_id: row.try_get("level_id")?,
        });
    }
    Ok(out)
}

/// Every destination assignment.
pub async fn assignments(db: &Pool) -> Result<Vec<Assignment>, DbError> {
    let mut out = Vec::new();
    for row in sqlx::query(
        "SELECT kind, id, level_id, over FROM classification_assignments ORDER BY kind, id",
    )
    .fetch_all(db)
    .await?
    {
        let kind: String = row.try_get("kind")?;
        let over: String = row.try_get("over")?;
        out.push(Assignment {
            kind: DestinationKind::parse(&kind).ok_or_else(|| decode("kind", &kind))?,
            id: row.try_get("id")?,
            level_id: row.try_get("level_id")?,
            over: Over::parse(&over).ok_or_else(|| decode("over", &over))?,
        });
    }
    Ok(out)
}

/// The rank of every pool that has an assignment, by pool name. A pool absent
/// from the map is at the lowest level.
pub async fn pool_ranks(db: &Pool) -> Result<HashMap<String, u32>, DbError> {
    let rows = sqlx::query(
        "SELECT a.id AS pool, l.rank AS rank
           FROM classification_assignments a
           JOIN classification_levels l ON l.id = a.level_id
          WHERE a.kind = 'pool'",
    )
    .fetch_all(db)
    .await?;
    rows.iter()
        .map(|row| Ok((row.try_get("pool")?, row.try_get::<u32, _>("rank")?)))
        .collect::<Result<_, sqlx::Error>>()
        .map_err(DbError::from)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::server::db::open;

    #[tokio::test]
    async fn a_fresh_database_holds_the_single_public_level() {
        let db = open(Path::new(":memory:")).await.unwrap();
        let scheme = load_scheme(&db).await.unwrap();
        assert!(!scheme.is_active());
        assert_eq!(scheme.lowest().id, "public");
        assert!(scheme.classes().is_empty());
        assert!(assignments(&db).await.unwrap().is_empty());
        assert!(pool_ranks(&db).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn levels_classes_and_pool_ranks_are_read_back() {
        let db = open(Path::new(":memory:")).await.unwrap();
        sqlx::query(
            "INSERT INTO classification_levels (id, rank, name) VALUES ('secret', 1, 'Secret');
             INSERT INTO classification_data_classes
                 (id, name, level_id, rule_question, rule_threshold, action)
             VALUES ('pii', 'Personal data', 'secret', 'Is it personal?', 0.7, 'confirm'),
                    ('bare', 'Bare', 'secret', '', 0.5, 'raise');
             INSERT INTO pools (name, kind, created_at, updated_at)
             VALUES ('cloud', 'chat', '2026-01-01', '2026-01-01');
             INSERT INTO classification_assignments (kind, id, level_id) VALUES ('pool', 'cloud', 'secret');",
        )
        .execute(&db)
        .await
        .unwrap();
        let scheme = load_scheme(&db).await.unwrap();
        assert!(scheme.is_active());
        let pii = scheme.classes().iter().find(|c| c.id == "pii").unwrap();
        assert_eq!(pii.action, Action::Confirm);
        assert_eq!(pii.rule.as_ref().unwrap().threshold, 0.7);
        let bare = scheme.classes().iter().find(|c| c.id == "bare").unwrap();
        assert!(bare.rule.is_none());
        assert_eq!(pool_ranks(&db).await.unwrap().get("cloud"), Some(&1));
        let all = assignments(&db).await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].over, Over::Hide);
    }

    #[tokio::test]
    async fn patterns_and_word_lists_are_read_back_per_class() {
        let db = open(Path::new(":memory:")).await.unwrap();
        sqlx::query(
            "INSERT INTO classification_levels (id, rank, name) VALUES ('secret', 1, 'Secret');
             INSERT INTO classification_data_classes (id, name, level_id, patterns)
             VALUES ('pay', 'Payment', 'secret', '[\"iban\",\"payment_card\"]'),
                    ('names', 'Names', 'secret', '[]');
             INSERT INTO classification_terms (class_id, term) VALUES ('names', 'Project Aurora'), ('names', 'Orion');",
        )
        .execute(&db)
        .await
        .unwrap();
        let scheme = load_scheme(&db).await.unwrap();
        let pay = scheme.classes().iter().find(|c| c.id == "pay").unwrap();
        assert_eq!(pay.patterns, ["iban", "payment_card"]);
        let terms = class_terms(&db).await.unwrap();
        assert_eq!(terms["names"], ["Orion", "Project Aurora"]);
        assert!(!terms.contains_key("pay"));
    }

    #[tokio::test]
    async fn marking_rules_are_read_back_by_kind() {
        let db = open(Path::new(":memory:")).await.unwrap();
        sqlx::query(
            "INSERT INTO classification_levels (id, rank, name) VALUES ('secret', 1, 'Secret');
             INSERT INTO classification_markings (kind, value, level_id)
             VALUES ('mip', '9fbde396-1a24-4c79-8edf-9254a0f35055', 'secret'),
                    ('stamp', 'VERTRAULICH', 'secret');",
        )
        .execute(&db)
        .await
        .unwrap();
        let rules = marking_rules(&db).await.unwrap();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].kind, MarkingKind::Mip);
        assert_eq!(rules[1].value, "VERTRAULICH");
        assert!(
            sqlx::query("INSERT INTO classification_markings VALUES ('filename', 'x', 'secret')")
                .execute(&db)
                .await
                .is_err()
        );
    }
}
