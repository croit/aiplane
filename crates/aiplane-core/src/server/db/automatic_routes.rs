// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use sqlx::Row;

use super::{DbError, Pool};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutomaticRouteCandidate {
    pub key: String,
    pub target: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutomaticRoute {
    pub alias: String,
    pub selector_model: String,
    pub objective: String,
    pub instructions: String,
    pub minimum_confidence: f64,
    pub selector_timeout_ms: u64,
    pub fallback_target: String,
    pub session_affinity: bool,
    pub session_ttl_seconds: u64,
    pub rollout: String,
    #[serde(default)]
    pub version: i64,
    pub candidates: Vec<AutomaticRouteCandidate>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AutomaticRouteDecisionRow {
    pub created_at: String,
    pub route_alias: String,
    pub route_version: i64,
    pub selector_model: String,
    pub selected_target: Option<String>,
    pub effective_target: String,
    pub confidence: Option<f64>,
    pub reason: String,
    pub duration_ms: i64,
}

impl AutomaticRoute {
    pub fn validate(&self) -> Result<(), String> {
        if self.alias.trim().is_empty() {
            return Err("automatic route alias must not be empty".into());
        }
        if self.selector_model.trim().is_empty() {
            return Err("automatic route selector_model must not be empty".into());
        }
        if !matches!(self.objective.as_str(), "quality" | "balanced" | "cost") {
            return Err("automatic route objective must be quality, balanced, or cost".into());
        }
        if !(0.0..=1.0).contains(&self.minimum_confidence) {
            return Err("automatic route minimum_confidence must be between 0 and 1".into());
        }
        if !(100..=30_000).contains(&self.selector_timeout_ms) {
            return Err("automatic route selector_timeout_ms must be between 100 and 30000".into());
        }
        if !(60..=604_800).contains(&self.session_ttl_seconds) {
            return Err("automatic route session_ttl_seconds must be between 60 and 604800".into());
        }
        if !matches!(self.rollout.as_str(), "active" | "shadow") {
            return Err("automatic route rollout must be active or shadow".into());
        }
        if self.candidates.len() < 2 {
            return Err("automatic route must have at least two candidates".into());
        }
        let mut keys = HashSet::new();
        let mut targets = HashSet::new();
        for candidate in &self.candidates {
            if candidate.key.trim().is_empty()
                || candidate.target.trim().is_empty()
                || candidate.description.trim().is_empty()
            {
                return Err(
                    "automatic route candidates require a key, target, and description".into(),
                );
            }
            if candidate.target.trim() == self.alias.trim() {
                return Err("automatic route cannot contain itself as a candidate".into());
            }
            if !keys.insert(candidate.key.trim()) {
                return Err("automatic route candidate keys must be unique".into());
            }
            if !targets.insert(candidate.target.trim()) {
                return Err("automatic route candidate targets must be unique".into());
            }
        }
        if !targets.contains(self.fallback_target.trim()) {
            return Err("automatic route fallback_target must be one of its candidates".into());
        }
        Ok(())
    }
}

pub async fn all(db: &Pool) -> Result<Vec<AutomaticRoute>, DbError> {
    let aliases: Vec<String> =
        sqlx::query_scalar("SELECT alias FROM automatic_routes ORDER BY alias")
            .fetch_all(db)
            .await?;
    let mut routes = Vec::with_capacity(aliases.len());
    for alias in aliases {
        if let Some(route) = get(db, &alias).await? {
            routes.push(route);
        }
    }
    Ok(routes)
}

pub async fn get(db: &Pool, alias: &str) -> Result<Option<AutomaticRoute>, DbError> {
    let Some(row) = sqlx::query(
        r#"SELECT alias, selector_model, objective, instructions, minimum_confidence,
                  selector_timeout_ms, fallback_target, session_affinity,
                  session_ttl_seconds, rollout, version
             FROM automatic_routes WHERE alias = ?"#,
    )
    .bind(alias)
    .fetch_optional(db)
    .await?
    else {
        return Ok(None);
    };
    let candidates = sqlx::query(
        r#"SELECT key, target, description FROM automatic_route_candidates
            WHERE route_alias = ? ORDER BY sort_order, key"#,
    )
    .bind(alias)
    .fetch_all(db)
    .await?
    .into_iter()
    .map(|candidate| AutomaticRouteCandidate {
        key: candidate.get("key"),
        target: candidate.get("target"),
        description: candidate.get("description"),
    })
    .collect();
    Ok(Some(AutomaticRoute {
        alias: row.get("alias"),
        selector_model: row.get("selector_model"),
        objective: row.get("objective"),
        instructions: row.get("instructions"),
        minimum_confidence: row.get("minimum_confidence"),
        selector_timeout_ms: row.get::<i64, _>("selector_timeout_ms") as u64,
        fallback_target: row.get("fallback_target"),
        session_affinity: row.get::<i64, _>("session_affinity") != 0,
        session_ttl_seconds: row.get::<i64, _>("session_ttl_seconds") as u64,
        rollout: row.get("rollout"),
        version: row.get("version"),
        candidates,
    }))
}

pub async fn upsert(db: &Pool, route: &AutomaticRoute) -> Result<i64, DbError> {
    route.validate().map_err(|message| DbError::Decode {
        column: "automatic_route",
        source: anyhow::anyhow!(message),
    })?;
    let now = jiff::Timestamp::now().to_string();
    let mut tx = db.begin().await?;
    sqlx::query(
        r#"INSERT INTO automatic_routes
              (alias, selector_model, objective, instructions, minimum_confidence,
               selector_timeout_ms, fallback_target, session_affinity, session_ttl_seconds,
               rollout, version, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?)
            ON CONFLICT(alias) DO UPDATE SET
              selector_model = excluded.selector_model,
              objective = excluded.objective,
              instructions = excluded.instructions,
              minimum_confidence = excluded.minimum_confidence,
              selector_timeout_ms = excluded.selector_timeout_ms,
              fallback_target = excluded.fallback_target,
              session_affinity = excluded.session_affinity,
              session_ttl_seconds = excluded.session_ttl_seconds,
              rollout = excluded.rollout,
              version = automatic_routes.version + 1,
              updated_at = excluded.updated_at"#,
    )
    .bind(route.alias.trim())
    .bind(route.selector_model.trim())
    .bind(&route.objective)
    .bind(route.instructions.trim())
    .bind(route.minimum_confidence)
    .bind(route.selector_timeout_ms as i64)
    .bind(route.fallback_target.trim())
    .bind(i64::from(route.session_affinity))
    .bind(route.session_ttl_seconds as i64)
    .bind(&route.rollout)
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM automatic_route_candidates WHERE route_alias = ?")
        .bind(route.alias.trim())
        .execute(&mut *tx)
        .await?;
    for (sort_order, candidate) in route.candidates.iter().enumerate() {
        sqlx::query(
            r#"INSERT INTO automatic_route_candidates
                  (route_alias, key, target, description, sort_order)
                VALUES (?, ?, ?, ?, ?)"#,
        )
        .bind(route.alias.trim())
        .bind(candidate.key.trim())
        .bind(candidate.target.trim())
        .bind(candidate.description.trim())
        .bind(sort_order as i64)
        .execute(&mut *tx)
        .await?;
    }
    let version: i64 = sqlx::query_scalar("SELECT version FROM automatic_routes WHERE alias = ?")
        .bind(route.alias.trim())
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(version)
}

pub async fn delete(db: &Pool, alias: &str) -> Result<bool, DbError> {
    let result = sqlx::query("DELETE FROM automatic_routes WHERE alias = ?")
        .bind(alias)
        .execute(db)
        .await?;
    Ok(result.rows_affected() != 0)
}

pub async fn recent_decisions(
    db: &Pool,
    limit: u32,
) -> Result<Vec<AutomaticRouteDecisionRow>, DbError> {
    let rows = sqlx::query(
        r#"SELECT created_at, route_alias, route_version, selector_model,
                  selected_target, effective_target, confidence, reason, duration_ms
             FROM automatic_route_decisions ORDER BY id DESC LIMIT ?"#,
    )
    .bind(i64::from(limit.min(500)))
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| AutomaticRouteDecisionRow {
            created_at: row.get("created_at"),
            route_alias: row.get("route_alias"),
            route_version: row.get("route_version"),
            selector_model: row.get("selector_model"),
            selected_target: row.get("selected_target"),
            effective_target: row.get("effective_target"),
            confidence: row.get("confidence"),
            reason: row.get("reason"),
            duration_ms: row.get("duration_ms"),
        })
        .collect())
}

#[derive(Debug, Clone)]
pub(crate) struct DecisionInsert {
    pub route_alias: String,
    pub route_version: i64,
    pub selector_model: String,
    pub selected_key: Option<String>,
    pub selected_target: Option<String>,
    pub effective_target: String,
    pub confidence: Option<f64>,
    pub reason: String,
    pub eligible_targets: Vec<String>,
    pub probabilities: Option<serde_json::Value>,
    pub duration_ms: i64,
}

pub(crate) async fn record_decisions(
    db: &Pool,
    decisions: &[DecisionInsert],
) -> Result<(), DbError> {
    let mut tx = db.begin().await?;
    for decision in decisions {
        sqlx::query(
            r#"INSERT INTO automatic_route_decisions
              (created_at, route_alias, route_version, selector_model, selected_key,
               selected_target, effective_target, confidence, reason, eligible_targets,
               probabilities, duration_ms)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
        )
        .bind(jiff::Timestamp::now().to_string())
        .bind(&decision.route_alias)
        .bind(decision.route_version)
        .bind(&decision.selector_model)
        .bind(&decision.selected_key)
        .bind(&decision.selected_target)
        .bind(&decision.effective_target)
        .bind(decision.confidence)
        .bind(&decision.reason)
        .bind(serde_json::to_string(&decision.eligible_targets).unwrap_or_else(|_| "[]".into()))
        .bind(decision.probabilities.as_ref().map(ToString::to_string))
        .bind(decision.duration_ms)
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query(
        "DELETE FROM automatic_route_decisions WHERE id <= (SELECT MAX(id) - 100000 FROM automatic_route_decisions)",
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route() -> AutomaticRoute {
        AutomaticRoute {
            alias: "default".into(),
            selector_model: "jev-latest".into(),
            objective: "balanced".into(),
            instructions: "Prefer the coding model for software tasks.".into(),
            minimum_confidence: 0.65,
            selector_timeout_ms: 750,
            fallback_target: "fast".into(),
            session_affinity: true,
            session_ttl_seconds: 3600,
            rollout: "shadow".into(),
            version: 0,
            candidates: vec![
                AutomaticRouteCandidate {
                    key: "quick".into(),
                    target: "fast".into(),
                    description: "Fast inexpensive general model".into(),
                },
                AutomaticRouteCandidate {
                    key: "expert".into(),
                    target: "coding".into(),
                    description: "Strong software engineering model".into(),
                },
            ],
        }
    }

    #[test]
    fn validation_rejects_recursive_and_ambiguous_routes() {
        let mut value = route();
        value.candidates[1].target = "default".into();
        assert_eq!(
            value.validate().unwrap_err(),
            "automatic route cannot contain itself as a candidate"
        );
        value.candidates[1].target = "fast".into();
        assert_eq!(
            value.validate().unwrap_err(),
            "automatic route candidate targets must be unique"
        );
        value.candidates[1].target = "coding".into();
        value.candidates[1].key = " quick ".into();
        assert_eq!(
            value.validate().unwrap_err(),
            "automatic route candidate keys must be unique"
        );
    }

    #[tokio::test]
    async fn round_trips_and_versions_a_route() {
        let db = crate::server::db::open(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        assert_eq!(upsert(&db, &route()).await.unwrap(), 1);
        let stored = get(&db, "default").await.unwrap().unwrap();
        assert_eq!(stored.version, 1);
        assert_eq!(stored.candidates, route().candidates);

        let mut changed = route();
        changed.rollout = "active".into();
        assert_eq!(upsert(&db, &changed).await.unwrap(), 2);
        assert_eq!(all(&db).await.unwrap()[0].version, 2);
    }

    #[tokio::test]
    async fn deleting_a_route_cascades_its_candidates() {
        let db = crate::server::db::open(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        upsert(&db, &route()).await.unwrap();
        assert!(delete(&db, "default").await.unwrap());
        assert!(get(&db, "default").await.unwrap().is_none());
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM automatic_route_candidates")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn route_cannot_be_created_when_an_existing_route_targets_its_alias() {
        let db = crate::server::db::open(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        let mut outer = route();
        outer.candidates[1].target = "future-route".into();
        upsert(&db, &outer).await.unwrap();

        let mut nested = route();
        nested.alias = "future-route".into();
        assert!(upsert(&db, &nested).await.is_err());
    }

    #[tokio::test]
    async fn recent_decisions_are_newest_first() {
        let db = crate::server::db::open(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        let mut stored = route();
        stored.version = upsert(&db, &stored).await.unwrap();
        let eligible = vec!["fast".into(), "coding".into()];
        record_decisions(
            &db,
            &[DecisionInsert {
                route_alias: stored.alias.clone(),
                route_version: stored.version,
                selector_model: stored.selector_model.clone(),
                selected_key: Some("quick".into()),
                selected_target: Some("fast".into()),
                effective_target: "fast".into(),
                confidence: Some(0.8),
                reason: "selected".into(),
                eligible_targets: eligible,
                probabilities: None,
                duration_ms: 12,
            }],
        )
        .await
        .unwrap();
        let rows = recent_decisions(&db, 10).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].route_alias, "default");
        assert_eq!(rows[0].effective_target, "fast");
    }
}
