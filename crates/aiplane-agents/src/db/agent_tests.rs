// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Stored evaluation of an agent (`docs/agent-builder.md` → "Evaluation"):
//! test cases, and runs with a result per case.
//!
//! This layer stores shapes and knows nothing of what a script or an
//! expectation means; `aiplane-runtime::agents::eval` parses and judges them.
//! A run is written once, finished, together with its results.

use jiff::Timestamp;
use serde_json::Value;
use sqlx::Row;
use sqlx::sqlite::SqliteRow;
use uuid::Uuid;

use super::{DbError, Pool};
use crate::SEALED_SUFFIX;
use aiplane_core::server::crypto::sha256_hex;

#[derive(Debug, Clone, PartialEq)]
pub struct TestCase {
    pub id: String,
    pub principal_id: String,
    pub name: String,
    pub script: Value,
    pub expect: Value,
    pub rubric: Option<String>,
    pub created_by: String,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

pub struct CaseBody<'a> {
    pub name: &'a str,
    pub script: &'a Value,
    pub expect: &'a Value,
    pub rubric: Option<&'a str>,
}

const CASE_COLS: &str =
    "id, principal_id, name, script, expect, rubric, created_by, created_at, updated_at";

fn json_column(row: &SqliteRow, column: &'static str) -> Result<Value, DbError> {
    let text: String = row.try_get(column)?;
    serde_json::from_str(&text).map_err(|e| DbError::Decode {
        column,
        source: e.into(),
    })
}

fn map_case(row: &SqliteRow) -> Result<TestCase, DbError> {
    Ok(TestCase {
        id: row.try_get("id")?,
        principal_id: row.try_get("principal_id")?,
        name: row.try_get("name")?,
        script: json_column(row, "script")?,
        expect: json_column(row, "expect")?,
        rubric: row.try_get("rubric")?,
        created_by: row.try_get("created_by")?,
        created_at: super::parse_ts(row.try_get("created_at")?, "created_at")?,
        updated_at: super::parse_ts(row.try_get("updated_at")?, "updated_at")?,
    })
}

/// The agent's cases in the order they were written, so a suite runs and
/// reads the same way every time.
pub async fn cases(pool: &Pool, principal_id: &str) -> Result<Vec<TestCase>, DbError> {
    let rows = sqlx::query(&format!(
        "SELECT {CASE_COLS} FROM agent_test_cases WHERE principal_id = ?
         ORDER BY created_at, name"
    ))
    .bind(principal_id)
    .fetch_all(pool)
    .await?;
    rows.iter().map(map_case).collect()
}

pub async fn case(
    pool: &Pool,
    principal_id: &str,
    case_id: &str,
) -> Result<Option<TestCase>, DbError> {
    let row = sqlx::query(&format!(
        "SELECT {CASE_COLS} FROM agent_test_cases WHERE principal_id = ? AND id = ?"
    ))
    .bind(principal_id)
    .bind(case_id)
    .fetch_optional(pool)
    .await?;
    row.as_ref().map(map_case).transpose()
}

/// `None` when the agent already has a case with that name.
pub async fn create_case(
    pool: &Pool,
    principal_id: &str,
    body: &CaseBody<'_>,
    actor_id: &str,
) -> Result<Option<TestCase>, DbError> {
    let id = Uuid::new_v4().to_string();
    let now = Timestamp::now().to_string();
    let inserted = sqlx::query(
        "INSERT INTO agent_test_cases
            (id, principal_id, name, script, expect, rubric, created_by, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT (principal_id, name) DO NOTHING",
    )
    .bind(&id)
    .bind(principal_id)
    .bind(body.name)
    .bind(body.script.to_string())
    .bind(body.expect.to_string())
    .bind(body.rubric)
    .bind(actor_id)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await?
    .rows_affected();
    if inserted == 0 {
        return Ok(None);
    }
    case(pool, principal_id, &id).await
}

/// Outer `None`: no such case. Inner `None`: the new name is taken by
/// another case of the agent.
pub async fn update_case(
    pool: &Pool,
    principal_id: &str,
    case_id: &str,
    body: &CaseBody<'_>,
) -> Result<Option<Option<TestCase>>, DbError> {
    let taken: Option<String> = sqlx::query_scalar(
        "SELECT id FROM agent_test_cases WHERE principal_id = ? AND name = ? AND id <> ?",
    )
    .bind(principal_id)
    .bind(body.name)
    .bind(case_id)
    .fetch_optional(pool)
    .await?;
    if taken.is_some() {
        return Ok(Some(None));
    }
    let updated = sqlx::query(
        "UPDATE agent_test_cases
            SET name = ?, script = ?, expect = ?, rubric = ?, updated_at = ?
          WHERE principal_id = ? AND id = ?",
    )
    .bind(body.name)
    .bind(body.script.to_string())
    .bind(body.expect.to_string())
    .bind(body.rubric)
    .bind(Timestamp::now().to_string())
    .bind(principal_id)
    .bind(case_id)
    .execute(pool)
    .await?
    .rows_affected();
    if updated == 0 {
        return Ok(None);
    }
    Ok(Some(case(pool, principal_id, case_id).await?))
}

pub async fn delete_case(pool: &Pool, principal_id: &str, case_id: &str) -> Result<bool, DbError> {
    let deleted = sqlx::query("DELETE FROM agent_test_cases WHERE principal_id = ? AND id = ?")
        .bind(principal_id)
        .bind(case_id)
        .execute(pool)
        .await?
        .rows_affected();
    Ok(deleted > 0)
}

/// What the cases say, hashed: a run is only evidence for the suite it ran.
pub fn suite_hash(cases: &[TestCase]) -> String {
    let canonical: Vec<Value> = cases
        .iter()
        .map(|c| serde_json::json!([c.id, c.name, c.script, c.expect, c.rubric]))
        .collect();
    sha256_hex(Value::Array(canonical).to_string().as_bytes())
}

/// What a spec says, hashed. A sealed credential counts as a fixed marker:
/// its ciphertext changes whenever it is re-sealed, and a credential is no
/// behaviour the suite tests, so neither may make a passing run stale.
pub fn spec_hash(spec: &Value) -> String {
    let mut meaning = spec.clone();
    mask_sealed(&mut meaning);
    sha256_hex(meaning.to_string().as_bytes())
}

fn mask_sealed(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, v) in map.iter_mut() {
                if key.ends_with(SEALED_SUFFIX) && v.is_string() {
                    *v = Value::String("sealed".into());
                } else {
                    mask_sealed(v);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(mask_sealed),
        _ => {}
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TestRun {
    pub id: String,
    pub principal_id: String,
    /// `draft` or `version:N`.
    pub source: String,
    pub version: Option<i64>,
    pub spec_hash: String,
    pub suite_hash: String,
    pub started_by: String,
    pub started_at: Timestamp,
    pub finished_at: Timestamp,
    pub passed: i64,
    pub failed: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CaseResult {
    pub case_id: String,
    pub case_name: String,
    pub passed: bool,
    pub report: Value,
    pub session_id: Option<String>,
}

pub struct NewRun<'a> {
    pub principal_id: &'a str,
    pub source: &'a str,
    pub version: Option<i64>,
    pub spec_hash: &'a str,
    pub suite_hash: &'a str,
    pub started_by: &'a str,
    pub started_at: Timestamp,
    pub results: &'a [CaseResult],
}

/// Store a finished run with its results, in one transaction.
pub async fn record_run(pool: &Pool, new: &NewRun<'_>) -> Result<TestRun, DbError> {
    let id = Uuid::new_v4().to_string();
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO agent_test_runs
            (id, principal_id, source, version, spec_hash, suite_hash, started_by, started_at,
             finished_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(new.principal_id)
    .bind(new.source)
    .bind(new.version)
    .bind(new.spec_hash)
    .bind(new.suite_hash)
    .bind(new.started_by)
    .bind(new.started_at.to_string())
    .bind(Timestamp::now().to_string())
    .execute(&mut *tx)
    .await?;
    for (position, result) in new.results.iter().enumerate() {
        sqlx::query(
            "INSERT INTO agent_test_results
                (run_id, case_id, case_name, position, passed, report, session_id)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&result.case_id)
        .bind(&result.case_name)
        .bind(position as i64)
        .bind(result.passed)
        .bind(result.report.to_string())
        .bind(&result.session_id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    run(pool, new.principal_id, &id)
        .await?
        .ok_or_else(|| DbError::Query(sqlx::Error::RowNotFound))
}

const RUN_SELECT: &str = "SELECT r.id, r.principal_id, r.source, r.version, r.spec_hash,
        r.suite_hash, r.started_by, r.started_at, r.finished_at,
        COALESCE((SELECT SUM(passed) FROM agent_test_results WHERE run_id = r.id), 0) AS passed,
        COALESCE((SELECT COUNT(*) - SUM(passed) FROM agent_test_results WHERE run_id = r.id), 0)
            AS failed
     FROM agent_test_runs r";

fn map_run(row: &SqliteRow) -> Result<TestRun, DbError> {
    Ok(TestRun {
        id: row.try_get("id")?,
        principal_id: row.try_get("principal_id")?,
        source: row.try_get("source")?,
        version: row.try_get("version")?,
        spec_hash: row.try_get("spec_hash")?,
        suite_hash: row.try_get("suite_hash")?,
        started_by: row.try_get("started_by")?,
        started_at: super::parse_ts(row.try_get("started_at")?, "started_at")?,
        finished_at: super::parse_ts(row.try_get("finished_at")?, "finished_at")?,
        passed: row.try_get("passed")?,
        failed: row.try_get("failed")?,
    })
}

/// Newest first.
pub async fn runs(pool: &Pool, principal_id: &str, limit: i64) -> Result<Vec<TestRun>, DbError> {
    let rows = sqlx::query(&format!(
        "{RUN_SELECT} WHERE r.principal_id = ? ORDER BY r.started_at DESC LIMIT ?"
    ))
    .bind(principal_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    rows.iter().map(map_run).collect()
}

pub async fn run(
    pool: &Pool,
    principal_id: &str,
    run_id: &str,
) -> Result<Option<TestRun>, DbError> {
    let row = sqlx::query(&format!(
        "{RUN_SELECT} WHERE r.principal_id = ? AND r.id = ?"
    ))
    .bind(principal_id)
    .bind(run_id)
    .fetch_optional(pool)
    .await?;
    row.as_ref().map(map_run).transpose()
}

/// The newest run of the draft.
pub async fn latest_draft_run(pool: &Pool, principal_id: &str) -> Result<Option<TestRun>, DbError> {
    let row = sqlx::query(&format!(
        "{RUN_SELECT} WHERE r.principal_id = ? AND r.source = 'draft'
         ORDER BY r.started_at DESC LIMIT 1"
    ))
    .bind(principal_id)
    .fetch_optional(pool)
    .await?;
    row.as_ref().map(map_run).transpose()
}

pub async fn results(pool: &Pool, run_id: &str) -> Result<Vec<CaseResult>, DbError> {
    let rows = sqlx::query(
        "SELECT case_id, case_name, passed, report, session_id FROM agent_test_results
         WHERE run_id = ? ORDER BY position",
    )
    .bind(run_id)
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|row| {
            Ok(CaseResult {
                case_id: row.try_get("case_id")?,
                case_name: row.try_get("case_name")?,
                passed: row.try_get("passed")?,
                report: json_column(row, "report")?,
                session_id: row.try_get("session_id")?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{agents, system_principals as sp};
    use serde_json::json;
    use std::path::Path;

    async fn pool_with_agent() -> (Pool, String) {
        let pool = aiplane_core::server::db::open(Path::new(":memory:"))
            .await
            .unwrap();
        let agent = agents::create(
            &pool,
            &sp::NewPrincipal {
                name: "support",
                display: "Support",
                description: "",
            },
            "{}",
            "alice",
        )
        .await
        .unwrap()
        .unwrap();
        (pool, agent.principal.id)
    }

    fn body<'a>(name: &'a str, script: &'a Value, expect: &'a Value) -> CaseBody<'a> {
        CaseBody {
            name,
            script,
            expect,
            rubric: None,
        }
    }

    #[test]
    fn a_resealed_credential_keeps_the_spec_hash_and_any_other_change_does_not() {
        let spec = |token: &str, orchestration: &str| {
            json!({
                "main": { "instructions": { "orchestration": orchestration } },
                "routes": { "partner": { "a2a": { "auth": { "token_sealed": token } } } },
                "verifiers": { "site": { "kind": "host_jwt", "secret_sealed": token } }
            })
        };
        let before = spec_hash(&spec("nonce1.ct1", "Help."));
        assert_eq!(
            before,
            spec_hash(&spec("nonce2.ct2", "Help.")),
            "re-sealing a credential changes its ciphertext, not what the spec means"
        );
        assert_ne!(before, spec_hash(&spec("nonce1.ct1", "Help more.")));
    }

    #[tokio::test]
    async fn cases_round_trip_and_names_are_unique_per_agent() {
        let (pool, agent) = pool_with_agent().await;
        let script = json!([{ "say": "hi" }]);
        let expect = json!({ "finished": true });
        let made = create_case(&pool, &agent, &body("greets", &script, &expect), "alice")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(made.script, script);
        assert_eq!(made.expect, expect);
        assert!(
            create_case(&pool, &agent, &body("greets", &script, &expect), "alice")
                .await
                .unwrap()
                .is_none()
        );

        let other = create_case(&pool, &agent, &body("other", &script, &expect), "alice")
            .await
            .unwrap()
            .unwrap();
        let clash = update_case(&pool, &agent, &other.id, &body("greets", &script, &expect))
            .await
            .unwrap();
        assert_eq!(clash, Some(None));
        let renamed = update_case(&pool, &agent, &other.id, &body("renamed", &script, &expect))
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(renamed.name, "renamed");
        assert_eq!(cases(&pool, &agent).await.unwrap().len(), 2);
        assert!(delete_case(&pool, &agent, &other.id).await.unwrap());
        assert!(!delete_case(&pool, &agent, &other.id).await.unwrap());
    }

    #[tokio::test]
    async fn a_run_keeps_its_results_in_order_and_counts_them() {
        let (pool, agent) = pool_with_agent().await;
        let results = vec![
            CaseResult {
                case_id: "c1".into(),
                case_name: "one".into(),
                passed: true,
                report: json!({ "passed": true }),
                session_id: Some("s1".into()),
            },
            CaseResult {
                case_id: "c2".into(),
                case_name: "two".into(),
                passed: false,
                report: json!({ "passed": false }),
                session_id: None,
            },
        ];
        let run = record_run(
            &pool,
            &NewRun {
                principal_id: &agent,
                source: "draft",
                version: None,
                spec_hash: "s",
                suite_hash: "h",
                started_by: "alice",
                started_at: Timestamp::now(),
                results: &results,
            },
        )
        .await
        .unwrap();
        assert_eq!((run.passed, run.failed), (1, 1));
        let names: Vec<String> = super::results(&pool, &run.id)
            .await
            .unwrap()
            .into_iter()
            .map(|r| r.case_name)
            .collect();
        assert_eq!(names, vec!["one", "two"]);
        assert_eq!(
            latest_draft_run(&pool, &agent).await.unwrap().unwrap().id,
            run.id
        );
    }

    #[test]
    fn the_suite_hash_changes_with_an_expectation() {
        let case = |expect: Value| TestCase {
            id: "c".into(),
            principal_id: "p".into(),
            name: "n".into(),
            script: json!([]),
            expect,
            rubric: None,
            created_by: "u".into(),
            created_at: Timestamp::UNIX_EPOCH,
            updated_at: Timestamp::UNIX_EPOCH,
        };
        assert_ne!(
            suite_hash(&[case(json!({ "finished": true }))]),
            suite_hash(&[case(json!({ "finished": false }))])
        );
    }
}
