// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! `/api/v0/agents/{id}/tests` and `/test-runs` — stored test cases and suite
//! runs (`docs/agent-builder.md` → "Evaluation").
//!
//! Reading cases and runs needs a `read` share, like the spec itself. Writing
//! a case or running the suite needs `write`: a run drives the agent's tools
//! as its principal, exactly as the test chat does. A suite runs
//! synchronously and the response carries the stored run.
//!
//! The same module holds the publish guard ([`require_green_suite`]): with
//! `publish.require_passing_tests` set, publishing needs a green run of the
//! draft as it is now against the suite as it is now.

use std::sync::Arc;

use jiff::Timestamp;
use rama::http::service::web::extract::State;
use rama::http::{Request, Response, StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};

use super::json_agents::{agent_at, parse_spec};
use super::json_principals::require_agent_manager;
use super::{
    bad_request, internal, json_error, json_error_with, json_ok, no_content, not_found,
    raw_path_segment,
};
use aiplane_agents::db::agent_tests::{self as tests_db, CaseBody, CaseResult, TestCase};
use aiplane_agents::db::agents::{self as agents_db, Access};
use aiplane_runtime::agents::eval::{self, EvalIssue, RubricJudge};
use aiplane_runtime::agents::eval_judge::ModelJudge;
use aiplane_runtime::agents::profile::RunOptions;
use aiplane_runtime::agents::spec::AgentSpec;
use aiplane_runtime::rama_server::state::RamaState;

const RUN_LIST_LIMIT: i64 = 50;

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CaseDto {
    pub name: String,
    /// The conversation to play (`docs/agent-builder.md` → "Evaluation").
    pub script: Value,
    /// What the run must show.
    pub expect: Value,
    /// A model-judged criterion, reported apart from the checks.
    #[serde(default)]
    pub rubric: Option<String>,
}

/// A stored test case.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct CaseView {
    pub id: String,
    pub name: String,
    pub script: Value,
    pub expect: Value,
    pub rubric: Option<String>,
    #[schemars(with = "shared::api::TimestampSchema")]
    pub created_at: Timestamp,
    #[schemars(with = "shared::api::TimestampSchema")]
    pub updated_at: Timestamp,
}

impl CaseView {
    fn of(case: &TestCase) -> Self {
        Self {
            id: case.id.clone(),
            name: case.name.clone(),
            script: case.script.clone(),
            expect: case.expect.clone(),
            rubric: case.rubric.clone(),
            created_at: case.created_at,
            updated_at: case.updated_at,
        }
    }
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct CaseReply {
    pub case: CaseView,
}

fn invalid_case(issues: &[EvalIssue]) -> Response {
    let first = &issues[0];
    let more = match issues.len() {
        1 => String::new(),
        n => format!(" (and {} more — see `issues`)", n - 1),
    };
    json_error_with(
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_test_case",
        &format!(
            "cannot save the test case: at `{}`, {}{more}",
            first.path, first.message
        ),
        serde_json::Map::from_iter([("issues".to_string(), json!(issues))]),
    )
}

/// The body of a create or update, checked. A case that does not parse is
/// refused here rather than failing on every run.
fn checked_case(body: &CaseDto) -> Result<(String, Option<String>), Response> {
    let name = body.name.trim();
    if name.is_empty() || name.chars().count() > eval::MAX_CASE_NAME_CHARS {
        return Err(bad_request(format!(
            "a test case needs a name of 1 to {} characters",
            eval::MAX_CASE_NAME_CHARS
        )));
    }
    if let Err(issues) = eval::parse_case(&body.script, &body.expect) {
        return Err(invalid_case(&issues));
    }
    let rubric = body
        .rubric
        .as_deref()
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .map(str::to_string);
    Ok((name.to_string(), rubric))
}

fn name_taken(name: &str) -> Response {
    json_error(
        StatusCode::CONFLICT,
        "conflict",
        &format!("this agent already has a test case named `{name}` — pick another name"),
    )
}

/// How many results carry each rubric verdict. Reported apart from the
/// deterministic counts, which alone decide `green`.
fn rubric_counts(results: &[CaseResult]) -> RubricCounts {
    let count = |verdict: &str| {
        results
            .iter()
            .filter(|r| {
                r.report.pointer("/rubric/verdict").and_then(Value::as_str) == Some(verdict)
            })
            .count()
    };
    RubricCounts {
        passed: count("passed"),
        failed: count("failed"),
        error: count("error"),
        skipped: count("skipped"),
    }
}

/// How many results carry each rubric verdict.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct RubricCounts {
    pub passed: usize,
    pub failed: usize,
    pub error: usize,
    pub skipped: usize,
}

/// A stored suite run.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct RunView {
    pub id: String,
    /// `draft` or `version:N`.
    pub source: String,
    pub version: Option<i64>,
    pub started_by: String,
    #[schemars(with = "shared::api::TimestampSchema")]
    pub started_at: Timestamp,
    #[schemars(with = "shared::api::TimestampSchema")]
    pub finished_at: Timestamp,
    pub passed: i64,
    pub failed: i64,
    /// No case failed and at least one passed.
    pub green: bool,
}

fn run_view(run: &tests_db::TestRun) -> RunView {
    RunView {
        id: run.id.clone(),
        source: run.source.clone(),
        version: run.version,
        started_by: run.started_by.clone(),
        started_at: run.started_at,
        finished_at: run.finished_at,
        passed: run.passed,
        failed: run.failed,
        green: run.failed == 0 && run.passed > 0,
    }
}

/// A suite run with its per-case results.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct RunDetail {
    #[serde(flatten)]
    pub run: RunView,
    pub rubric: RubricCounts,
    pub results: Vec<CaseResultView>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct CaseResultView {
    pub case_id: String,
    pub case_name: String,
    pub passed: bool,
    pub session_id: Option<String>,
    /// The case's checks and their outcome, as the evaluation wrote them.
    pub report: Value,
}

fn run_detail_view(run: &tests_db::TestRun, results: &[CaseResult]) -> RunDetail {
    RunDetail {
        run: run_view(run),
        rubric: rubric_counts(results),
        results: results
            .iter()
            .map(|r| CaseResultView {
                case_id: r.case_id.clone(),
                case_name: r.case_name.clone(),
                passed: r.passed,
                session_id: r.session_id.clone(),
                report: r.report.clone(),
            })
            .collect(),
    }
}

/// The stored cases and the newest run of the draft.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct TestSuite {
    pub cases: Vec<CaseView>,
    pub latest_draft_run: Option<RunView>,
    /// The draft and the suite are unchanged since that run.
    pub latest_draft_run_current: bool,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct RunList {
    /// Newest first.
    pub runs: Vec<RunView>,
}

/// GET /api/v0/agents/{id}/tests — the cases, and the newest run of the
/// draft, if any, and whether it still counts (the draft and the suite are
/// unchanged since).
pub async fn list(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let user = or_return!(require_agent_manager(&state, &req).await);
    let (agent, _) = or_return!(agent_at(&state, &req, &user, 1, Access::Read).await);
    let id = &agent.principal.id;
    let cases = match tests_db::cases(&state.db, id).await {
        Ok(c) => c,
        Err(e) => return internal(e),
    };
    let latest = match tests_db::latest_draft_run(&state.db, id).await {
        Ok(r) => r,
        Err(e) => return internal(e),
    };
    let current = latest.as_ref().is_some_and(|run| {
        run.spec_hash == tests_db::spec_hash(&parse_spec(&agent.draft_spec))
            && run.suite_hash == tests_db::suite_hash(&cases)
    });
    json_ok(
        StatusCode::OK,
        TestSuite {
            cases: cases.iter().map(CaseView::of).collect(),
            latest_draft_run: latest.as_ref().map(run_view),
            latest_draft_run_current: current,
        },
    )
}

/// POST /api/v0/agents/{id}/tests
pub async fn create(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let user = or_return!(require_agent_manager(&state, &req).await);
    let (agent, _) = or_return!(agent_at(&state, &req, &user, 1, Access::Write).await);
    let body: CaseDto = or_return!(super::read_json(req.into_body(), "the test case").await);
    let (name, rubric) = or_return!(checked_case(&body));
    let created = tests_db::create_case(
        &state.db,
        &agent.principal.id,
        &CaseBody {
            name: &name,
            script: &body.script,
            expect: &body.expect,
            rubric: rubric.as_deref(),
        },
        &user.id,
    )
    .await;
    match created {
        Ok(Some(case)) => json_ok(
            StatusCode::CREATED,
            CaseReply {
                case: CaseView::of(&case),
            },
        ),
        Ok(None) => name_taken(&name),
        Err(e) => internal(e),
    }
}

/// PUT /api/v0/agents/{id}/tests/{case}
pub async fn update(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let user = or_return!(require_agent_manager(&state, &req).await);
    let (agent, _) = or_return!(agent_at(&state, &req, &user, 2, Access::Write).await);
    let Some(case_id) = raw_path_segment(&req, 0) else {
        return bad_request("the URL is missing the test case id");
    };
    let body: CaseDto = or_return!(super::read_json(req.into_body(), "the test case").await);
    let (name, rubric) = or_return!(checked_case(&body));
    let updated = tests_db::update_case(
        &state.db,
        &agent.principal.id,
        &case_id,
        &CaseBody {
            name: &name,
            script: &body.script,
            expect: &body.expect,
            rubric: rubric.as_deref(),
        },
    )
    .await;
    match updated {
        Ok(Some(Some(case))) => json_ok(
            StatusCode::OK,
            CaseReply {
                case: CaseView::of(&case),
            },
        ),
        Ok(Some(None)) => name_taken(&name),
        Ok(None) => not_found(format!("this agent has no test case `{case_id}`")),
        Err(e) => internal(e),
    }
}

/// DELETE /api/v0/agents/{id}/tests/{case}
pub async fn delete(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let user = or_return!(require_agent_manager(&state, &req).await);
    let (agent, _) = or_return!(agent_at(&state, &req, &user, 2, Access::Write).await);
    let Some(case_id) = raw_path_segment(&req, 0) else {
        return bad_request("the URL is missing the test case id");
    };
    match tests_db::delete_case(&state.db, &agent.principal.id, &case_id).await {
        Ok(true) => no_content(),
        Ok(false) => not_found(format!("this agent has no test case `{case_id}`")),
        Err(e) => internal(e),
    }
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunBody {
    /// `draft`, or `version:N` for a published version.
    pub source: String,
}

/// POST /api/v0/agents/{id}/tests/run `{source: "draft" | "version:N"}` — run
/// every case in order and store the run. A version's stored spec is run
/// through the same door the draft uses, as test conversations.
pub async fn run(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let user = or_return!(require_agent_manager(&state, &req).await);
    let (agent, _) = or_return!(agent_at(&state, &req, &user, 2, Access::Write).await);
    let body: RunBody = or_return!(super::read_json(req.into_body(), "the run request").await);
    let id = &agent.principal.id;
    let (spec, version) = match body.source.as_str() {
        "draft" => (parse_spec(&agent.draft_spec), None),
        other => {
            let Some(n) = other
                .strip_prefix("version:")
                .and_then(|n| n.parse::<i64>().ok())
                .filter(|n| *n >= 1)
            else {
                return bad_request(format!(
                    "`source` is `{other}`; use `draft` or `version:N`, N being a published \
                     version — list them with GET /api/v0/agents/{id}/versions"
                ));
            };
            match agents_db::version(&state.db, id, n).await {
                Ok(Some(v)) => (parse_spec(&v.spec), Some(n)),
                Ok(None) => {
                    return not_found(format!(
                        "this agent has no version {n} — list them with GET \
                         /api/v0/agents/{id}/versions"
                    ));
                }
                Err(e) => return internal(e),
            }
        }
    };
    let cases = match tests_db::cases(&state.db, id).await {
        Ok(c) if c.is_empty() => {
            return bad_request("this agent has no test cases yet — add one, then run the suite");
        }
        Ok(c) => c,
        Err(e) => return internal(e),
    };

    let started_at = Timestamp::now();
    let options = RunOptions::default();
    let judge = match AgentSpec::from_value(&spec) {
        Ok(typed) => ModelJudge::for_agent(state.clone(), id, &typed).await,
        Err(_) => None,
    };
    let mut results = Vec::with_capacity(cases.len());
    for case in &cases {
        let outcome = eval::run_case(
            &state,
            id,
            &spec,
            case,
            &options,
            judge.as_ref().map(|j| j as &dyn RubricJudge),
        )
        .await;
        results.push(CaseResult {
            case_id: case.id.clone(),
            case_name: case.name.clone(),
            passed: outcome.passed(),
            report: serde_json::to_value(&outcome.report).unwrap_or(Value::Null),
            session_id: outcome.session_id,
        });
    }
    let recorded = tests_db::record_run(
        &state.db,
        &tests_db::NewRun {
            principal_id: id,
            source: &body.source,
            version,
            spec_hash: &tests_db::spec_hash(&spec),
            suite_hash: &tests_db::suite_hash(&cases),
            started_by: &user.id,
            started_at,
            results: &results,
        },
    )
    .await;
    match recorded {
        Ok(run) => json_ok(StatusCode::CREATED, run_detail_view(&run, &results)),
        Err(e) => internal(e),
    }
}

/// GET /api/v0/agents/{id}/test-runs — the newest runs, without their
/// per-case results.
pub async fn runs(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let user = or_return!(require_agent_manager(&state, &req).await);
    let (agent, _) = or_return!(agent_at(&state, &req, &user, 1, Access::Read).await);
    match tests_db::runs(&state.db, &agent.principal.id, RUN_LIST_LIMIT).await {
        Ok(runs) => json_ok(
            StatusCode::OK,
            RunList {
                runs: runs.iter().map(run_view).collect(),
            },
        ),
        Err(e) => internal(e),
    }
}

/// GET /api/v0/agents/{id}/test-runs/{run}
pub async fn run_detail(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let user = or_return!(require_agent_manager(&state, &req).await);
    let (agent, _) = or_return!(agent_at(&state, &req, &user, 2, Access::Read).await);
    let Some(run_id) = raw_path_segment(&req, 0) else {
        return bad_request("the URL is missing the run id");
    };
    let run = match tests_db::run(&state.db, &agent.principal.id, &run_id).await {
        Ok(Some(run)) => run,
        Ok(None) => return not_found(format!("this agent has no test run `{run_id}`")),
        Err(e) => return internal(e),
    };
    match tests_db::results(&state.db, &run.id).await {
        Ok(results) => json_ok(StatusCode::OK, run_detail_view(&run, &results)),
        Err(e) => internal(e),
    }
}

fn guard_refusal(message: String, failing: Value) -> Response {
    json_error_with(
        StatusCode::UNPROCESSABLE_ENTITY,
        "agent_tests_failing",
        &message,
        serde_json::Map::from_iter([("failing".to_string(), failing)]),
    )
}

/// What a failed case got wrong, in the words of its report.
fn failure_messages(report: &Value) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(error) = report.get("error").and_then(Value::as_str) {
        out.push(error.to_string());
    }
    for section in ["goal", "plan", "action"] {
        let checks = report
            .pointer(&format!("/{section}/checks"))
            .and_then(Value::as_array);
        for check in checks.into_iter().flatten() {
            if check.get("passed") == Some(&Value::Bool(false))
                && let Some(message) = check.get("message").and_then(Value::as_str)
            {
                out.push(format!("{section}: {message}"));
            }
        }
    }
    out
}

/// The publish guard. Passes when the draft (`checked`, as validation typed
/// it) does not ask for it (`publish.require_passing_tests`), or when the
/// newest draft run is green for the draft and the suite as they are now.
/// Otherwise 422 `agent_tests_failing`, naming the failing cases, or saying
/// the suite has to be run first.
pub(super) async fn require_green_suite(
    state: &RamaState,
    agent: &agents_db::AgentRow,
    checked: &AgentSpec,
) -> Result<(), Response> {
    if !checked.publish.require_passing_tests {
        return Ok(());
    }
    let draft = parse_spec(&agent.draft_spec);
    let id = &agent.principal.id;
    let cases = tests_db::cases(&state.db, id).await.map_err(internal)?;
    let run_it = || {
        format!(
            "run the suite against the draft first: POST /api/v0/agents/{id}/tests/run with \
             {{\"source\": \"draft\"}}"
        )
    };
    if cases.is_empty() {
        return Err(guard_refusal(
            "cannot publish: `publish.require_passing_tests` is on, and the agent has no test \
             cases — add some, or turn the setting off"
                .to_string(),
            json!([]),
        ));
    }
    let latest = tests_db::latest_draft_run(&state.db, id)
        .await
        .map_err(internal)?;
    let Some(run) = latest.filter(|r| {
        r.spec_hash == tests_db::spec_hash(&draft) && r.suite_hash == tests_db::suite_hash(&cases)
    }) else {
        return Err(guard_refusal(
            format!(
                "cannot publish: `publish.require_passing_tests` is on, and no test run covers \
                 this draft and these cases (the draft or the suite changed since the last run) \
                 — {}",
                run_it()
            ),
            json!([]),
        ));
    };
    if run.failed == 0 {
        return Ok(());
    }
    let results = tests_db::results(&state.db, &run.id)
        .await
        .map_err(internal)?;
    let failing: Vec<Value> = results
        .iter()
        .filter(|r| !r.passed)
        .map(|r| {
            json!({
                "case_id": r.case_id,
                "case_name": r.case_name,
                "problems": failure_messages(&r.report),
            })
        })
        .collect();
    Err(guard_refusal(
        format!(
            "cannot publish: {} of {} test cases fail on this draft ({}) — fix the agent or the \
             cases, run the suite again, then publish",
            failing.len(),
            results.len(),
            failing
                .iter()
                .filter_map(|f| f["case_name"].as_str())
                .map(|n| format!("`{n}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        json!(failing),
    ))
}
