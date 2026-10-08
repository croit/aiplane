// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Agents served over A2A (`docs/agent-a2a.md` → "Serving an agent over A2A"):
//! the agent card of an opted-in agent, and its JSON-RPC endpoint
//! (`SendMessage`, `SendStreamingMessage`, `GetTask`, `CancelTask`) for a
//! caller holding a `gws_` token whose principal is granted `a2a_caller` on
//! the agent. Every task runs through the production runner on a wiremock
//! upstream, exactly like an embed visitor's message.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use rama::Service;
use rama::http::{Body, HeaderMap, Method, Request, StatusCode, header};
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::agents::{self, Fx, TIME};
use crate::common;

use aiplane_agents::db::agent_audit;
use aiplane_agents::db::run_sessions;
use aiplane_runtime::agents::embed::LiveAgentRunner;
use aiplane_runtime::server::tools::ToolRegistry;
use aiplane_runtime::server::tools::ask_first::AskFirst;
use aiplane_runtime::server::tools::check_code::{CHECK_CODE, CheckCode};
use aiplane_runtime::server::tools::echo::Echo;
use aiplane_runtime::server::tools::time::CurrentTimestamp;

const ANSWER: &str = "Your order ships today.";
const CODE: &str = "481516";
const ECHO: &str = "company_echo";

/// A chat upstream answering each round with the next scripted delta,
/// optionally after a delay from round `slow_from` on.
struct Scripted {
    deltas: Vec<Value>,
    served: AtomicUsize,
    delay: Duration,
    slow_from: usize,
}

impl wiremock::Respond for Scripted {
    fn respond(&self, _req: &wiremock::Request) -> ResponseTemplate {
        let i = self.served.fetch_add(1, Ordering::SeqCst);
        let delta = &self.deltas[i.min(self.deltas.len() - 1)];
        let sse = format!(
            "data: {}\n\ndata: [DONE]\n\n",
            json!({"choices": [{"index": 0, "delta": delta}]})
        );
        let delay = if i >= self.slow_from {
            self.delay
        } else {
            Duration::ZERO
        };
        ResponseTemplate::new(200)
            .set_body_raw(sse, "text/event-stream")
            .set_delay(delay)
    }
}

async fn upstream_slow_from(deltas: Vec<Value>, slow_from: usize, delay: Duration) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(Scripted {
            deltas,
            served: AtomicUsize::new(0),
            delay,
            slow_from,
        })
        .mount(&server)
        .await;
    server
}

async fn upstream_with(deltas: Vec<Value>, delay: Duration) -> MockServer {
    upstream_slow_from(deltas, 0, delay).await
}

async fn upstream(deltas: Vec<Value>) -> MockServer {
    upstream_with(deltas, Duration::ZERO).await
}

fn text(s: &str) -> Value {
    json!({ "content": s })
}

fn call(id: &str, name: &str, args: Value) -> Value {
    json!({"tool_calls": [{"index": 0, "id": id, "type": "function",
        "function": {"name": name, "arguments": args.to_string()}}]})
}

async fn sent(server: &MockServer) -> Vec<Value> {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|r| serde_json::from_slice(&r.body).unwrap())
        .collect()
}

struct A2a {
    fx: Fx,
    agent: String,
    caller: String,
    token: String,
}

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Value,
}

impl Reply {
    fn result(&self) -> &Value {
        assert!(
            self.body.get("error").is_none(),
            "expected a result: {}",
            self.body
        );
        &self.body["result"]
    }

    fn error_code(&self) -> i64 {
        self.body["error"]["code"].as_i64().unwrap_or_default()
    }

    /// The `reason` of the error's `google.rpc.ErrorInfo` detail.
    fn reason(&self) -> &str {
        self.body["error"]["data"]
            .as_array()
            .and_then(|d| d.iter().find_map(|x| x["reason"].as_str()))
            .unwrap_or_default()
    }

    fn task(&self) -> &Value {
        &self.result()["task"]
    }
}

fn state_of(task: &Value) -> &str {
    task["status"]["state"].as_str().unwrap_or_default()
}

fn answer_of(task: &Value) -> &str {
    task["artifacts"][0]["parts"][0]["text"]
        .as_str()
        .unwrap_or_default()
}

fn message(text: &str) -> Value {
    json!({ "messageId": uuid::Uuid::new_v4().to_string(), "role": "ROLE_USER",
            "parts": [{ "text": text }] })
}

impl A2a {
    fn endpoint(&self) -> String {
        format!("/a2a/agents/{}", self.agent)
    }

    fn card_uri(&self) -> String {
        format!("/a2a/agents/{}/agent-card.json", self.agent)
    }

    async fn raw(&self, req: Request) -> rama::http::Response {
        common::app(self.fx.state.clone()).serve(req).await.unwrap()
    }

    async fn reply(&self, req: Request) -> Reply {
        let resp = self.raw(req).await;
        let status = resp.status();
        let headers = resp.headers().clone();
        let bytes = common::read_body(resp).await;
        Reply {
            status,
            headers,
            body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        }
    }

    fn request(&self, bearer: Option<&str>, version: Option<&str>, body: Value) -> Request {
        let mut req = Request::builder()
            .method(Method::POST)
            .uri(self.endpoint())
            .header("content-type", "application/json");
        if let Some(b) = bearer {
            req = req.header(header::AUTHORIZATION, format!("Bearer {b}"));
        }
        if let Some(v) = version {
            req = req.header("A2A-Version", v);
        }
        req.body(Body::from(body.to_string())).unwrap()
    }

    async fn rpc_as(&self, bearer: Option<&str>, method: &str, params: Value) -> Reply {
        let body = json!({ "jsonrpc": "2.0", "id": 7, "method": method, "params": params });
        self.reply(self.request(bearer, Some("1.0"), body)).await
    }

    async fn rpc(&self, method: &str, params: Value) -> Reply {
        self.rpc_as(Some(&self.token), method, params).await
    }

    async fn send(&self, text: &str) -> Reply {
        self.rpc("SendMessage", json!({ "message": message(text) }))
            .await
    }

    async fn send_in(&self, context: &str, text: &str) -> Reply {
        let mut m = message(text);
        m["contextId"] = json!(context);
        self.rpc("SendMessage", json!({ "message": m })).await
    }

    async fn get_task(&self, id: &str) -> Reply {
        self.rpc("GetTask", json!({ "id": id })).await
    }

    async fn card(&self) -> (StatusCode, Value) {
        let req = Request::builder()
            .method(Method::GET)
            .uri(self.card_uri())
            .body(Body::empty())
            .unwrap();
        let r = self.reply(req).await;
        (r.status, r.body)
    }

    /// Another system principal with its own token, granted `a2a_caller` on
    /// `agents` (none: a token for nothing).
    async fn other_caller(&self, name: &str, agents: &[&str]) -> String {
        caller(&self.fx, name, agents).await.1
    }

    async fn publish(&self, spec: Value) {
        let (status, body) = self.fx.put_draft(&self.fx.alice, &self.agent, spec).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let (status, body) = self.fx.publish(&self.fx.alice, &self.agent).await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
    }

    /// The A2A caller the newest audit row of `kind` names in its call chain.
    async fn caller_in_chain_of(&self, kind: &str) -> String {
        let audit = agent_audit::for_principal(&self.fx.state.db, &self.agent)
            .await
            .unwrap();
        let row = audit
            .iter()
            .find(|e| e.kind == kind)
            .unwrap_or_else(|| panic!("no `{kind}` row"));
        let chain = row.chain.as_ref().expect("a run event carries its chain");
        assert_eq!(chain["caller"]["protocol"], "a2a");
        chain["caller"]["principal_id"]
            .as_str()
            .unwrap()
            .to_string()
    }

    async fn wait_settled(&self, task: &str) -> Value {
        for _ in 0..500 {
            let r = self.get_task(task).await;
            if state_of(r.result()) != "TASK_STATE_WORKING" {
                return r.result().clone();
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("task {task} never settled");
    }
}

/// A system principal named `name` with a fresh `gws_` token, granted
/// `a2a_caller` on each of `agents`: `(principal id, token)`.
async fn caller(fx: &Fx, name: &str, agents: &[&str]) -> (String, String) {
    let (status, body) = fx
        .post(
            &fx.alice,
            "/api/v0/system-principals",
            json!({ "name": name }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let id = body["principal"]["id"].as_str().unwrap().to_string();
    for agent in agents {
        assert_eq!(
            fx.grant(&fx.alice, &id, "a2a_caller", agent).await,
            StatusCode::CREATED
        );
    }
    let (status, body) = fx
        .post(
            &fx.alice,
            &format!("/api/v0/system-principals/{id}/tokens"),
            json!({ "name": "a2a" }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    (id, body["plaintext"].as_str().unwrap().to_string())
}

fn spec_with(tools: &[&str], publish: Value) -> Value {
    let mut publish = publish;
    publish["a2a"] = json!({ "enabled": true });
    json!({ "main": {
        "model": "m",
        "instructions": { "orchestration": "Help the caller." },
        "tools": tools
    }, "publish": publish })
}

/// A published, A2A-enabled agent `support` with `publish` settings, on the
/// production runner over `llm`, and a caller granted on it.
async fn served_on(llm: &MockServer, publish: Value) -> A2a {
    let hour = Duration::from_secs(3600);
    let tools = ToolRegistry::new()
        .with(CurrentTimestamp)
        .with(AskFirst::new(Echo, hour))
        .with(CheckCode::new(CODE, hour));
    let mut fx = agents::fixture_with(Some(&llm.uri()), tools, &[TIME, CHECK_CODE, ECHO]).await;
    fx.state = fx
        .state
        .clone()
        .with_agent_runner(Arc::new(LiveAgentRunner));
    let agent = fx.runnable("support").await;
    for tool in [CHECK_CODE, ECHO] {
        assert_eq!(
            fx.grant(&fx.alice, &agent, "tool", tool).await,
            StatusCode::CREATED
        );
    }
    let (caller, token) = caller(&fx, "partner-bot", &[&agent]).await;
    let a = A2a {
        fx,
        agent,
        caller,
        token,
    };
    a.publish(spec_with(&[TIME, CHECK_CODE, ECHO], publish))
        .await;
    a
}

async fn served(llm: &MockServer) -> A2a {
    served_on(llm, json!({})).await
}

// ---------------------------------------------------------------------------
// The agent card

#[tokio::test]
async fn the_card_is_served_only_for_a_published_agent_that_opted_in() {
    let llm = upstream(vec![text(ANSWER)]).await;
    let a = served(&llm).await;

    let (status, card) = a.card().await;
    assert_eq!(status, StatusCode::OK, "{card}");
    assert_eq!(card["name"], "support");
    assert_eq!(card["version"], "1");
    let url = card["supportedInterfaces"][0]["url"].as_str().unwrap();
    assert!(url.ends_with(&a.endpoint()), "{url}");
    assert_eq!(card["supportedInterfaces"][0]["protocolBinding"], "JSONRPC");
    assert_eq!(card["supportedInterfaces"][0]["protocolVersion"], "1.0");
    assert_eq!(
        card["securitySchemes"]["aiplaneSystemToken"]["httpAuthSecurityScheme"]["scheme"],
        "Bearer"
    );
    assert_eq!(card["skills"][0]["id"], "support");

    let mut off = spec_with(&[TIME], json!({}));
    off["publish"]["a2a"]["enabled"] = json!(false);
    a.publish(off).await;
    let (status, _) = a.card().await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "opted out on the live version"
    );

    let other = a.fx.runnable("internal").await;
    let (status, body) = a.fx.publish(&a.fx.alice, &other).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    for uri in [
        format!("/a2a/agents/{other}/agent-card.json"),
        "/a2a/agents/nope/agent-card.json".to_string(),
    ] {
        let req = Request::builder().uri(&uri).body(Body::empty()).unwrap();
        assert_eq!(a.reply(req).await.status, StatusCode::NOT_FOUND, "{uri}");
    }
}

#[tokio::test]
async fn a_draft_that_opts_in_serves_nothing_until_it_is_published() {
    let llm = upstream(vec![text(ANSWER)]).await;
    let fx = agents::fixture_on(Some(&llm.uri())).await;
    let agent = fx.runnable("support").await;
    let (status, body) = fx
        .put_draft(&fx.alice, &agent, spec_with(&[TIME], json!({})))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let req = Request::builder()
        .uri(format!("/a2a/agents/{agent}/agent-card.json"))
        .body(Body::empty())
        .unwrap();
    let resp = common::app(fx.state.clone()).serve(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// Who may call

#[tokio::test]
async fn an_unauthenticated_or_unscoped_caller_is_refused_and_nothing_runs() {
    let llm = upstream(vec![text(ANSWER)]).await;
    let a = served(&llm).await;
    let params = json!({ "message": message("hello") });

    let r = a.rpc_as(None, "SendMessage", params.clone()).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED, "{}", r.body);
    assert_eq!(r.reason(), "UNAUTHENTICATED");
    assert!(
        r.headers
            .get(header::WWW_AUTHENTICATE)
            .is_some_and(|v| v.to_str().unwrap().starts_with("Bearer")),
        "{:?}",
        r.headers
    );
    let r = a
        .rpc_as(Some("gws_0000000000"), "SendMessage", params.clone())
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED, "{}", r.body);

    let unscoped = a.other_caller("ci-bot", &[]).await;
    let r = a
        .rpc_as(Some(&unscoped), "SendMessage", params.clone())
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{}", r.body);
    assert_eq!(r.reason(), "PERMISSION_DENIED");
    assert!(
        r.body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("a2a_caller"),
        "{}",
        r.body
    );

    let elsewhere_agent = a.fx.runnable("internal").await;
    let elsewhere = a.other_caller("other-bot", &[&elsewhere_agent]).await;
    let r = a.rpc_as(Some(&elsewhere), "SendMessage", params).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "a grant names one agent");

    assert!(
        sent(&llm).await.is_empty(),
        "no refused call reached a model"
    );
}

#[tokio::test]
async fn granting_a2a_caller_needs_a_write_share_on_the_agent() {
    let llm = upstream(vec![text(ANSWER)]).await;
    let a = served(&llm).await;
    let (status, body) =
        a.fx.post(
            &a.fx.bob,
            "/api/v0/system-principals",
            json!({ "name": "bobs-bot" }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let bot = body["principal"]["id"].as_str().unwrap().to_string();
    assert_eq!(
        a.fx.grant(&a.fx.bob, &bot, "a2a_caller", &a.agent).await,
        StatusCode::NOT_FOUND,
        "bob holds no share on the agent"
    );
    assert_eq!(
        a.fx.grant(&a.fx.bob, &bot, "a2a_caller", "no-such-agent")
            .await,
        StatusCode::NOT_FOUND
    );
    let (status, _) =
        a.fx.share(&a.fx.alice, &a.agent, "user", "bob", "write")
            .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        a.fx.grant(&a.fx.bob, &bot, "a2a_caller", &a.agent).await,
        StatusCode::CREATED
    );
}

/// The token's minter keeps a `write` share but loses the agent-management
/// permission: the share no longer counts, so the token no longer carries
/// the `a2a_caller` grant and nothing runs.
#[tokio::test]
async fn a_caller_whose_minter_lost_the_permission_is_refused_despite_the_share() {
    let llm = upstream(vec![text(ANSWER)]).await;
    let a = served(&llm).await;
    aiplane_core::server::db::gateway_groups::set_can_manage_agents(
        &a.fx.state.db,
        "managers",
        false,
    )
    .await
    .unwrap();
    a.fx.state.reload_rbac().await;

    let r = a.send("hello").await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{}", r.body);
    assert_eq!(r.reason(), "PERMISSION_DENIED");
    assert!(
        sent(&llm).await.is_empty(),
        "no refused call reached a model"
    );
}

#[tokio::test]
async fn the_protocol_version_and_the_envelope_are_checked() {
    let llm = upstream(vec![text(ANSWER)]).await;
    let a = served(&llm).await;
    let body = json!({ "jsonrpc": "2.0", "id": "x", "method": "GetTask",
                       "params": { "id": "t" } });

    let r = a.reply(a.request(Some(&a.token), None, body.clone())).await;
    assert_eq!(r.error_code(), -32009, "no header means 0.3: {}", r.body);
    assert_eq!(r.body["id"], "x");
    let r = a
        .reply(a.request(Some(&a.token), Some("0.3"), body.clone()))
        .await;
    assert_eq!(r.error_code(), -32009);

    let r = a.rpc("ListTasks", json!({})).await;
    assert_eq!(r.error_code(), -32004);
    let r = a.rpc("CreateTaskPushNotificationConfig", json!({})).await;
    assert_eq!(r.error_code(), -32003);
    let r = a.rpc("message/send", json!({})).await;
    assert_eq!(r.error_code(), -32601);

    let garbage = Request::builder()
        .method(Method::POST)
        .uri(a.endpoint())
        .header(header::AUTHORIZATION, format!("Bearer {}", a.token))
        .header("A2A-Version", "1.0")
        .body(Body::from("{not json"))
        .unwrap();
    assert_eq!(a.reply(garbage).await.error_code(), -32700);

    let mut url_part = message("look");
    url_part["parts"] = json!([{ "url": "https://x.example/a.png", "mediaType": "image/png" }]);
    let r = a.rpc("SendMessage", json!({ "message": url_part })).await;
    assert_eq!(r.error_code(), -32005, "{}", r.body);
    let mut agent_role = message("hi");
    agent_role["role"] = json!("ROLE_AGENT");
    let r = a.rpc("SendMessage", json!({ "message": agent_role })).await;
    assert_eq!(r.error_code(), -32602, "{}", r.body);
    assert!(sent(&llm).await.is_empty());
}

// ---------------------------------------------------------------------------
// Tasks

#[tokio::test]
async fn a_message_becomes_a_completed_task_with_the_agents_answer() {
    let llm = upstream(vec![text(ANSWER)]).await;
    let a = served(&llm).await;

    let r = a.send("where is my order?").await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["id"], 7);
    let task = r.task();
    assert_eq!(state_of(task), "TASK_STATE_COMPLETED", "{task}");
    assert_eq!(answer_of(task), ANSWER);
    let context = task["contextId"].as_str().unwrap();
    let history = task["history"].as_array().unwrap();
    assert_eq!(history[0]["role"], "ROLE_USER");
    assert_eq!(history[0]["parts"][0]["text"], "where is my order?");
    assert_eq!(history[1]["role"], "ROLE_AGENT");
    assert_eq!(history[1]["parts"][0]["text"], ANSWER);
    assert!(task["status"]["timestamp"].as_str().unwrap().ends_with('Z'));

    let conversation = run_sessions::get_principal_session(&a.fx.state.db, &a.agent, context)
        .await
        .unwrap()
        .expect("the context is a conversation owned by the agent");
    assert_eq!(conversation.agent_version, Some(1));
    let upstream_saw = sent(&llm).await;
    assert!(
        upstream_saw[0]["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("Help the caller."),
        "the agent's own instructions"
    );

    let audit = agent_audit::for_principal(&a.fx.state.db, &a.agent)
        .await
        .unwrap();
    let started = audit
        .iter()
        .find(|e| e.kind == "a2a_task")
        .expect("the task is audited");
    assert_eq!(started.detail["caller_id"], a.caller.as_str());
    assert_eq!(started.detail["context_id"], context);
    assert_eq!(started.detail["task_id"], task["id"]);
}

#[tokio::test]
async fn a_second_message_in_the_context_continues_the_conversation_as_a_new_task() {
    let llm = upstream(vec![text("First answer."), text("Second answer.")]).await;
    let a = served(&llm).await;
    let first = a.send("first question").await;
    let first = first.task().clone();
    let context = first["contextId"].as_str().unwrap();

    let second = a.send_in(context, "second question").await;
    let second = second.task();
    assert_eq!(state_of(second), "TASK_STATE_COMPLETED", "{second}");
    assert_eq!(second["contextId"], context);
    assert_ne!(second["id"], first["id"]);
    assert_eq!(answer_of(second), "Second answer.");

    let replayed: Vec<String> = sent(&llm).await[1]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| m["content"].as_str().map(str::to_string))
        .collect();
    assert!(
        replayed.iter().any(|c| c == "first question")
            && replayed.iter().any(|c| c == "First answer."),
        "the second turn sees the first: {replayed:?}"
    );

    let r = a.send_in("not-a-context", "hi").await;
    assert_eq!(r.error_code(), -32602, "{}", r.body);
    let mut mismatched = message("hi");
    mismatched["contextId"] = json!("another");
    mismatched["taskId"] = first["id"].clone();
    let r = a.rpc("SendMessage", json!({ "message": mismatched })).await;
    assert_eq!(r.error_code(), -32602, "{}", r.body);
    let mut done = message("more");
    done["taskId"] = first["id"].clone();
    let r = a.rpc("SendMessage", json!({ "message": done })).await;
    assert_eq!(r.error_code(), -32004, "a completed task takes no message");
}

#[tokio::test]
async fn get_task_returns_the_task_to_its_caller_only() {
    let llm = upstream(vec![text(ANSWER)]).await;
    let a = served(&llm).await;
    let sent_task = a.send("hello").await.task().clone();
    let id = sent_task["id"].as_str().unwrap();

    let got = a.get_task(id).await;
    assert_eq!(got.result(), &sent_task, "GetTask answers the Task itself");
    let short = a
        .rpc("GetTask", json!({ "id": id, "historyLength": 0 }))
        .await;
    assert!(short.result().get("history").is_none(), "{}", short.body);
    let one = a
        .rpc("GetTask", json!({ "id": id, "historyLength": 1 }))
        .await;
    assert_eq!(one.result()["history"].as_array().unwrap().len(), 1);
    assert_eq!(one.result()["history"][0]["role"], "ROLE_AGENT");

    let colleague = a.other_caller("colleague-bot", &[&a.agent]).await;
    let theirs = a
        .rpc_as(Some(&colleague), "GetTask", json!({ "id": id }))
        .await;
    assert_eq!(theirs.error_code(), -32001, "{}", theirs.body);
    let unknown = a.get_task("no-such-task").await;
    assert_eq!(unknown.error_code(), -32001);
}

#[tokio::test]
async fn a_running_task_can_be_cancelled_and_a_finished_one_cannot() {
    let llm = upstream_with(vec![text(ANSWER)], Duration::from_secs(3)).await;
    let a = served(&llm).await;
    let mut params = json!({ "message": message("take your time") });
    params["configuration"] = json!({ "returnImmediately": true });
    let r = a.rpc("SendMessage", params).await;
    let task = r.task();
    assert_eq!(state_of(task), "TASK_STATE_WORKING", "{task}");
    let id = task["id"].as_str().unwrap().to_string();

    let cancelled = a.rpc("CancelTask", json!({ "id": id })).await;
    assert_eq!(
        state_of(cancelled.result()),
        "TASK_STATE_CANCELED",
        "{}",
        cancelled.body
    );
    let again = a.rpc("CancelTask", json!({ "id": id })).await;
    assert_eq!(again.error_code(), -32002, "{}", again.body);

    let llm2 = upstream(vec![text(ANSWER)]).await;
    let b = served(&llm2).await;
    let done = b.send("quick").await.task().clone();
    let r = b.rpc("CancelTask", json!({ "id": done["id"] })).await;
    assert_eq!(r.error_code(), -32002);
}

/// A context whose first task `A` completed and whose second task `B` is
/// still running: `(served agent, A, B)`.
async fn a_finished_task_beside_a_running_one() -> (A2a, MockServer, String, String) {
    let llm = upstream_slow_from(
        vec![text("First answer."), text("Second answer.")],
        1,
        Duration::from_secs(2),
    )
    .await;
    let a = served(&llm).await;
    let first = a.send("first question").await.task().clone();
    assert_eq!(state_of(&first), "TASK_STATE_COMPLETED", "{first}");
    let mut m = message("second question");
    m["contextId"] = first["contextId"].clone();
    let r = a
        .rpc(
            "SendMessage",
            json!({ "message": m, "configuration": { "returnImmediately": true } }),
        )
        .await;
    let second = r.task();
    assert_eq!(state_of(second), "TASK_STATE_WORKING", "{second}");
    let ids = (
        first["id"].as_str().unwrap().to_string(),
        second["id"].as_str().unwrap().to_string(),
    );
    (a, llm, ids.0, ids.1)
}

#[tokio::test]
async fn get_task_reports_a_finished_task_as_finished_while_its_context_runs_another() {
    let (a, _llm, done, running) = a_finished_task_beside_a_running_one().await;
    let r = a.get_task(&done).await;
    assert_eq!(state_of(r.result()), "TASK_STATE_COMPLETED", "{}", r.body);
    assert_eq!(answer_of(r.result()), "First answer.");
    let r = a.get_task(&running).await;
    assert_eq!(state_of(r.result()), "TASK_STATE_WORKING", "{}", r.body);
}

#[tokio::test]
async fn cancelling_a_finished_task_leaves_the_running_task_of_its_context_alone() {
    let (a, _llm, done, running) = a_finished_task_beside_a_running_one().await;
    let r = a.rpc("CancelTask", json!({ "id": done })).await;
    assert_eq!(r.error_code(), -32002, "{}", r.body);
    let after = a.wait_settled(&running).await;
    assert_eq!(state_of(&after), "TASK_STATE_COMPLETED", "{after}");
    assert_eq!(answer_of(&after), "Second answer.");
}

#[tokio::test]
async fn subscribing_to_a_finished_task_is_refused_while_its_context_runs_another() {
    let (a, _llm, done, _running) = a_finished_task_beside_a_running_one().await;
    let r = a.rpc("SubscribeToTask", json!({ "id": done })).await;
    assert_eq!(r.error_code(), -32004, "{}", r.body);
}

#[tokio::test]
async fn a_message_for_a_finished_task_is_refused_as_finished_while_its_context_runs_another() {
    let (a, _llm, done, _running) = a_finished_task_beside_a_running_one().await;
    let mut more = message("more");
    more["taskId"] = json!(done);
    let r = a.rpc("SendMessage", json!({ "message": more })).await;
    assert_eq!(r.error_code(), -32004, "{}", r.body);
}

#[tokio::test]
async fn the_output_filter_applies_to_an_a2a_answer() {
    let llm = upstream(vec![text("Your invoice RE-424242 is due.")]).await;
    let a = served_on(
        &llm,
        json!({ "output_filter": { "patterns": { "invoice": "RE-\\d{6}" } } }),
    )
    .await;
    let r = a.send("what do I owe?").await;
    let task = r.task();
    assert_eq!(state_of(task), "TASK_STATE_COMPLETED", "{task}");
    assert!(!r.body.to_string().contains("RE-424242"), "{}", r.body);
    assert!(!answer_of(task).is_empty());
    let kinds = a.fx.audit_kinds(&a.agent).await;
    assert!(kinds.contains(&"output_blocked".to_string()), "{kinds:?}");
    assert_eq!(a.caller_in_chain_of("output_blocked").await, a.caller);
}

#[tokio::test]
async fn a_rate_or_budget_refusal_is_a_json_rpc_error_and_runs_nothing() {
    let llm = upstream(vec![text(ANSWER)]).await;
    let a = served_on(
        &llm,
        json!({ "rate_limits": { "visitor": { "max": 1, "per": "10m" } } }),
    )
    .await;
    let first = a.send("one").await.task().clone();
    let context = first["contextId"].as_str().unwrap();
    let r = a.send_in(context, "two").await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.error_code(), -32000, "{}", r.body);
    assert_eq!(r.reason(), "RATE_LIMITED");
    assert!(r.headers.get(header::RETRY_AFTER).is_some());
    assert_eq!(sent(&llm).await.len(), 1, "the refused message ran nothing");

    let llm2 = upstream(vec![text(ANSWER)]).await;
    let b = served_on(&llm2, json!({ "budget": { "monthly_tokens": 1000 } })).await;
    spend(&b, 5000).await;
    let r = b.send("anything").await;
    assert_eq!(r.error_code(), -32000, "{}", r.body);
    assert_eq!(r.reason(), "AGENT_UNAVAILABLE");
    assert!(!r.body.to_string().contains("budget"), "{}", r.body);
    assert!(sent(&llm2).await.is_empty());
    let kinds = b.fx.audit_kinds(&b.agent).await;
    assert!(kinds.contains(&"limit_refused".to_string()), "{kinds:?}");
}

async fn spend(a: &A2a, tokens: i64) {
    use aiplane_core::server::db::usage::{self, UsageKind, UsageRecord, UsageSource};
    use aiplane_core::server::principal::PrincipalKind;
    usage::insert_batch(
        &a.fx.state.db,
        &[UsageRecord {
            created_at: jiff::Timestamp::now(),
            user_id: a.agent.clone(),
            user_email: Some("support".into()),
            token_id: None,
            token_name: None,
            source: UsageSource::Agent,
            kind: UsageKind::Chat,
            backend: "mock".into(),
            model: "m".into(),
            status: 200,
            duration_ms: 1,
            prompt_tokens: Some(tokens),
            completion_tokens: Some(0),
            total_tokens: Some(tokens),
            input_units: None,
            output_units: None,
            enforce_limits: true,
            principal_kind: PrincipalKind::System,
            agent_id: Some(a.agent.clone()),
            chain: None,
            stop_reason: None,
        }],
    )
    .await
    .unwrap();
}

// ---------------------------------------------------------------------------
// Suspended tasks

#[tokio::test]
async fn a_secure_input_is_input_required_and_the_caller_answers_it_on_the_task() {
    let llm = upstream(vec![
        call("c1", CHECK_CODE, json!({})),
        text("Thanks, you are verified."),
    ])
    .await;
    let a = served(&llm).await;
    let r = a.send("verify me").await;
    let task = r.task().clone();
    assert_eq!(state_of(&task), "TASK_STATE_INPUT_REQUIRED", "{task}");
    assert_eq!(task["metadata"]["aiplane"]["answeredBy"], "caller");
    assert_eq!(task["metadata"]["aiplane"]["kind"], "secure_input");
    assert_eq!(task["status"]["message"]["role"], "ROLE_AGENT");

    let mut answer = message(CODE);
    answer["taskId"] = task["id"].clone();
    answer["contextId"] = task["contextId"].clone();
    let r = a.rpc("SendMessage", json!({ "message": answer })).await;
    let resumed = r.task();
    assert_eq!(resumed["id"], task["id"], "the same task continues");
    assert_eq!(state_of(resumed), "TASK_STATE_COMPLETED", "{resumed}");
    assert_eq!(answer_of(resumed), "Thanks, you are verified.");
    assert!(
        !r.body.to_string().contains(CODE),
        "the code is not in the task: {}",
        r.body
    );
    let to_model = serde_json::to_string(&sent(&llm).await).unwrap();
    assert!(!to_model.contains(CODE), "the model never saw the code");
    assert_eq!(
        a.caller_in_chain_of("run_resumed").await,
        a.caller,
        "the resumed run still names its caller"
    );
}

#[tokio::test]
async fn an_approval_waits_for_staff_and_the_caller_cannot_give_it() {
    let llm = upstream(vec![
        call("c1", ECHO, json!({ "text": "refund" })),
        text("Done."),
    ])
    .await;
    let a = served(&llm).await;
    let task = a.send("refund me").await.task().clone();
    assert_eq!(state_of(&task), "TASK_STATE_INPUT_REQUIRED", "{task}");
    assert_eq!(task["metadata"]["aiplane"]["answeredBy"], "staff");

    let mut answer = message("yes, approve");
    answer["taskId"] = task["id"].clone();
    let r = a.rpc("SendMessage", json!({ "message": answer })).await;
    assert_eq!(r.error_code(), -32000, "{}", r.body);
    assert_eq!(r.reason(), "DECISION_FOR_STAFF");

    let cancelled = a.rpc("CancelTask", json!({ "id": task["id"] })).await;
    assert_eq!(
        state_of(cancelled.result()),
        "TASK_STATE_CANCELED",
        "a caller may give up on a task waiting for staff: {}",
        cancelled.body
    );
    let after = a.wait_settled(task["id"].as_str().unwrap()).await;
    assert_eq!(state_of(&after), "TASK_STATE_CANCELED");
}

// ---------------------------------------------------------------------------
// Streaming

/// The `result` of every `data:` frame of an SSE body.
fn stream_results(body: &[u8]) -> Vec<Value> {
    String::from_utf8_lossy(body)
        .split("\n\n")
        .filter_map(|block| {
            block
                .lines()
                .find_map(|l| l.strip_prefix("data: "))
                .and_then(|d| serde_json::from_str::<Value>(d).ok())
        })
        .map(|frame| frame["result"].clone())
        .collect()
}

#[tokio::test]
async fn a_streamed_message_is_the_task_then_its_whole_answer_then_its_final_status() {
    let llm = upstream(vec![text(ANSWER)]).await;
    let a = served(&llm).await;
    let body = json!({ "jsonrpc": "2.0", "id": 9, "method": "SendStreamingMessage",
                       "params": { "message": message("stream please") } });
    let resp = a.raw(a.request(Some(&a.token), Some("1.0"), body)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers().get(header::CONTENT_TYPE).unwrap(),
        "text/event-stream"
    );
    let bytes = tokio::time::timeout(Duration::from_secs(20), common::read_body(resp))
        .await
        .expect("the stream closes once the task is done");
    let results = stream_results(&bytes);
    assert_eq!(results.len(), 3, "{results:?}");
    assert_eq!(state_of(&results[0]["task"]), "TASK_STATE_WORKING");
    let task_id = results[0]["task"]["id"].clone();
    assert_eq!(results[1]["artifactUpdate"]["taskId"], task_id);
    assert_eq!(
        results[1]["artifactUpdate"]["artifact"]["parts"][0]["text"], ANSWER,
        "one whole answer, not a token stream"
    );
    assert_eq!(results[1]["artifactUpdate"]["lastChunk"], true);
    assert_eq!(
        results[2]["statusUpdate"]["status"]["state"],
        "TASK_STATE_COMPLETED"
    );
    assert_eq!(
        results[2]["statusUpdate"]["contextId"],
        results[0]["task"]["contextId"]
    );
}

// ---------------------------------------------------------------------------
// The request body cap

#[tokio::test]
async fn a_request_body_over_the_cap_is_refused_without_buffering_it() {
    let llm = upstream(vec![text(ANSWER)]).await;
    let a = served(&llm).await;
    let request = |body: Body| {
        Request::builder()
            .method(Method::POST)
            .uri(a.endpoint())
            .header(header::AUTHORIZATION, format!("Bearer {}", a.token))
            .header("A2A-Version", "1.0")
            .header("content-type", "application/json")
            .body(body)
            .unwrap()
    };
    let declared = Body::from(vec![b' '; 1024 * 1024 + 1]);
    for body in [declared, common::endless_body()] {
        let resp = common::serve_promptly(&a.fx.state, request(body)).await;
        let r = Reply {
            status: resp.status(),
            headers: resp.headers().clone(),
            body: serde_json::from_slice(&common::read_body(resp).await).unwrap(),
        };
        assert_eq!(r.status, StatusCode::PAYLOAD_TOO_LARGE, "{}", r.body);
        assert_eq!(r.reason(), "PAYLOAD_TOO_LARGE", "{}", r.body);
        assert!(
            r.body["error"]["message"]
                .as_str()
                .unwrap()
                .contains("1 MiB"),
            "{}",
            r.body
        );
    }
    assert!(sent(&llm).await.is_empty());
}
