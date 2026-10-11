// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Human in the loop (`docs/agent-hil.md`): a tool whose spec says `always_ask`, a handoff
//! to a person through `request_human` and through a `human` route, the
//! inbox that lists them for the right people, and the notification that
//! goes out once per pause.

use std::time::Duration;

use super::suspend::{answer, staff, visitor_says, wait_settled};
use super::*;
use crate::agents::inbox::{self, Standing, Viewer};
use crate::agents::resume::{AgentResume, claim, resume_expired, run_claimed};
use aiplane_agents::db::agent_channels::{self, ChannelKind, NewChannel};
use aiplane_agents::db::agents::{self as agents_db, Access, SubjectKind};
use aiplane_agents::db::run_sessions;
use session_core::db::{Decision, DecisionKind, DenyReason, SuspensionKind};

const ECHO: &str = "company_echo";

fn asking_spec(permission: &str) -> Value {
    json!({ "main": {
        "model": "support-model",
        "instructions": { "orchestration": "Help the visitor." },
        "tools": [ECHO],
        "tool_resources": { ECHO: { "permission": permission, "approval_timeout": "2h" } },
        "budget": { "rounds": 6 }
    } })
}

async fn asking(world: &World, permission: &str) -> String {
    let id = world
        .agent(
            "support",
            &[(GrantKind::Model, "support-model"), (GrantKind::Tool, ECHO)],
        )
        .await;
    assert_eq!(world.issues(&id, &asking_spec(permission)).await, []);
    world.publish(&id, &asking_spec(permission)).await;
    id
}

async fn decide(world: &World, agent: &str, paused: &AgentReply, decision: Decision) -> AgentReply {
    let claimed = claim(
        &world.state,
        AgentResume {
            agent_id: agent,
            ..answer(paused, decision, staff())
        },
    )
    .await
    .unwrap();
    run_claimed(&world.state, claimed, RunOptions::default())
        .await
        .unwrap()
}

#[tokio::test]
async fn an_always_ask_tool_waits_for_staff_and_runs_once_approved() {
    let main = llm(vec![
        call("e1", ECHO, json!({"message": "refund RE-1"})),
        text("Done: the refund is issued."),
    ])
    .await;
    let world = World::new(&[("support-pool", "support-model", &main)], None).await;
    let agent = asking(&world, "always_ask").await;

    let paused = visitor_says(&world, &agent, "Refund RE-1, please.").await;
    assert_eq!(paused.status, chat::TurnStatus::Suspended);
    let waiting = paused.suspension.clone().unwrap();
    assert_eq!(waiting.kind, SuspensionKind::Approval);
    assert_eq!(waiting.tool.as_deref(), Some(ECHO));
    assert_eq!(
        waiting.options,
        [DecisionKind::AllowOnce, DecisionKind::Deny]
    );
    let lasts = waiting.expires_at.duration_since(jiff::Timestamp::now());
    assert!(
        lasts.as_secs() > 7000 && lasts.as_secs() <= 7200,
        "the spec's approval_timeout: {lasts:?}"
    );
    assert_eq!(requests(&main).await.len(), 1, "the echo has not run");

    let done = decide(&world, &agent, &paused, Decision::AllowOnce).await;
    assert_eq!(done.status, chat::TurnStatus::Completed);
    assert_eq!(done.answer.as_deref(), Some("Done: the refund is issued."));
    assert!(tool_answer(&requests(&main).await, "e1").contains("refund RE-1"));
}

#[tokio::test]
async fn a_denied_always_ask_call_is_a_tool_error_and_never_runs() {
    let main = llm(vec![
        call("e1", ECHO, json!({"message": "refund RE-1"})),
        text("I could not issue the refund."),
    ])
    .await;
    let world = World::new(&[("support-pool", "support-model", &main)], None).await;
    let agent = asking(&world, "always_ask").await;

    let paused = visitor_says(&world, &agent, "Refund RE-1, please.").await;
    let done = decide(
        &world,
        &agent,
        &paused,
        Decision::Deny {
            reason: DenyReason::User,
        },
    )
    .await;
    assert_eq!(done.status, chat::TurnStatus::Completed);
    let denied = tool_answer(&requests(&main).await, "e1").to_string();
    assert!(
        !denied.contains("refund RE-1"),
        "the echo never ran: {denied}"
    );
    let row = chat::list_turns(world.db(), &paused.session_id)
        .await
        .unwrap()
        .into_iter()
        .find(|t| t.turn.id == paused.turn_id)
        .unwrap();
    assert_eq!(row.tool_calls[0].status, chat::ToolCallStatus::Errored);
}

#[tokio::test]
async fn an_always_ask_approval_nobody_gives_in_time_is_denied() {
    let main = llm(vec![
        call("e1", ECHO, json!({"message": "refund RE-1"})),
        text("Nobody approved it in time."),
    ])
    .await;
    let world = World::new(&[("support-pool", "support-model", &main)], None).await;
    let agent = asking(&world, "always_ask").await;
    let paused = visitor_says(&world, &agent, "Refund RE-1, please.").await;
    sqlx::query("UPDATE chat_turn_suspensions SET expires_at = ?")
        .bind((jiff::Timestamp::now() - jiff::SignedDuration::from_secs(1)).to_string())
        .execute(world.db())
        .await
        .unwrap();

    resume_expired(&world.state).await;
    let done = wait_settled(&world, &paused.session_id, &paused.turn_id).await;
    assert_eq!(done.status, chat::TurnStatus::Completed);
    let denied = tool_answer(&requests(&main).await, "e1").to_string();
    assert!(denied.contains("expired"), "{denied}");
    assert!(!denied.contains("refund RE-1"), "{denied}");
}

#[tokio::test]
async fn always_allow_runs_without_asking() {
    let main = llm(vec![
        call("e1", ECHO, json!({"message": "hello"})),
        text("Echoed."),
    ])
    .await;
    let world = World::new(&[("support-pool", "support-model", &main)], None).await;
    let agent = asking(&world, "always_allow").await;
    let done = visitor_says(&world, &agent, "Echo hello.").await;
    assert_eq!(done.status, chat::TurnStatus::Completed);
    assert!(tool_answer(&requests(&main).await, "e1").contains("hello"));
}

fn handoff_spec(human: Value) -> Value {
    json!({
        "main": {
            "model": "support-model",
            "instructions": { "orchestration": "Hand refunds to a person." },
            "budget": { "rounds": 6 }
        },
        "state": {
            "issue": { "type": "enum", "values": ["refund"], "set_by": ["llm"],
                "description": "What the visitor needs" },
            "verified": { "type": "subject", "set_by": ["host"] }
        },
        "routes": {
            "people": {
                "description": "a refund only staff can approve",
                "when": { "slot": "issue", "eq": "refund" },
                "human": human
            }
        }
    })
}

async fn handing_off(world: &World, human: Value) -> String {
    let id = world
        .agent("support", &[(GrantKind::Model, "support-model")])
        .await;
    assert_eq!(world.issues(&id, &handoff_spec(human.clone())).await, []);
    world.publish(&id, &handoff_spec(human)).await;
    id
}

const QUESTION: &str = "May we refund invoice RE-1 twice billed?";

#[tokio::test]
async fn request_human_hands_off_to_a_responder_whose_answer_reaches_the_visitor() {
    let main = llm(vec![
        call("s1", "set_issue", json!({"value": "refund"})),
        call("h1", "request_human", json!({"question": QUESTION})),
        text("Good news: our team approved your refund."),
    ])
    .await;
    let world = World::new(&[("support-pool", "support-model", &main)], None).await;
    let agent = handing_off(&world, json!({ "inbox": "billing desk" })).await;

    let paused = visitor_says(&world, &agent, "I was billed twice for RE-1.").await;
    assert_eq!(paused.status, chat::TurnStatus::Suspended);
    let waiting = paused.suspension.clone().unwrap();
    assert_eq!(waiting.kind, SuspensionKind::HumanAnswer);
    assert_eq!(waiting.message.as_deref(), Some(QUESTION));
    assert_eq!(waiting.options, [DecisionKind::Value, DecisionKind::Deny]);
    let first = requests(&main).await;
    assert!(
        offered(&first[0]).contains(&"request_human"),
        "offered because the spec has a human route"
    );
    assert!(system(&first[0]).contains("request_human"));

    let nobody = Viewer {
        user_id: "u2".into(),
        groups: vec![],
    };
    assert!(inbox::list(&world.state, &nobody).await.unwrap().is_empty());
    agents_db::set_share(
        world.db(),
        &agent,
        SubjectKind::Group,
        "support",
        Access::Respond,
        "u1",
    )
    .await
    .unwrap();
    use aiplane_core::server::db::{gateway_groups, users};
    gateway_groups::upsert_group(world.db(), "support", "", false, false)
        .await
        .unwrap();
    gateway_groups::set_mappings_for_group(world.db(), "support", &["support".into()])
        .await
        .unwrap();
    let now = jiff::Timestamp::now();
    let sam_user = users::User {
        id: "sam".into(),
        email: "sam@example.com".into(),
        name: None,
        roles: vec!["support".into()],
        created_at: now,
        updated_at: now,
        timezone: None,
        speech_voice: None,
    };
    users::upsert(world.db(), &sam_user).await.unwrap();
    world.state.reload_rbac().await;
    let sam = Viewer::of(&world.state, &sam_user);
    let items = inbox::list(&world.state, &sam).await.unwrap();
    assert_eq!(items.len(), 1);
    let item = &items[0];
    assert_eq!(item.id, waiting.request_id);
    assert_eq!(item.standing, Standing::Responder);
    assert_eq!(item.question.as_deref(), Some(QUESTION));
    assert_eq!(item.call, None, "a handoff shows no tool");
    let context = item.context.clone().unwrap();
    assert_eq!(context["visitor_message"], "I was billed twice for RE-1.");
    assert_eq!(context["inbox"], "billing desk");
    assert_eq!(
        context["slots"],
        json!([{ "slot": "issue", "label": "What the visitor needs", "value": "refund" }]),
        "each slot by the label its manager gave it"
    );
    assert!(
        context.get("transcript").is_none(),
        "no transcript unless configured"
    );

    let audit = world.audit(&agent).await;
    let handoff = audit
        .iter()
        .find(|e| e.kind == "human_handoff")
        .expect("the handoff is audited");
    assert!(handoff.chain.is_some(), "with the run chain");
    assert_eq!(handoff.detail["route"], "people");
    assert_eq!(handoff.detail["via"], "request_human");

    let done = decide(
        &world,
        &agent,
        &paused,
        Decision::Value {
            value: json!("Yes, refund it."),
        },
    )
    .await;
    assert_eq!(done.status, chat::TurnStatus::Completed);
    assert_eq!(
        done.answer.as_deref(),
        Some("Good news: our team approved your refund.")
    );
    let relayed = tool_answer(&requests(&main).await, "h1").to_string();
    assert!(relayed.contains("Yes, refund it."), "{relayed}");
    assert!(inbox::list(&world.state, &sam).await.unwrap().is_empty());
}

#[tokio::test]
async fn a_handoff_nobody_answers_tells_the_visitor_so_in_their_language() {
    let main = llm(vec![
        call("s1", "set_issue", json!({"value": "refund"})),
        call("h1", "request_human", json!({"question": QUESTION})),
    ])
    .await;
    let world = World::new(&[("support-pool", "support-model", &main)], None).await;
    let agent = handing_off(&world, json!({ "timeout": "5m", "transcript": true })).await;
    let paused = run_turn_with(
        &world.state,
        AgentTurn {
            agent_id: &agent,
            session_id: None,
            message: "Ich wurde doppelt belastet.",
            visitor_id: None,
            lang: Some(Lang::De),
        },
        RunOptions::default(),
    )
    .await
    .unwrap();
    let lasts = paused
        .suspension
        .as_ref()
        .unwrap()
        .expires_at
        .duration_since(jiff::Timestamp::now());
    assert!(lasts.as_secs() <= 300, "the route's timeout: {lasts:?}");
    let pending = run_sessions::pending_by_request(
        world.db(),
        &paused.suspension.as_ref().unwrap().request_id,
    )
    .await
    .unwrap()
    .unwrap();
    let transcript = &pending.suspension.run_context.unwrap()["handoff"]["transcript"];
    assert_eq!(transcript[0]["text"], "Ich wurde doppelt belastet.");

    sqlx::query("UPDATE chat_turn_suspensions SET expires_at = ?")
        .bind((jiff::Timestamp::now() - jiff::SignedDuration::from_secs(1)).to_string())
        .execute(world.db())
        .await
        .unwrap();
    let sent_before = requests(&main).await.len();
    resume_expired(&world.state).await;
    let done = wait_settled(&world, &paused.session_id, &paused.turn_id).await;
    assert_eq!(done.status, chat::TurnStatus::Completed);
    assert_eq!(
        done.content.as_deref(),
        Some(session_core::i18n::t(Lang::De, "agent-human-no-answer").as_str())
    );
    assert_eq!(
        requests(&main).await.len(),
        sent_before,
        "the fallback needs no model call"
    );
}

#[tokio::test]
async fn forward_request_on_a_human_route_hands_off_too() {
    let main = llm(vec![
        call("s1", "set_issue", json!({"value": "refund"})),
        call("fwd", "forward_request", json!({})),
        text("A colleague confirmed it."),
    ])
    .await;
    let world = World::new(&[("support-pool", "support-model", &main)], None).await;
    let agent = handing_off(&world, json!({})).await;

    let paused = visitor_says(&world, &agent, "Refund RE-1.").await;
    let waiting = paused.suspension.clone().unwrap();
    assert_eq!(waiting.kind, SuspensionKind::HumanAnswer);
    assert_eq!(
        waiting.message.as_deref(),
        Some("a refund only staff can approve"),
        "the route's description is the question"
    );
    let done = decide(
        &world,
        &agent,
        &paused,
        Decision::Value {
            value: json!("Confirmed."),
        },
    )
    .await;
    assert_eq!(done.answer.as_deref(), Some("A colleague confirmed it."));
    assert!(tool_answer(&requests(&main).await, "fwd").contains("Confirmed."));
}

/// A person in the `admins` group, which is flagged `is_admin`.
async fn admin(world: &World, id: &str) {
    use aiplane_core::server::db::{gateway_groups, users};
    gateway_groups::upsert_group(world.db(), "admins", "", true, false)
        .await
        .unwrap();
    gateway_groups::set_mappings_for_group(world.db(), "admins", &["admins".into()])
        .await
        .unwrap();
    world.state.reload_rbac().await;
    let now = jiff::Timestamp::now();
    users::upsert(
        world.db(),
        &users::User {
            id: id.into(),
            email: format!("{id}@example.com"),
            name: None,
            roles: vec!["admins".into()],
            created_at: now,
            updated_at: now,
            timezone: None,
            speech_voice: None,
        },
    )
    .await
    .unwrap();
}

/// Every channel here posts to a loopback relay: the admin's does, as the
/// operator's own choice; the manager's (`u1`) never does, because a URL a
/// manager chose goes through `outbound_guard`, which keeps the gateway out
/// of its own network.
#[tokio::test]
async fn a_pause_is_announced_once_on_the_agents_channels() {
    let hook = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/hook"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&hook)
        .await;
    Mock::given(method("POST"))
        .and(path("/managers-hook"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&hook)
        .await;
    let main = llm(vec![
        call("s1", "set_issue", json!({"value": "refund"})),
        call("h1", "request_human", json!({"question": QUESTION})),
    ])
    .await;
    let world = World::new(&[("support-pool", "support-model", &main)], None).await;
    admin(&world, "root").await;
    let agent = handing_off(&world, json!({ "notify": ["slack"] })).await;
    for (name, kind, details, hook_path, by) in [
        ("ops", ChannelKind::Slack, true, "/hook", "root"),
        ("chat", ChannelKind::Discord, false, "/hook", "root"),
        ("team", ChannelKind::Slack, true, "/managers-hook", "u1"),
    ] {
        let url = format!("{}{hook_path}", hook.uri());
        agent_channels::create(
            world.db(),
            &world.state.crypto,
            &agent,
            &NewChannel {
                kind,
                name,
                url: &url,
                url_host: "127.0.0.1",
                details,
                lang: "en",
            },
            by,
        )
        .await
        .unwrap();
    }

    let paused = visitor_says(&world, &agent, "I was billed twice for RE-1.").await;
    let request_id = paused.suspension.unwrap().request_id;
    for _ in 0..200 {
        if !requests(&hook).await.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        !inbox::announce(&world.state, &request_id).await,
        "already announced"
    );
    tokio::time::sleep(Duration::from_millis(100)).await;
    let sent = requests(&hook).await;
    assert_eq!(
        sent.len(),
        1,
        "once, and only on Slack as the route says: {sent:?}"
    );
    let text = sent[0]["text"].as_str().unwrap();
    assert!(text.contains("support"), "{text}");
    assert!(
        text.contains(QUESTION),
        "details are on for this channel: {text}"
    );
    assert!(
        text.contains(&format!("/inbox?item={request_id}")),
        "{text}"
    );
    assert!(
        !text.contains("billed twice for RE-1"),
        "the visitor's message is not in the notice: {text}"
    );
}

#[tokio::test]
async fn a_persons_paused_run_is_their_own_inbox_item() {
    let main = llm(vec![call("e1", ECHO, json!({"message": "x"}))]).await;
    let tools = crate::server::tools::ToolRegistry::new().with(
        crate::server::tools::ask_first::AskFirst::new(
            crate::server::tools::echo::Echo,
            Duration::from_secs(3600),
        ),
    );
    let world = World::build(
        &[("support-pool", "support-model", &main)],
        None,
        false,
        tools,
        None,
    )
    .await;
    let (session, turn) = crate::server::headless::open_session(
        world.db(),
        crate::server::headless::OpenParams {
            owner: crate::server::headless::Owner::User("u1"),
            title: "Nightly refunds",
            prompt: "Refund what is due.",
            model: "support-model",
            existing_session: None,
            dot_id: None,
        },
    )
    .await
    .unwrap();
    gateway_tools_for(&world, "u1").await;
    let action = crate::server::scheduled::create(
        world.db(),
        crate::server::scheduled::NewAction {
            owner: crate::server::background::AutomationOwner::User("u1".into()),
            created_by: "u1".into(),
            agent_version: None,
            dot_id: None,
            name: "Nightly refunds".into(),
            prompt: "Refund what is due.".into(),
            model: Some("support-model".into()),
            cron: "0 0 * * *".into(),
            timezone: "UTC".into(),
            tools_enabled: true,
            reuse_conversation: false,
            reuse_rounds: 3,
            next_run_at: None,
            linked_session_id: None,
        },
    )
    .await
    .unwrap();
    let run = crate::server::scheduled::record_run_start(world.db(), &action.id)
        .await
        .unwrap();
    crate::server::scheduled::attach_run_turn(world.db(), &run, &session, &turn, true, None)
        .await
        .unwrap();
    crate::server::headless::drive(
        &world.state,
        crate::server::headless::DriveParams {
            actor: crate::agent_run::Actor::person("u1", vec!["everyone".into()]),
            session_id: session.clone(),
            assistant_turn_id: turn.clone(),
            model: "support-model".into(),
            source: aiplane_core::server::db::usage::UsageSource::Scheduled,
            history_limit: None,
        },
    )
    .await;
    let row = chat::get_turn(world.db(), &session, &turn)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        row.status,
        chat::TurnStatus::Suspended,
        "{:?}",
        row.error_message
    );

    let owner = Viewer {
        user_id: "u1".into(),
        groups: vec![],
    };
    let items = inbox::list(&world.state, &owner).await.unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].standing, Standing::Owner);
    assert_eq!(items[0].title.as_deref(), Some("Nightly refunds"));
    assert_eq!(items[0].call.as_ref().unwrap().name, ECHO);
    let stranger = Viewer {
        user_id: "u2".into(),
        groups: vec![],
    };
    assert!(
        inbox::list(&world.state, &stranger)
            .await
            .unwrap()
            .is_empty()
    );
}

/// Grant every registered tool to `user`'s groups, so a person's run is
/// offered them.
async fn gateway_tools_for(world: &World, _user: &str) {
    use aiplane_core::server::db::gateway_groups;
    gateway_groups::upsert_group(world.db(), "everyone", "", false, true)
        .await
        .unwrap();
    gateway_groups::set_tools_for_group(world.db(), "everyone", &[ECHO.to_string()])
        .await
        .unwrap();
    world.state.reload_rbac().await;
}

/// An agent whose `always_ask` echo pauses the visitor's turn, with a human
/// route to hand off to and an output filter on invoice numbers.
fn resuming_spec() -> Value {
    let mut spec = handoff_spec(json!({ "timeout": "5m" }));
    spec["main"]["tools"] = json!([ECHO]);
    spec["main"]["tool_resources"] =
        json!({ ECHO: { "permission": "always_ask", "approval_timeout": "2h" } });
    spec["publish"] = json!({ "output_filter": { "patterns": { "invoice": "RE-\\d+" } } });
    spec
}

async fn resuming(world: &World) -> String {
    let id = world
        .agent(
            "support",
            &[(GrantKind::Model, "support-model"), (GrantKind::Tool, ECHO)],
        )
        .await;
    assert_eq!(world.issues(&id, &resuming_spec()).await, []);
    world.publish(&id, &resuming_spec()).await;
    id
}

async fn visitor_in(world: &World, agent: &str, lang: Lang, message: &str) -> AgentReply {
    let paused = run_turn_with(
        &world.state,
        AgentTurn {
            agent_id: agent,
            session_id: None,
            message,
            visitor_id: None,
            lang: Some(lang),
        },
        RunOptions::default(),
    )
    .await
    .unwrap();
    assert_eq!(paused.status, chat::TurnStatus::Suspended, "{paused:?}");
    paused
}

async fn staff_in(world: &World, agent: &str, paused: &AgentReply) -> AgentReply {
    let claimed = claim(
        &world.state,
        AgentResume {
            agent_id: agent,
            ..answer(paused, Decision::AllowOnce, staff())
        },
    )
    .await
    .unwrap();
    run_claimed(&world.state, claimed, RunOptions::default())
        .await
        .unwrap()
}

#[tokio::test]
async fn a_resume_by_staff_leaves_an_english_visitor_in_english() {
    let main = llm(vec![
        call("e1", ECHO, json!({"message": "hello"})),
        text("RE-999 is refunded."),
    ])
    .await;
    let world = World::new(&[("support-pool", "support-model", &main)], None).await;
    let agent = resuming(&world).await;
    let paused = visitor_in(&world, &agent, Lang::En, "Refund me, please.").await;

    let done = staff_in(&world, &agent, &paused).await;
    assert_eq!(
        done.answer.as_deref(),
        Some(session_core::i18n::t(Lang::En, "agent-output-withheld").as_str())
    );
}

#[tokio::test]
async fn a_handoff_after_a_resume_is_recorded_in_the_visitors_language() {
    let main = llm(vec![
        call("e1", ECHO, json!({"message": "hello"})),
        call("s1", "set_issue", json!({"value": "refund"})),
        call("h1", "request_human", json!({"question": QUESTION})),
    ])
    .await;
    let world = World::new(&[("support-pool", "support-model", &main)], None).await;
    let agent = resuming(&world).await;
    let paused = visitor_in(&world, &agent, Lang::De, "Bitte erstatten.").await;

    let handed_off = staff_in(&world, &agent, &paused).await;
    let waiting = handed_off.suspension.expect("handed off to a person");
    assert_eq!(waiting.kind, SuspensionKind::HumanAnswer);
    let pending = run_sessions::pending_by_request(world.db(), &waiting.request_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        pending.suspension.run_context.unwrap()["handoff"]["lang"],
        "de"
    );
}

#[tokio::test]
async fn a_timeout_for_a_german_visitor_answers_in_german() {
    let main = llm(vec![
        call("e1", ECHO, json!({"message": "hello"})),
        text("RE-999 is refunded."),
    ])
    .await;
    let world = World::new(&[("support-pool", "support-model", &main)], None).await;
    let agent = resuming(&world).await;
    let paused = visitor_in(&world, &agent, Lang::De, "Bitte erstatten.").await;
    sqlx::query("UPDATE chat_turn_suspensions SET expires_at = ?")
        .bind((jiff::Timestamp::now() - jiff::SignedDuration::from_secs(1)).to_string())
        .execute(world.db())
        .await
        .unwrap();

    resume_expired(&world.state).await;
    wait_settled(&world, &paused.session_id, &paused.turn_id).await;
    for _ in 0..400 {
        if world.state.chats.get(&agent, &paused.session_id).is_none() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let done = wait_settled(&world, &paused.session_id, &paused.turn_id).await;
    assert_eq!(
        done.content.as_deref(),
        Some(session_core::i18n::t(Lang::De, "agent-output-withheld").as_str()),
        "the filter has ruled once the sweeper's hold is released"
    );
}
