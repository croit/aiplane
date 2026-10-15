use std::sync::Arc;

use serde_json::json;

use super::*;
use aiplane_core::server::principal::{GrantSet, SystemPrincipal};
use aiplane_core::server::run_chain::{CallSite, Frame};

fn principal(id: &str, name: &str) -> SystemPrincipal {
    SystemPrincipal {
        id: id.into(),
        name: name.into(),
        grants: Arc::new(GrantSet::default()),
    }
}

async fn memory() -> Pool {
    aiplane_core::server::db::open(std::path::Path::new(":memory:"))
        .await
        .unwrap()
}

fn main_chain(conversation: &str) -> RunChain {
    RunChain::root(
        conversation,
        Some("v-7".into()),
        Frame::for_principal(&principal("p-main", "support-website"), Some(2)),
    )
}

fn billing_chain(conversation: &str) -> RunChain {
    main_chain(conversation)
        .enter(
            Frame::for_principal(&principal("p-bill", "billing"), Some(5)).called_from(CallSite {
                turn_id: "t-main".into(),
                tool_call_id: "call-1".into(),
            }),
        )
        .unwrap()
}

async fn run_event(pool: &Pool, chain: &RunChain, kind: AuditKind, detail: Value) -> Appended {
    append_now(
        pool,
        NewEvent::new(kind, &chain.current().principal_id, detail)
            .in_run(Some(chain))
            .at(Correlation {
                session_id: Some(chain.root_session.clone()),
                turn_id: Some("t-main".into()),
                round: Some(1),
                call_id: Some("call-1".into()),
            }),
    )
    .await
    .unwrap()
}

async fn chain_rows(pool: &Pool, key: &str) -> Vec<StoredEvent> {
    let sql = format!("SELECT {COLUMNS} FROM agent_audit WHERE chain_key = ? ORDER BY seq");
    sqlx::query(&sql)
        .bind(key)
        .fetch_all(pool)
        .await
        .unwrap()
        .iter()
        .map(|r| stored(r).unwrap())
        .collect()
}

#[tokio::test]
async fn a_sub_agents_event_lands_in_the_conversations_chain_with_every_correlation_id() {
    let pool = memory().await;
    let chain = billing_chain("s-visitor");

    run_event(
        &pool,
        &chain,
        AuditKind::ToolCall,
        json!({"tool": "lookup_invoice", "decision": "allowed"}),
    )
    .await;

    let rows = chain_rows(&pool, "conversation:s-visitor").await;
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.principal_id, "p-bill");
    assert_eq!(row.agent_id.as_deref(), Some("p-main"));
    assert_eq!(row.version, Some(5));
    assert_eq!(row.conversation_id.as_deref(), Some("s-visitor"));
    assert_eq!(row.visitor_id.as_deref(), Some("v-7"));
    assert_eq!(row.round, Some(1));
    assert_eq!(row.call_id.as_deref(), Some("call-1"));
    assert_eq!(row.seq, Some(1));
    assert_eq!(row.prev_hash, None);
    assert_eq!(
        row.hash.as_deref(),
        row.expected_hash(&key_ring()).as_deref()
    );
    assert_eq!(
        row.key_id.as_deref(),
        Some(UNKEYED),
        "no ring in this process"
    );
    assert!(for_principal(&pool, "p-main").await.unwrap().is_empty());
    let events = for_principal(&pool, "p-bill").await.unwrap();
    assert_eq!(events[0].chain, Some(chain.to_json()));
}

#[tokio::test]
async fn a_management_event_extends_its_agents_own_chain_on_the_callers_transaction() {
    let pool = memory().await;
    for n in 0..3 {
        let mut tx = WriteTx::begin(&pool).await.unwrap();
        append(
            &mut tx,
            NewEvent::new(AuditKind::GrantAdded, "p1", json!({"kind": "tool", "n": n}))
                .by(Some("alice")),
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
    }
    let mut tx = WriteTx::begin(&pool).await.unwrap();
    append(
        &mut tx,
        NewEvent::new(AuditKind::GrantAdded, "p1", json!({})).by(Some("alice")),
    )
    .await
    .unwrap();
    drop(tx);

    let rows = chain_rows(&pool, "agent:p1").await;
    assert_eq!(
        rows.iter().map(|r| r.seq.unwrap()).collect::<Vec<_>>(),
        [1, 2, 3],
        "a rolled-back change leaves no event"
    );
    assert_eq!(rows[1].prev_hash, rows[0].hash);
    assert_eq!(rows[2].prev_hash, rows[1].hash);
    assert_eq!(rows[0].actor_id.as_deref(), Some("alice"));
    assert_eq!(rows[0].chain, None);
    let v = verify(&pool, "p1").await.unwrap();
    assert!(v.ok(), "{v:?}");
    assert_eq!((v.chains, v.events), (1, 3));
}

#[tokio::test]
async fn verify_names_the_first_link_that_was_changed_removed_or_never_chained() {
    let pool = memory().await;
    let chain = main_chain("s1");
    for n in 0..4 {
        run_event(&pool, &chain, AuditKind::RouteDecision, json!({"n": n})).await;
    }
    assert!(verify(&pool, "p-main").await.unwrap().ok());

    sqlx::query(
        "UPDATE agent_audit SET detail = '{\"n\":99}' WHERE chain_key = 'conversation:s1' AND seq = 3",
    )
    .execute(&pool)
    .await
    .unwrap();
    let broken = verify_full(&pool, "p-main").await.unwrap().broken.unwrap();
    assert_eq!(
        (broken.chain_key.as_str(), broken.seq),
        ("conversation:s1", 3)
    );
    assert!(
        broken.reason.contains("does not match its hash"),
        "{broken:?}"
    );

    sqlx::query("DELETE FROM agent_audit WHERE chain_key = 'conversation:s1' AND seq = 2")
        .execute(&pool)
        .await
        .unwrap();
    let broken = verify_full(&pool, "p-main").await.unwrap().broken.unwrap();
    assert_eq!(broken.seq, 2);
    assert!(broken.reason.contains("missing"), "{broken:?}");

    sqlx::query(
        "INSERT INTO agent_audit (id, kind, principal_id, detail, created_at, agent_id)
         VALUES ('old', 'tool_call', 'p-main', '{}', '2026-01-01T00:00:00Z', 'p-main')",
    )
    .execute(&pool)
    .await
    .unwrap();
    let broken = verify(&pool, "p-main").await.unwrap().broken.unwrap();
    assert_eq!(broken.event_id.as_deref(), Some("old"));
    assert!(broken.reason.contains("no chain"), "{broken:?}");
}

#[tokio::test]
async fn a_relinked_event_breaks_the_chain_at_the_link_it_skipped() {
    let pool = memory().await;
    let chain = main_chain("s1");
    let first = run_event(&pool, &chain, AuditKind::TurnStarted, json!({})).await;
    run_event(&pool, &chain, AuditKind::TurnFinished, json!({})).await;
    sqlx::query(
        "UPDATE agent_audit SET prev_hash = 'forged' WHERE chain_key = 'conversation:s1' AND seq = 2",
    )
    .execute(&pool)
    .await
    .unwrap();
    let broken = verify(&pool, "p-main").await.unwrap().broken.unwrap();
    assert_eq!(broken.seq, 2);
    assert!(broken.reason.contains("prev_hash"));
    assert_ne!(first.hash, "forged");
}

#[tokio::test]
async fn parallel_writers_to_one_conversation_lose_no_event_and_never_fork_the_chain() {
    let dir = tempfile::tempdir().unwrap();
    let pool = aiplane_core::server::db::open(&dir.path().join("log.sqlite"))
        .await
        .unwrap();
    let writers: Vec<_> = (0..40)
        .map(|n| {
            let pool = pool.clone();
            tokio::spawn(async move {
                let chain = if n % 2 == 0 {
                    main_chain("s-busy")
                } else {
                    billing_chain("s-busy")
                };
                run_event(&pool, &chain, AuditKind::ToolResult, json!({"n": n})).await
            })
        })
        .collect();
    for w in writers {
        w.await.unwrap();
    }
    let rows = chain_rows(&pool, "conversation:s-busy").await;
    assert_eq!(
        rows.iter().map(|r| r.seq.unwrap()).collect::<Vec<_>>(),
        (1..=40).collect::<Vec<_>>()
    );
    let mut ns: Vec<i64> = rows
        .iter()
        .map(|r| {
            serde_json::from_str::<Value>(&r.detail).unwrap()["n"]
                .as_i64()
                .unwrap()
        })
        .collect();
    ns.sort();
    assert_eq!(ns, (0..40).collect::<Vec<_>>());
    let v = verify(&pool, "p-main").await.unwrap();
    assert!(v.ok(), "{v:?}");
    assert_eq!(v.events, 40);
}

#[tokio::test]
async fn a_page_follows_its_filters_and_cursor_and_stops_at_its_byte_cap() {
    let pool = memory().await;
    let a = main_chain("s-a");
    let b = main_chain("s-b");
    for n in 0..5 {
        run_event(&pool, &a, AuditKind::LlmExchange, json!({"n": n})).await;
    }
    run_event(
        &pool,
        &b,
        AuditKind::ToolResult,
        json!({"big": "x".repeat(10_000)}),
    )
    .await;
    run_event(
        &pool,
        &b,
        AuditKind::ToolResult,
        json!({"big": "y".repeat(10_000)}),
    )
    .await;

    let q = ActivityQuery {
        agent_id: "p-main".into(),
        conversation_id: Some("s-a".into()),
        order: Order::Asc,
        limit: 2,
        max_bytes: usize::MAX,
        ..ActivityQuery::default()
    };
    let first = page(&pool, &q).await.unwrap();
    assert_eq!(first.events.len(), 2);
    let second = page(
        &pool,
        &ActivityQuery {
            cursor: first.next_cursor,
            ..q.clone()
        },
    )
    .await
    .unwrap();
    let ns: Vec<String> = first
        .events
        .iter()
        .chain(&second.events)
        .map(|e| e.detail.clone())
        .collect();
    assert_eq!(ns, ["{\"n\":0}", "{\"n\":1}", "{\"n\":2}", "{\"n\":3}"]);

    let capped = page(
        &pool,
        &ActivityQuery {
            agent_id: "p-main".into(),
            kinds: vec![AuditKind::ToolResult],
            limit: 50,
            max_bytes: 5_000,
            ..ActivityQuery::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(capped.events.len(), 1, "one event always fits");
    assert!(capped.events[0].detail.contains('y'), "newest first");
    assert!(capped.next_cursor.is_some());

    assert!(
        page(
            &pool,
            &ActivityQuery {
                agent_id: "p-other".into(),
                limit: 50,
                max_bytes: usize::MAX,
                ..ActivityQuery::default()
            },
        )
        .await
        .unwrap()
        .events
        .is_empty()
    );
}

#[tokio::test]
async fn the_decision_trail_leaves_the_content_events_out() {
    let pool = memory().await;
    let chain = main_chain("s1");
    run_event(&pool, &chain, AuditKind::LlmExchange, json!({})).await;
    run_event(&pool, &chain, AuditKind::ToolCall, json!({})).await;
    let kinds: Vec<String> = for_principal(&pool, "p-main")
        .await
        .unwrap()
        .into_iter()
        .map(|e| e.kind)
        .collect();
    assert_eq!(kinds, ["tool_call"]);
}

#[tokio::test]
async fn the_sweep_takes_whole_chains_of_gone_conversations_only() {
    let pool = memory().await;
    let agent = crate::db::agents::create(
        &pool,
        &crate::db::system_principals::NewPrincipal {
            name: "support",
            display: "Support",
            description: "",
        },
        "{}",
        "alice",
    )
    .await
    .unwrap()
    .unwrap()
    .principal;
    let live = crate::db::run_sessions::create_principal_session(
        &pool,
        &crate::db::run_sessions::NewRunSession {
            principal_id: &agent.id,
            title: None,
            parent_turn_id: None,
            agent_version: Some(1),
        },
    )
    .await
    .unwrap()
    .id;
    let chain_of = |conversation: &str| {
        RunChain::root(
            conversation,
            None,
            Frame::for_principal(&principal(&agent.id, &agent.name), Some(1)),
        )
    };
    let (gone, kept) = (chain_of("s-gone"), chain_of(&live));
    for _ in 0..3 {
        run_event(&pool, &gone, AuditKind::ToolCall, json!({})).await;
        run_event(&pool, &kept, AuditKind::ToolCall, json!({})).await;
    }
    let agent_events = chain_rows(&pool, &agent_chain(&agent.id)).await.len();
    assert!(
        agent_events >= 1,
        "creating the agent is an event of its own chain"
    );

    let early = sweep_conversation_chains(&pool, &agent.id, Timestamp::UNIX_EPOCH)
        .await
        .unwrap();
    assert_eq!(early, SweptChains::default(), "nothing is old enough yet");

    let later = Timestamp::now() + jiff::SignedDuration::from_secs(60);
    let swept = sweep_conversation_chains(&pool, &agent.id, later)
        .await
        .unwrap();
    assert_eq!(
        swept,
        SweptChains {
            chains: 1,
            events: 3,
            agent_events: agent_events as u64,
        }
    );
    assert!(chain_rows(&pool, "conversation:s-gone").await.is_empty());
    assert_eq!(
        chain_rows(&pool, &conversation_chain(&live)).await.len(),
        3,
        "a conversation that still exists keeps its log"
    );
    let own = chain_rows(&pool, &agent_chain(&agent.id)).await;
    assert_eq!(
        own.iter().map(|e| e.kind.as_str()).collect::<Vec<_>>(),
        ["chain_checkpoint", "activity_swept"],
        "the agent's own chain is cut back behind a checkpoint, then gains the sweep's marker"
    );
    let marker: Value = serde_json::from_str(&own.last().unwrap().detail).unwrap();
    assert_eq!(own.last().unwrap().kind, "activity_swept");
    assert_eq!(marker["chain_key"], "conversation:s-gone");
    assert_eq!(marker["events"], 3);
    assert!(verify(&pool, &agent.id).await.unwrap().ok());
}

#[tokio::test]
async fn the_head_and_the_sweep_never_read_a_row_behind_its_payload() {
    let pool = memory().await;
    for sql in [HEAD_SQL, SWEEP_SQL] {
        let plan: Vec<(i64, i64, i64, String)> =
            sqlx::query_as(&format!("EXPLAIN QUERY PLAN {sql}"))
                .bind("x")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert!(
            plan.iter()
                .any(|(_, _, _, step)| step.contains("COVERING INDEX")),
            "{sql}\n{plan:?}"
        );
    }
}

fn keyed(seed: u8) -> Vec<ActivityKey> {
    aiplane_core::server::crypto::Crypto::from_key([seed; 32]).activity_keys()
}

async fn row_of(pool: &Pool, key: &str, seq: i64) -> StoredEvent {
    let sql = format!("SELECT {COLUMNS} FROM agent_audit WHERE chain_key = ? AND seq = ?");
    stored(
        &sqlx::query(&sql)
            .bind(key)
            .bind(seq)
            .fetch_one(pool)
            .await
            .unwrap(),
    )
    .unwrap()
}

/// Rewrite event `seq` of chain `key` with `detail`, re-hashing it — and
/// every event after it, so the links stay whole — under `keys` (an
/// attacker's own key, or none), the way someone with only the database
/// would forge a consistent chain.
async fn forge(pool: &Pool, key: &str, seq: i64, detail: &str, keys: &[ActivityKey]) {
    let last: i64 = sqlx::query_scalar("SELECT MAX(seq) FROM agent_audit WHERE chain_key = ?")
        .bind(key)
        .fetch_one(pool)
        .await
        .unwrap();
    let mut prev = if seq > 1 {
        row_of(pool, key, seq - 1).await.hash
    } else {
        None
    };
    for n in seq..=last {
        let mut row = row_of(pool, key, n).await;
        if n == seq {
            row.detail = detail.to_string();
        }
        row.prev_hash = prev.clone();
        row.key_id = Some(keys.first().map_or(UNKEYED, |k| k.id.as_str()).to_string());
        let hash = row.expected_hash(keys).unwrap();
        sqlx::query(
            "UPDATE agent_audit SET detail = ?, prev_hash = ?, key_id = ?, hash = ? WHERE id = ?",
        )
        .bind(&row.detail)
        .bind(&row.prev_hash)
        .bind(&row.key_id)
        .bind(&hash)
        .bind(&row.id)
        .execute(pool)
        .await
        .unwrap();
        prev = Some(hash);
    }
}

#[tokio::test]
async fn a_keyed_chain_cannot_be_reforged_from_the_database_alone() {
    let pool = memory().await;
    install_key_ring(keyed(1));
    let chain = main_chain("s1");
    for n in 0..3 {
        run_event(&pool, &chain, AuditKind::ToolResult, json!({"n": n})).await;
    }
    let first = row_of(&pool, "conversation:s1", 1).await;
    assert_eq!(first.key_id.as_deref(), Some(keyed(1)[0].id.as_str()));
    assert_ne!(
        first.hash,
        Some(sha256_hex(first.canonical().as_bytes())),
        "an HMAC, not a plain digest"
    );
    assert!(verify(&pool, "p-main").await.unwrap().ok());

    forge(&pool, "conversation:s1", 2, r#"{"n":99}"#, &[]).await;
    let broken = verify(&pool, "p-main").await.unwrap().broken.unwrap();
    assert_eq!(broken.seq, 2);
    assert!(broken.reason.contains("not signed"), "{broken:?}");

    forge(&pool, "conversation:s1", 2, r#"{"n":98}"#, &keyed(9)).await;
    let broken = verify(&pool, "p-main").await.unwrap().broken.unwrap();
    assert_eq!(broken.seq, 2);
    assert!(broken.reason.contains("does not hold"), "{broken:?}");
}

#[tokio::test]
async fn events_signed_under_an_earlier_ring_key_still_verify() {
    let pool = memory().await;
    install_key_ring(keyed(1));
    run_event(&pool, &main_chain("s1"), AuditKind::ToolCall, json!({})).await;
    let mut ring = keyed(2);
    ring.extend(keyed(1));
    install_key_ring(ring);
    run_event(&pool, &main_chain("s1"), AuditKind::ToolCall, json!({})).await;
    let v = verify(&pool, "p-main").await.unwrap();
    assert!(v.ok(), "{v:?}");
    assert_ne!(
        row_of(&pool, "conversation:s1", 1).await.key_id,
        row_of(&pool, "conversation:s1", 2).await.key_id
    );
}

#[tokio::test]
async fn an_anchor_catches_a_cut_tail_and_a_deleted_conversation_chain() {
    let pool = memory().await;
    install_key_ring(keyed(1));
    for conversation in ["s-cut", "s-gone", "s-later"] {
        for n in 0..3 {
            run_event(
                &pool,
                &main_chain(conversation),
                AuditKind::ToolCall,
                json!({"n": n}),
            )
            .await;
        }
        anchor_conversation(&pool, "p-main", conversation)
            .await
            .unwrap()
            .expect("an anchor");
    }
    run_event(
        &pool,
        &main_chain("s-later"),
        AuditKind::ToolCall,
        json!({}),
    )
    .await;
    assert!(
        anchor_conversation(&pool, "p-main", "s-empty")
            .await
            .unwrap()
            .is_none()
    );
    let v = verify(&pool, "p-main").await.unwrap();
    assert!(v.ok(), "{v:?}");
    assert_eq!(v.unanchored, 1, "s-later's event after its anchor");
    let head = v.head.expect("the agent chain's head");
    assert_eq!((head.chain_key.as_str(), head.seq), ("agent:p-main", 3));
    assert_eq!(head.hash, row_of(&pool, "agent:p-main", 3).await.hash);

    sqlx::query("DELETE FROM agent_audit WHERE chain_key = 'conversation:s-cut' AND seq = 3")
        .execute(&pool)
        .await
        .unwrap();
    let broken = verify(&pool, "p-main").await.unwrap().broken.unwrap();
    assert_eq!(broken.chain_key, "conversation:s-cut");
    assert!(
        broken.reason.contains("newest events were removed"),
        "{broken:?}"
    );

    sqlx::query("DELETE FROM agent_audit WHERE chain_key = 'conversation:s-cut'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM agent_audit WHERE chain_key = 'conversation:s-gone'")
        .execute(&pool)
        .await
        .unwrap();
    let broken = verify(&pool, "p-main").await.unwrap().broken.unwrap();
    assert_eq!(broken.chain_key, "conversation:s-cut");
    assert!(broken.reason.contains("is gone"), "{broken:?}");
}

#[tokio::test]
async fn a_chain_the_sweep_removed_is_let_go_by_its_anchor() {
    let pool = memory().await;
    install_key_ring(keyed(1));
    run_event(&pool, &main_chain("s-old"), AuditKind::ToolCall, json!({})).await;
    anchor_conversation(&pool, "p-main", "s-old").await.unwrap();
    let swept = sweep_conversation_chains(
        &pool,
        "p-main",
        Timestamp::now() + jiff::SignedDuration::from_secs(60),
    )
    .await
    .unwrap();
    assert_eq!(swept.chains, 1);
    let v = verify(&pool, "p-main").await.unwrap();
    assert!(v.ok(), "{v:?}");
    let kinds: Vec<String> = for_principal(&pool, "p-main")
        .await
        .unwrap()
        .into_iter()
        .map(|e| e.kind)
        .collect();
    assert_eq!(kinds, ["activity_swept", "chain_checkpoint"]);
}

#[test]
fn canonical_json_sorts_keys_at_every_level_without_whitespace() {
    let v = json!({"b": {"z": 1, "a": [{"y": true, "x": null}]}, "a": "é"});
    assert_eq!(
        canonical_json(&v),
        "{\"a\":\"é\",\"b\":{\"a\":[{\"x\":null,\"y\":true}],\"z\":1}}"
    );
}

#[test]
fn the_insert_has_a_placeholder_per_column() {
    let columns = event_columns!().split(',').count();
    assert_eq!(INSERT_SQL.matches('?').count(), columns);
    assert_eq!(COLUMNS.split(',').count(), columns + 1, "rowid first");
}

#[test]
fn the_markers_are_what_the_agent_chain_says_about_other_chains() {
    let markers: Vec<AuditKind> = AuditKind::ALL
        .iter()
        .copied()
        .filter(|k| k.is_marker())
        .collect();
    assert_eq!(
        sql_kinds(markers),
        "'activity_swept', 'chain_anchored', 'chain_checkpoint'"
    );
    assert_eq!(sql_kinds([]), "");
}

async fn live_conversation(pool: &Pool, id: &str) {
    sqlx::query(
        "INSERT INTO users (id, email, created_at, updated_at)
         VALUES ('u1', 'u1@example.com', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')
         ON CONFLICT(id) DO NOTHING",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO chat_sessions (id, user_id, created_at, updated_at)
         VALUES (?, 'u1', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
    )
    .bind(id)
    .execute(pool)
    .await
    .unwrap();
}

async fn management_events(pool: &Pool, n: usize) {
    for n in 0..n {
        append_now(
            pool,
            NewEvent::new(AuditKind::GrantAdded, "p-main", json!({ "n": n })),
        )
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn the_sweep_cuts_the_agent_chains_old_prefix_behind_a_checkpoint_verify_starts_from() {
    let pool = memory().await;
    install_key_ring(keyed(1));
    live_conversation(&pool, "s-live").await;
    management_events(&pool, 20).await;
    for _ in 0..2 {
        run_event(&pool, &main_chain("s-live"), AuditKind::ToolCall, json!({})).await;
    }
    anchor_conversation(&pool, "p-main", "s-live")
        .await
        .unwrap();
    run_event(&pool, &main_chain("s-gone"), AuditKind::ToolCall, json!({})).await;
    anchor_conversation(&pool, "p-main", "s-gone")
        .await
        .unwrap();
    let cut = chain_rows(&pool, "agent:p-main").await.len();

    let later = Timestamp::now() + jiff::SignedDuration::from_secs(60);
    sweep_conversation_chains(&pool, "p-main", later)
        .await
        .unwrap();
    let own = chain_rows(&pool, "agent:p-main").await;
    let kinds: Vec<&str> = own.iter().map(|e| e.kind.as_str()).collect();
    assert_eq!(
        kinds,
        ["chain_checkpoint", "activity_swept"],
        "the old prefix is gone; the checkpoint and the sweep's marker remain"
    );
    let checkpoint: Value = serde_json::from_str(&own[0].detail).unwrap();
    assert_eq!(checkpoint["removed"], cut);
    assert_eq!(checkpoint["base_seq"], cut);
    assert_eq!(
        own[0].prev_hash,
        checkpoint["base_hash"].as_str().map(str::to_string)
    );
    let v = verify(&pool, "p-main").await.unwrap();
    assert!(v.ok(), "{v:?}");
    assert_eq!(
        v.unanchored, 0,
        "the live conversation's anchor was carried over"
    );

    sqlx::query("DELETE FROM agent_audit WHERE chain_key = 'conversation:s-live' AND seq = 2")
        .execute(&pool)
        .await
        .unwrap();
    let broken = verify(&pool, "p-main").await.unwrap().broken.unwrap();
    assert_eq!(broken.chain_key, "conversation:s-live");
    assert!(
        broken.reason.contains("newest events were removed"),
        "the carried anchor still guards the tail: {broken:?}"
    );
}

#[tokio::test]
async fn a_removed_checkpoint_after_a_cut_breaks_the_agent_chain() {
    let pool = memory().await;
    install_key_ring(keyed(1));
    management_events(&pool, 5).await;
    let later = Timestamp::now() + jiff::SignedDuration::from_secs(60);
    sweep_conversation_chains(&pool, "p-main", later)
        .await
        .unwrap();
    management_events(&pool, 2).await;
    assert!(verify(&pool, "p-main").await.unwrap().ok());
    let own = chain_rows(&pool, "agent:p-main").await;
    assert_eq!(own.len(), 3);

    sqlx::query("DELETE FROM agent_audit WHERE id = ?")
        .bind(&own[0].id)
        .execute(&pool)
        .await
        .unwrap();
    let broken = verify_full(&pool, "p-main").await.unwrap().broken.unwrap();
    assert_eq!(broken.chain_key, "agent:p-main");
    assert!(broken.reason.contains("missing"), "{broken:?}");
}

#[tokio::test]
async fn an_image_sent_in_every_round_is_stored_once_and_read_back_whole() {
    let pool = memory().await;
    install_key_ring(keyed(1));
    let image = format!(
        "data:image/png;base64,{}",
        "Q".repeat(exchange::BLOB_MIN_BYTES)
    );
    let chain = main_chain("s1");
    let first = json!({ "model": "m", "messages": [
        { "role": "system", "content": "s" },
        { "role": "user", "content": [{ "type": "image_url", "image_url": { "url": image } }] }
    ] });
    let mut second = first.clone();
    second["messages"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "role": "assistant", "content": "a cat" }));
    let round = |request: Value| json!({ "purpose": "round", "request": request });
    let a = run_event(&pool, &chain, AuditKind::LlmExchange, round(first.clone())).await;
    let mut delta = exchange::request_delta(&first, &second).unwrap();
    delta["prev"] = json!(a.id);
    run_event(
        &pool,
        &chain,
        AuditKind::LlmExchange,
        json!({ "purpose": "round", "request_delta": delta }),
    )
    .await;
    run_event(&pool, &chain, AuditKind::LlmExchange, round(first.clone())).await;

    let blobs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM activity_blobs")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(blobs, 1, "the image is stored once");
    let stored: Vec<String> = sqlx::query_scalar("SELECT detail FROM agent_audit")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert!(stored.iter().all(|d| !d.contains(&image)));

    let mut reconstructor = Reconstructor::default();
    let mut requests = Vec::new();
    for event in chain_rows(&pool, "conversation:s1").await {
        let json = reconstructor.event_json(&pool, &event).await.unwrap();
        assert!(json["detail"].get("request_delta").is_none());
        requests.push(json["detail"]["request"].clone());
    }
    assert_eq!(requests, [first.clone(), second, first]);
    assert!(verify(&pool, "p-main").await.unwrap().ok());

    sweep_conversation_chains(
        &pool,
        "p-main",
        Timestamp::now() + jiff::SignedDuration::from_secs(60),
    )
    .await
    .unwrap();
    let blobs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM activity_blobs")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(blobs, 0, "the chain's blobs go with it");
}

#[tokio::test]
async fn a_check_hashes_only_what_was_written_since_the_last_and_a_full_walk_still_finds_a_change_below_it()
 {
    let pool = memory().await;
    install_key_ring(keyed(1));
    for n in 0..4 {
        run_event(
            &pool,
            &main_chain("s1"),
            AuditKind::ToolCall,
            json!({"n": n}),
        )
        .await;
    }
    anchor_conversation(&pool, "p-main", "s1").await.unwrap();
    let first = verify(&pool, "p-main").await.unwrap();
    assert!(first.ok(), "{first:?}");
    assert_eq!((first.events, first.checked), (5, 5));

    let again = verify(&pool, "p-main").await.unwrap();
    assert_eq!((again.events, again.checked), (5, 0), "{again:?}");
    assert_eq!(again.head, first.head);

    run_event(
        &pool,
        &main_chain("s1"),
        AuditKind::ToolCall,
        json!({"n": 4}),
    )
    .await;
    let next = verify(&pool, "p-main").await.unwrap();
    assert_eq!(
        (next.events, next.checked, next.unanchored),
        (6, 1, 1),
        "{next:?}"
    );

    sqlx::query(
        "UPDATE agent_audit SET detail = '{\"n\":99}' WHERE chain_key = 'conversation:s1' AND seq = 2",
    )
    .execute(&pool)
    .await
    .unwrap();
    assert!(
        verify(&pool, "p-main").await.unwrap().ok(),
        "below the watermark only a full walk looks"
    );
    let broken = verify_full(&pool, "p-main").await.unwrap().broken.unwrap();
    assert_eq!(
        (broken.chain_key.as_str(), broken.seq),
        ("conversation:s1", 2)
    );
    assert!(
        broken.reason.contains("does not match its hash"),
        "{broken:?}"
    );
}

#[tokio::test]
async fn a_watermark_moved_in_the_database_is_not_trusted() {
    let pool = memory().await;
    install_key_ring(keyed(1));
    for n in 0..3 {
        run_event(
            &pool,
            &main_chain("s1"),
            AuditKind::ToolCall,
            json!({"n": n}),
        )
        .await;
    }
    assert!(verify(&pool, "p-main").await.unwrap().ok());
    for n in 3..5 {
        run_event(
            &pool,
            &main_chain("s1"),
            AuditKind::ToolCall,
            json!({"n": n}),
        )
        .await;
    }
    sqlx::query(
        "UPDATE agent_audit SET detail = '{\"n\":99}' WHERE chain_key = 'conversation:s1' AND seq = 4",
    )
    .execute(&pool)
    .await
    .unwrap();
    let head = row_of(&pool, "conversation:s1", 5).await;
    sqlx::query(
        "UPDATE activity_verified SET seq = 5, hash = ? WHERE chain_key = 'conversation:s1'",
    )
    .bind(&head.hash)
    .execute(&pool)
    .await
    .unwrap();

    let broken = verify(&pool, "p-main").await.unwrap().broken.unwrap();
    assert_eq!(broken.seq, 4);
    assert!(
        broken.reason.contains("does not match its hash"),
        "{broken:?}"
    );
}

#[tokio::test]
async fn the_sweep_lets_go_of_a_swept_chains_watermark() {
    let pool = memory().await;
    install_key_ring(keyed(1));
    run_event(&pool, &main_chain("s-old"), AuditKind::ToolCall, json!({})).await;
    anchor_conversation(&pool, "p-main", "s-old").await.unwrap();
    assert!(verify(&pool, "p-main").await.unwrap().ok());
    sweep_conversation_chains(
        &pool,
        "p-main",
        Timestamp::now() + jiff::SignedDuration::from_secs(60),
    )
    .await
    .unwrap();
    let marks: Vec<String> = sqlx::query_scalar("SELECT chain_key FROM activity_verified")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(marks, ["agent:p-main"]);
    assert!(verify(&pool, "p-main").await.unwrap().ok());
}
