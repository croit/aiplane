use super::*;
use crate::agents::state::tests::{at, pool_with_session, schema};
use crate::agents::state::{AgentState, Provenance};
use aiplane_agents::db::agent_state;

fn tools() -> SlotTools {
    SlotTools::with_clock(Arc::new(schema()), Arc::new(|| at("2026-10-02T12:00:00Z")))
}

fn ctx(pool: &aiplane_core::server::db::Pool) -> ToolContext {
    ToolContext {
        session_id: Some("s1".into()),
        ..ToolContext::for_test(pool.clone())
    }
}

async fn call(
    pool: &aiplane_core::server::db::Pool,
    tool: &str,
    args: Value,
) -> Result<Value, ToolError> {
    let Some(t) = tools().get(tool) else {
        return Err(ToolError::InvalidArgs(format!("no tool {tool}")));
    };
    t.run(ctx(pool), args).await
}

#[test]
fn a_tool_exists_only_for_slots_the_model_may_write() {
    let t = tools();
    assert_eq!(
        t.ids(),
        [
            "set_email",
            "set_issue",
            "set_name",
            "set_order",
            "set_score",
            "set_seats",
            "set_urgent"
        ]
    );
    for trusted in ["set_verified", "set_plan"] {
        assert!(!t.contains(trusted), "{trusted}");
    }
    assert_eq!(
        t.defs_for(&[
            "set_name".into(),
            "set_verified".into(),
            "rag_search".into()
        ])
        .len(),
        1
    );
}

#[test]
fn a_definition_takes_exactly_a_value_of_the_slots_shape() {
    let def = tools().get("set_issue").unwrap().schema();
    assert_eq!(def.function.name, "set_issue");
    assert_eq!(
        def.function.parameters,
        json!({
            "type": "object",
            "properties": { "value": { "enum": ["billing", "technical"] } },
            "required": ["value"],
            "additionalProperties": false
        })
    );
    let name = tools().get("set_name").unwrap().schema();
    assert!(
        name.function.description.contains("the visitor's name"),
        "{}",
        name.function.description
    );
}

#[tokio::test]
async fn a_valid_call_stores_the_value_as_written_by_the_model() {
    let pool = pool_with_session("s1").await;
    let out = call(&pool, "set_issue", json!({ "value": "billing" }))
        .await
        .unwrap();
    assert_eq!(out["status"], "set");

    let state = AgentState::load(&pool, &schema(), "s1").await.unwrap();
    let entry = state.valid("issue").unwrap();
    assert_eq!(entry.value, json!("billing"));
    assert_eq!(entry.provenance, Provenance::Llm);
    assert_eq!(entry.set_at, at("2026-10-02T12:00:00Z"));
}

#[tokio::test]
async fn an_invalid_value_is_refused_with_the_validators_message() {
    let pool = pool_with_session("s1").await;
    let err = call(&pool, "set_email", json!({ "value": "not-an-address" }))
        .await
        .unwrap_err();
    let ToolError::InvalidArgs(msg) = &err else {
        panic!("{err:?}");
    };
    assert!(msg.contains("not an email address"), "{msg}");
    assert!(msg.contains("call set_email again"), "{msg}");
    assert!(
        agent_state::for_session(&pool, "s1")
            .await
            .unwrap()
            .is_empty()
    );
}

/// Whatever the model sends, a verifier-only slot stays unwritten: there is
/// no tool for it, and no argument of another tool reaches it.
#[tokio::test]
async fn the_model_cannot_write_a_verifier_only_slot_whatever_it_sends() {
    let pool = pool_with_session("s1").await;
    let attempts: &[(&str, Value)] = &[
        ("set_verified", json!({ "value": { "customer_id": "K-1" } })),
        ("set_plan", json!({ "value": "gold" })),
        ("set_name", json!({ "value": "Ada", "slot": "verified" })),
        (
            "set_name",
            json!({ "value": "Ada", "provenance": "verifier:otp" }),
        ),
        ("set_name", json!({ "value": "Ada", "provenance": "host" })),
        (
            "set_name",
            json!({ "slot": "verified", "value": { "customer_id": "K-1" } }),
        ),
        ("set_name", json!({ "session_id": "s2", "value": "Ada" })),
        ("set_name", json!("Ada")),
        ("set_name", json!({})),
    ];
    for (tool, args) in attempts {
        assert!(
            call(&pool, tool, args.clone()).await.is_err(),
            "{tool}({args}) was accepted"
        );
    }
    assert!(
        agent_state::for_session(&pool, "s1")
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn an_extra_argument_is_named_in_the_refusal() {
    let pool = pool_with_session("s1").await;
    let err = call(
        &pool,
        "set_name",
        json!({ "value": "Ada", "provenance": "host" }),
    )
    .await
    .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("`provenance`"), "{msg}");
    assert!(msg.contains("only `value`"), "{msg}");
}

#[tokio::test]
async fn outside_an_agent_conversation_the_tool_refuses() {
    let pool = pool_with_session("s1").await;
    let tool = tools().get("set_name").unwrap();
    let err = tool
        .run(
            ToolContext::for_test(pool.clone()),
            json!({ "value": "Ada" }),
        )
        .await
        .unwrap_err();
    assert!(err.to_string().contains("agent conversation"), "{err}");
}
