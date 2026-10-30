// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! What the administrator's editor reads and writes: the whole scheme replaced
//! in one transaction, the word lists of the data classes and the audit of
//! level changes.

use std::collections::{HashMap, HashSet};

use jiff::Timestamp;
use sqlx::Row;

use super::conversation::write_event;
use super::scheme::{Action, Assignment, Level, MarkingRule};
use crate::server::db::{DbError, Pool, WriteTx};
use crate::server::settings::PREFIX;

/// A data class as the editor edits it. The word list is not part of it.
#[derive(Debug, Clone, PartialEq)]
pub struct ClassInput {
    pub id: String,
    pub name: String,
    pub description: String,
    pub level_id: String,
    pub patterns: Vec<String>,
    pub rule_question: String,
    pub rule_positive: String,
    pub rule_negative: String,
    pub rule_threshold: f64,
    pub action: Action,
}

/// Who uploaded a class's word list, when, and how many terms it holds.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TermsInfo {
    pub count: u64,
    pub uploaded_by: Option<String>,
    pub uploaded_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClassRecord {
    pub class: ClassInput,
    pub terms: TermsInfo,
}

/// Everything one save of the editor replaces. `policy` holds the settings
/// rows (`classification.*` keys) written in the same transaction.
#[derive(Debug, Clone)]
pub struct Replacement {
    pub levels: Vec<Level>,
    pub classes: Vec<ClassInput>,
    pub markings: Vec<MarkingRule>,
    pub assignments: Vec<Assignment>,
    pub policy: Vec<(String, String)>,
}

/// One conversation moved because its level was removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovedConversation {
    pub session_id: String,
    pub from: String,
    pub to: String,
}

/// A row of the level audit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventRecord {
    pub session_id: String,
    pub at: String,
    pub from_level: Option<String>,
    pub to_level: String,
    pub cause: String,
    pub actor: String,
    pub reason: String,
}

const RANK_PARKING: i64 = 1_000_000;

fn action_of(raw: &str) -> Result<Action, DbError> {
    Action::parse(raw).ok_or_else(|| DbError::Decode {
        column: "action",
        source: anyhow::anyhow!("unknown value `{raw}`"),
    })
}

/// Every data class with the state of its word list.
pub async fn list_classes(db: &Pool) -> Result<Vec<ClassRecord>, DbError> {
    let counts: HashMap<String, i64> = sqlx::query_as::<_, (String, i64)>(
        "SELECT class_id, COUNT(*) FROM classification_terms GROUP BY class_id",
    )
    .fetch_all(db)
    .await?
    .into_iter()
    .collect();
    let mut out = Vec::new();
    for row in sqlx::query(
        "SELECT id, name, description, level_id, patterns, rule_question, rule_positive,
                rule_negative, rule_threshold, action, terms_uploaded_by, terms_uploaded_at
           FROM classification_data_classes ORDER BY id",
    )
    .fetch_all(db)
    .await?
    {
        let id: String = row.try_get("id")?;
        let patterns: String = row.try_get("patterns")?;
        let action: String = row.try_get("action")?;
        out.push(ClassRecord {
            terms: TermsInfo {
                count: counts.get(&id).copied().unwrap_or(0).unsigned_abs(),
                uploaded_by: row.try_get("terms_uploaded_by")?,
                uploaded_at: row.try_get("terms_uploaded_at")?,
            },
            class: ClassInput {
                name: row.try_get("name")?,
                description: row.try_get("description")?,
                level_id: row.try_get("level_id")?,
                patterns: serde_json::from_str(&patterns).map_err(|source| DbError::Decode {
                    column: "patterns",
                    source: source.into(),
                })?,
                rule_question: row.try_get("rule_question")?,
                rule_positive: row.try_get("rule_positive")?,
                rule_negative: row.try_get("rule_negative")?,
                rule_threshold: row.try_get("rule_threshold")?,
                action: action_of(&action)?,
                id,
            },
        });
    }
    Ok(out)
}

/// Replace levels, classes, markings, assignments and the policy rows in one
/// transaction. A class whose id is kept keeps its word list. A conversation
/// at a removed level moves to the lowest kept level above it that existed
/// before, else to the highest level of the new scheme, with an audit event of
/// cause `scheme_change`. A removed lowest level maps to the new lowest: rank 0
/// means unclassified in either scheme, so moving it up would rate a chat
/// higher than one that never had a level stored. The caller has validated the replacement.
pub async fn replace(
    db: &Pool,
    replacement: &Replacement,
    actor: &str,
) -> Result<Vec<MovedConversation>, DbError> {
    let mut tx = WriteTx::begin(db).await?;
    let old: Vec<(String, u32)> =
        sqlx::query_as("SELECT id, rank FROM classification_levels ORDER BY rank")
            .fetch_all(&mut *tx)
            .await?;
    let new_ids: HashSet<&str> = replacement.levels.iter().map(|l| l.id.as_str()).collect();
    let highest = replacement
        .levels
        .iter()
        .max_by_key(|l| l.rank)
        .map(|l| l.id.clone())
        .unwrap_or_default();
    let lowest = replacement
        .levels
        .iter()
        .min_by_key(|l| l.rank)
        .map(|l| l.id.clone())
        .unwrap_or_default();

    sqlx::query("UPDATE classification_levels SET rank = rank + ?")
        .bind(RANK_PARKING)
        .execute(&mut *tx)
        .await?;
    for level in &replacement.levels {
        sqlx::query(
            "INSERT INTO classification_levels
                 (id, rank, name, description, color, marking_text, mip_label_id, filename_tag)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
                 rank = excluded.rank, name = excluded.name,
                 description = excluded.description, color = excluded.color,
                 marking_text = excluded.marking_text, mip_label_id = excluded.mip_label_id,
                 filename_tag = excluded.filename_tag",
        )
        .bind(&level.id)
        .bind(level.rank)
        .bind(&level.name)
        .bind(&level.description)
        .bind(&level.color)
        .bind(&level.marking_text)
        .bind(&level.mip_label_id)
        .bind(&level.filename_tag)
        .execute(&mut *tx)
        .await?;
    }

    let mut moved = Vec::new();
    for (removed, removed_rank) in old.iter().filter(|(id, _)| !new_ids.contains(id.as_str())) {
        let target = if *removed_rank == 0 {
            lowest.as_str()
        } else {
            old.iter()
                .find(|(id, rank)| rank > removed_rank && new_ids.contains(id.as_str()))
                .map_or(highest.as_str(), |(id, _)| id.as_str())
        };
        let sessions: Vec<String> =
            sqlx::query_scalar("SELECT session_id FROM chat_session_settings WHERE level_id = ?")
                .bind(removed)
                .fetch_all(&mut *tx)
                .await?;
        for session_id in sessions {
            super::conversation::store(&mut tx, &session_id, target).await?;
            write_event(
                &mut tx,
                &session_id,
                removed,
                target,
                "scheme_change",
                actor,
                "",
            )
            .await?;
            moved.push(MovedConversation {
                session_id,
                from: removed.clone(),
                to: target.to_owned(),
            });
        }
    }

    sqlx::query("DELETE FROM classification_markings")
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM classification_assignments")
        .execute(&mut *tx)
        .await?;
    let kept: Vec<&str> = replacement.classes.iter().map(|c| c.id.as_str()).collect();
    let existing: Vec<String> = sqlx::query_scalar("SELECT id FROM classification_data_classes")
        .fetch_all(&mut *tx)
        .await?;
    for id in existing.iter().filter(|id| !kept.contains(&id.as_str())) {
        sqlx::query("DELETE FROM classification_data_classes WHERE id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    for class in &replacement.classes {
        sqlx::query(
            "INSERT INTO classification_data_classes
                 (id, name, description, level_id, patterns, rule_question, rule_positive,
                  rule_negative, rule_threshold, action)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
                 name = excluded.name, description = excluded.description,
                 level_id = excluded.level_id, patterns = excluded.patterns,
                 rule_question = excluded.rule_question,
                 rule_positive = excluded.rule_positive,
                 rule_negative = excluded.rule_negative,
                 rule_threshold = excluded.rule_threshold, action = excluded.action",
        )
        .bind(&class.id)
        .bind(&class.name)
        .bind(&class.description)
        .bind(&class.level_id)
        .bind(serde_json::to_string(&class.patterns).unwrap_or_else(|_| "[]".into()))
        .bind(&class.rule_question)
        .bind(&class.rule_positive)
        .bind(&class.rule_negative)
        .bind(class.rule_threshold)
        .bind(class.action.as_str())
        .execute(&mut *tx)
        .await?;
    }
    for marking in &replacement.markings {
        sqlx::query("INSERT INTO classification_markings (kind, value, level_id) VALUES (?, ?, ?)")
            .bind(marking.kind.as_str())
            .bind(&marking.value)
            .bind(&marking.level_id)
            .execute(&mut *tx)
            .await?;
    }
    for assignment in &replacement.assignments {
        sqlx::query(
            "INSERT INTO classification_assignments (kind, id, level_id, over) VALUES (?, ?, ?, ?)",
        )
        .bind(assignment.kind.as_str())
        .bind(&assignment.id)
        .bind(&assignment.level_id)
        .bind(assignment.over.as_str())
        .execute(&mut *tx)
        .await?;
    }
    for (key, value) in &replacement.policy {
        sqlx::query(
            "INSERT INTO app_settings (key, value, updated_at) VALUES (?, ?, ?)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        )
        .bind(format!("{PREFIX}{key}"))
        .bind(value)
        .bind(Timestamp::now().to_string())
        .execute(&mut *tx)
        .await?;
    }

    for (removed, _) in old.iter().filter(|(id, _)| !new_ids.contains(id.as_str())) {
        sqlx::query("DELETE FROM classification_levels WHERE id = ?")
            .bind(removed)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(moved)
}

/// Replace a class's word list. `false` when no class has that id.
pub async fn replace_terms(
    db: &Pool,
    class_id: &str,
    terms: &[String],
    uploaded_by: &str,
) -> Result<bool, DbError> {
    let mut tx = WriteTx::begin(db).await?;
    let known: Option<String> =
        sqlx::query_scalar("SELECT id FROM classification_data_classes WHERE id = ?")
            .bind(class_id)
            .fetch_optional(&mut *tx)
            .await?;
    if known.is_none() {
        return Ok(false);
    }
    sqlx::query("DELETE FROM classification_terms WHERE class_id = ?")
        .bind(class_id)
        .execute(&mut *tx)
        .await?;
    for term in terms {
        sqlx::query("INSERT OR IGNORE INTO classification_terms (class_id, term) VALUES (?, ?)")
            .bind(class_id)
            .bind(term)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query(
        "UPDATE classification_data_classes
            SET terms_uploaded_by = ?, terms_uploaded_at = ? WHERE id = ?",
    )
    .bind(uploaded_by)
    .bind(Timestamp::now().to_string())
    .bind(class_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(true)
}

/// Empty a class's word list. `false` when no class has that id.
pub async fn clear_terms(db: &Pool, class_id: &str) -> Result<bool, DbError> {
    let mut tx = WriteTx::begin(db).await?;
    let updated = sqlx::query(
        "UPDATE classification_data_classes
            SET terms_uploaded_by = NULL, terms_uploaded_at = NULL WHERE id = ?",
    )
    .bind(class_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    sqlx::query("DELETE FROM classification_terms WHERE class_id = ?")
        .bind(class_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(updated > 0)
}

/// The newest level-audit rows, newest first, of one conversation or of all.
pub async fn events(
    db: &Pool,
    session_id: Option<&str>,
    limit: u32,
) -> Result<Vec<EventRecord>, DbError> {
    let rows = sqlx::query(
        "SELECT session_id, at, from_level, to_level, cause, actor, reason
           FROM chat_classification_events
          WHERE (?1 IS NULL OR session_id = ?1)
          ORDER BY at DESC, rowid DESC LIMIT ?2",
    )
    .bind(session_id)
    .bind(limit)
    .fetch_all(db)
    .await?;
    rows.iter()
        .map(|row| {
            Ok(EventRecord {
                session_id: row.try_get("session_id")?,
                at: row.try_get("at")?,
                from_level: row.try_get("from_level")?,
                to_level: row.try_get("to_level")?,
                cause: row.try_get("cause")?,
                actor: row.try_get("actor")?,
                reason: row.try_get("reason")?,
            })
        })
        .collect::<Result<_, sqlx::Error>>()
        .map_err(DbError::from)
}

/// The cause of a conversation's latest level change, if it ever changed.
pub async fn latest_cause(db: &Pool, session_id: &str) -> Result<Option<String>, DbError> {
    Ok(events(db, Some(session_id), 1)
        .await?
        .into_iter()
        .next()
        .map(|event| event.cause))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::server::classification::{DestinationKind, MarkingKind, Over, load_scheme};
    use crate::server::db::open;

    fn level(id: &str, rank: u32) -> Level {
        Level {
            id: id.into(),
            rank,
            name: id.to_uppercase(),
            description: String::new(),
            color: "neutral".into(),
            marking_text: String::new(),
            mip_label_id: String::new(),
            filename_tag: String::new(),
        }
    }

    fn class(id: &str, level_id: &str) -> ClassInput {
        ClassInput {
            id: id.into(),
            name: id.into(),
            description: String::new(),
            level_id: level_id.into(),
            patterns: vec!["email".into()],
            rule_question: "Is it?".into(),
            rule_positive: String::new(),
            rule_negative: String::new(),
            rule_threshold: 0.6,
            action: Action::Confirm,
        }
    }

    fn replacement(levels: &[(&str, u32)]) -> Replacement {
        Replacement {
            levels: levels.iter().map(|(id, rank)| level(id, *rank)).collect(),
            classes: Vec::new(),
            markings: Vec::new(),
            assignments: Vec::new(),
            policy: Vec::new(),
        }
    }

    async fn database() -> Pool {
        let db = open(Path::new(":memory:")).await.unwrap();
        sqlx::query(
            "INSERT INTO users (id, email, created_at, updated_at)
                 VALUES ('u1', 'u1@example.com', '2026-01-01', '2026-01-01');
             INSERT INTO chat_sessions (id, user_id, created_at, updated_at)
                 VALUES ('s1', 'u1', '2026-01-01', '2026-01-01'),
                        ('s2', 'u1', '2026-01-01', '2026-01-01');",
        )
        .execute(&db)
        .await
        .unwrap();
        db
    }

    #[tokio::test]
    async fn a_replacement_reorders_levels_and_writes_every_table_and_the_policy() {
        let db = database().await;
        let mut all = replacement(&[("secret", 2), ("public", 0), ("internal", 1)]);
        all.classes.push(class("pii", "internal"));
        all.markings.push(MarkingRule {
            kind: MarkingKind::Stamp,
            value: "SECRET".into(),
            level_id: "secret".into(),
        });
        all.assignments.push(Assignment {
            kind: DestinationKind::Tool,
            id: "search_web".into(),
            level_id: "internal".into(),
            over: Over::Gate,
        });
        all.policy
            .push(("classification.mode".into(), "monitor".into()));
        replace(&db, &all, "admin").await.unwrap();

        let scheme = load_scheme(&db).await.unwrap();
        assert_eq!(
            scheme
                .levels()
                .iter()
                .map(|l| l.id.as_str())
                .collect::<Vec<_>>(),
            ["public", "internal", "secret"]
        );
        assert_eq!(scheme.classes().len(), 1);
        assert_eq!(scheme.assignments().len(), 1);
        let mode: String = sqlx::query_scalar(
            "SELECT value FROM app_settings WHERE key = 'settings.classification.mode'",
        )
        .fetch_one(&db)
        .await
        .unwrap();
        assert_eq!(mode, "monitor");
        assert_eq!(
            list_classes(&db).await.unwrap()[0].class.patterns,
            ["email"]
        );
    }

    #[tokio::test]
    async fn removing_a_level_moves_its_conversations_up_and_audits_it() {
        let db = database().await;
        replace(
            &db,
            &replacement(&[("public", 0), ("internal", 1), ("secret", 2)]),
            "admin",
        )
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO chat_session_settings (session_id, level_id, updated_at)
             VALUES ('s1', 'internal', 'now'), ('s2', 'secret', 'now')",
        )
        .execute(&db)
        .await
        .unwrap();
        let moved = replace(&db, &replacement(&[("public", 0), ("secret", 1)]), "admin")
            .await
            .unwrap();
        assert_eq!(
            moved,
            [MovedConversation {
                session_id: "s1".into(),
                from: "internal".into(),
                to: "secret".into()
            }]
        );
        assert_eq!(
            super::super::level_id(&db, "s1").await.unwrap().as_deref(),
            Some("secret")
        );
        assert_eq!(
            latest_cause(&db, "s1").await.unwrap().as_deref(),
            Some("scheme_change")
        );
        assert_eq!(latest_cause(&db, "s2").await.unwrap(), None);
    }

    #[tokio::test]
    async fn a_removed_lowest_level_sends_its_conversations_to_the_new_lowest() {
        let db = database().await;
        replace(&db, &replacement(&[("public", 0), ("secret", 1)]), "admin")
            .await
            .unwrap();
        sqlx::query("INSERT INTO chat_session_settings (session_id, level_id, updated_at) VALUES ('s1', 'public', 'now')")
            .execute(&db)
            .await
            .unwrap();
        replace(
            &db,
            &replacement(&[("normal", 0), ("high", 1), ("very_high", 2)]),
            "admin",
        )
        .await
        .unwrap();
        assert_eq!(
            super::super::level_id(&db, "s1").await.unwrap().as_deref(),
            Some("normal")
        );
    }

    #[tokio::test]
    async fn a_removed_top_level_sends_its_conversations_to_the_new_highest() {
        let db = database().await;
        replace(&db, &replacement(&[("public", 0), ("secret", 1)]), "admin")
            .await
            .unwrap();
        sqlx::query("INSERT INTO chat_session_settings (session_id, level_id, updated_at) VALUES ('s1', 'secret', 'now')")
            .execute(&db)
            .await
            .unwrap();
        replace(
            &db,
            &replacement(&[("normal", 0), ("high", 1), ("very_high", 2)]),
            "admin",
        )
        .await
        .unwrap();
        assert_eq!(
            super::super::level_id(&db, "s1").await.unwrap().as_deref(),
            Some("very_high")
        );
    }

    #[tokio::test]
    async fn a_kept_class_keeps_its_word_list_and_a_dropped_class_loses_it() {
        let db = database().await;
        let mut all = replacement(&[("public", 0), ("secret", 1)]);
        all.classes.push(class("pii", "secret"));
        all.classes.push(class("nda", "secret"));
        replace(&db, &all, "admin").await.unwrap();
        assert!(
            replace_terms(&db, "pii", &["alpha".into(), "beta".into()], "u1")
                .await
                .unwrap()
        );
        assert!(
            replace_terms(&db, "nda", &["gamma".into()], "u1")
                .await
                .unwrap()
        );
        all.classes.pop();
        replace(&db, &all, "admin").await.unwrap();
        let classes = list_classes(&db).await.unwrap();
        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0].terms.count, 2);
        assert_eq!(classes[0].terms.uploaded_by.as_deref(), Some("u1"));
        assert!(!replace_terms(&db, "gone", &[], "u1").await.unwrap());
        assert!(clear_terms(&db, "pii").await.unwrap());
        assert_eq!(
            list_classes(&db).await.unwrap()[0].terms,
            TermsInfo::default()
        );
    }
}
