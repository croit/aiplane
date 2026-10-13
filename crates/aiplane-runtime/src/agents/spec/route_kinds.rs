// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Route targets beyond a sub-agent and a person (`docs/agent-a2a.md` → "External agents as route targets"). A target is the one key of a route that is not `when`,
//! `description`, `task` or `bind`; everything it needs lives under that
//! key, so a builder that does not know a kind still finds the route's
//! shared keys where they always are.
//!
//! - `a2a`: an external agent behind an A2A agent card.
//! - `loop`: a worker and a critic, draft, critique, revise
//!   (`docs/agent-runs.md` → "Loop routes").

use std::collections::BTreeSet;

use aiplane_core::server::principal::GrantKind;
use serde_json::{Map, Value};

use super::{Check, Stage, join, route_values_taken};
use crate::agents::a2a_client::{self as client, AuthKind};
use crate::agents::router::loop_route as loops;
use crate::finish::FinishContract;

const A2A_KEYS: &[&str] = &["card_url", "auth", "finish", "budget"];
const A2A_BUDGET_KEYS: &[&str] = &["seconds"];
const FINISH_KEYS: &[&str] = &["schema"];
const AUTH_COMMON: &[&str] = &["kind", "scheme"];
const AUTH_TOKEN: &[&str] = &["token", "token_sealed"];
const AUTH_CLIENT: &[&str] = &[
    "client_id",
    "client_secret",
    "client_secret_sealed",
    "scopes",
];

impl Check<'_> {
    /// The route's `task` (required) and `bind`, shared by every target that
    /// receives a task.
    pub(super) fn task_and_bind(&mut self, map: &Map<String, Value>, path: &str, what: &str) {
        match map.get("task") {
            Some(task) => self.template(task, &join(path, "task")),
            None => self.issue(
                &join(path, "task"),
                format!(
                    "a route to {what} needs a `task` — it receives this, never the transcript"
                ),
            ),
        }
        if let Some(bind) = map.get("bind") {
            self.bind(bind, &join(path, "bind"), false);
        }
    }

    /// A route's result contract, at `path` (`…a2a.finish`).
    fn route_finish(&mut self, v: Option<&Value>, path: &str, why: &str) {
        let Some(v) = v else {
            if self.stage == Stage::Publish {
                self.issue(
                    &join(path, "schema"),
                    format!("{why} needs `finish.schema`, the shape its result must have"),
                );
            }
            return;
        };
        let Some(map) = self.object(v, path, FINISH_KEYS) else {
            return;
        };
        let at = join(path, "schema");
        match map.get("schema") {
            None => self.issue(&at, "`finish` needs a `schema` for the result"),
            Some(schema) => {
                if let Err(err) = FinishContract::new(schema.clone()) {
                    let at = if err.path.is_empty() || err.path == "/" {
                        at
                    } else {
                        format!("{at}{}", err.path)
                    };
                    self.issue(&at, err.to_string());
                }
            }
        }
    }

    pub(super) fn a2a_route(&mut self, map: &Map<String, Value>, path: &str) {
        self.task_and_bind(map, path, "an external agent");
        let at = join(path, "a2a");
        let Some(a2a) = self.object(&map["a2a"], &at, A2A_KEYS) else {
            return;
        };
        match a2a.get("card_url") {
            None => self.issue(
                &join(&at, "card_url"),
                "an A2A route needs `card_url`, the agent card's URL (usually \
                 https://<host>/.well-known/agent-card.json)",
            ),
            Some(v) => {
                let p = join(&at, "card_url");
                if let Some(url) = self.string(v, &p) {
                    match client::check_card_url(url, self.ctx.allow_private) {
                        Err(why) => self.issue(&p, format!("`{url}` cannot be used: {why}")),
                        Ok(_) => self.require_grant(&p, GrantKind::A2aAgent, url, "A2A agent"),
                    }
                }
            }
        }
        if let Some(auth) = a2a.get("auth") {
            self.a2a_auth(auth, &join(&at, "auth"));
        }
        self.route_finish(a2a.get("finish"), &join(&at, "finish"), "an A2A route");
        if let Some(budget) = a2a.get("budget")
            && let Some(b) = self.object(budget, &join(&at, "budget"), A2A_BUDGET_KEYS)
            && let Some(s) = b.get("seconds")
        {
            self.positive_int(s, &join(&at, "budget.seconds"), Some(client::MAX_SECONDS));
        }
    }

    fn a2a_auth(&mut self, v: &Value, path: &str) {
        let kind = v
            .get("kind")
            .and_then(Value::as_str)
            .and_then(AuthKind::parse);
        let mut allowed = AUTH_COMMON.to_vec();
        match kind {
            Some(AuthKind::Bearer | AuthKind::ApiKey) => allowed.extend_from_slice(AUTH_TOKEN),
            Some(AuthKind::ClientCredentials) => allowed.extend_from_slice(AUTH_CLIENT),
            None => {
                allowed.extend_from_slice(AUTH_TOKEN);
                allowed.extend_from_slice(AUTH_CLIENT);
            }
        }
        let Some(map) = self.object(v, path, &allowed) else {
            return;
        };
        match map.get("kind") {
            Some(k) => {
                self.one_of(k, &join(path, "kind"), AuthKind::NAMES);
            }
            None => self.issue(
                &join(path, "kind"),
                format!(
                    "`auth` needs a `kind`: {}, matching one of the agent card's securitySchemes",
                    AuthKind::NAMES.join(", ")
                ),
            ),
        }
        if let Some(s) = map.get("scheme") {
            self.string(s, &join(path, "scheme"));
        }
        let Some(kind) = kind else {
            return;
        };
        let secret = kind.secret_key();
        let sealed = format!("{secret}_sealed");
        match (map.get(secret), map.get(&sealed)) {
            (Some(_), Some(_)) => self.issue(
                path,
                format!("give either a new `{secret}` or keep the stored `{sealed}`, not both"),
            ),
            (None, None) => self.issue(
                &join(path, secret),
                format!(
                    "`{}` auth needs a `{secret}`; it is sealed when the spec is saved",
                    kind.as_str()
                ),
            ),
            (Some(x), None) => {
                if self
                    .string(x, &join(path, secret))
                    .is_some_and(|s| s.trim().is_empty())
                {
                    self.issue(&join(path, secret), format!("`{secret}` is empty"));
                }
            }
            (None, Some(x)) => {
                self.string(x, &join(path, &sealed));
            }
        }
        if kind == AuthKind::ClientCredentials {
            match map.get("client_id") {
                Some(x) => {
                    self.string(x, &join(path, "client_id"));
                }
                None => self.issue(
                    &join(path, "client_id"),
                    "OAuth client credentials need a `client_id`",
                ),
            }
            if let Some(scopes) = map.get("scopes") {
                self.string_list(scopes, &join(path, "scopes"));
            }
        }
    }
}

const LOOP_KEYS: &[&str] = &["worker", "critic", "max_iterations", "budget"];
const LOOP_BUDGET_KEYS: &[&str] = &["seconds", "tokens"];

impl Check<'_> {
    pub(super) fn loop_route(&mut self, map: &Map<String, Value>, path: &str) {
        self.task_and_bind(map, path, "a loop");
        let at = join(path, "loop");
        let Some(target) = self.object(&map["loop"], &at, LOOP_KEYS) else {
            return;
        };
        let mut ids: Vec<&str> = Vec::new();
        for (role, what) in [
            ("worker", "agent that drafts the result"),
            ("critic", "agent that reviews it"),
        ] {
            let p = join(&at, role);
            match target.get(role) {
                None => self.issue(&p, format!("a loop needs a `{role}`: the id of the {what}")),
                Some(v) => {
                    self.sub_agent(v, &p);
                    if let Some(id) = v.as_str() {
                        ids.push(id);
                    }
                }
            }
        }
        if let [worker, critic] = ids[..] {
            if worker == critic {
                self.issue(
                    &join(&at, "critic"),
                    "the critic must be another agent than the worker — an agent does not \
                     review its own result",
                );
            }
            self.loop_contract(worker, critic, map.get("bind"), path);
        }
        if let Some(n) = target.get("max_iterations") {
            self.positive_int(n, &join(&at, "max_iterations"), Some(loops::MAX_ITERATIONS));
        }
        if let Some(budget) = target.get("budget")
            && let Some(b) = self.object(budget, &join(&at, "budget"), LOOP_BUDGET_KEYS)
        {
            if let Some(s) = b.get("seconds") {
                self.positive_int(s, &join(&at, "budget.seconds"), Some(client::MAX_SECONDS));
            }
            if let Some(t) = b.get("tokens") {
                self.positive_int(t, &join(&at, "budget.tokens"), None);
            }
        }
    }

    /// What a loop and its live worker and critic must agree on: the route
    /// passes exactly the `route.<name>` values either binds, both say what
    /// they return, and the critic's result says whether it accepts.
    fn loop_contract(&mut self, worker: &str, critic: &str, bind: Option<&Value>, path: &str) {
        let at = join(path, "loop");
        let live = self.ctx.live_specs;
        let (Some(w), Some(c)) = (live.get(worker), live.get(critic)) else {
            return;
        };
        let taken: BTreeSet<String> = route_values_taken(w)
            .into_iter()
            .chain(route_values_taken(c))
            .collect();
        let passed: BTreeSet<String> = bind
            .and_then(Value::as_object)
            .into_iter()
            .flat_map(|m| m.keys().cloned())
            .collect();
        for name in taken.difference(&passed) {
            self.issue(
                &join(path, "bind"),
                format!(
                    "the loop's worker or critic binds `route.{name}`, which this route does not \
                     pass — add it to this route's `bind` with a `state.<slot>` source"
                ),
            );
        }
        for name in passed.difference(&taken) {
            self.issue(
                &join(&join(path, "bind"), name),
                format!(
                    "neither the worker nor the critic binds `route.{name}`, so this value would \
                     reach no tool — remove it here"
                ),
            );
        }
        if self.stage == Stage::Publish && w.pointer("/finish/schema").is_none() {
            self.issue(
                &join(&at, "worker"),
                format!(
                    "worker `{worker}` declares no `finish` schema, and a loop returns the \
                     worker's result — add `finish.schema` to it and publish it again"
                ),
            );
        }
        let schema = c.pointer("/finish/schema");
        let accepts = schema.is_some_and(|s| {
            s.pointer(&format!("/properties/{}/type", loops::ACCEPTED))
                == Some(&Value::from("boolean"))
                && s.get("required")
                    .and_then(Value::as_array)
                    .is_some_and(|r| r.iter().any(|f| f == loops::ACCEPTED))
        });
        if !accepts {
            self.issue(
                &join(&at, "critic"),
                format!(
                    "critic `{critic}` must say whether it accepts: its `finish.schema` needs a \
                     required boolean `{accepted}` (and may add a string `{feedback}` for the \
                     worker) — add {{\"{accepted}\": {{\"type\": \"boolean\"}}}} to its \
                     `properties` and `required`, then publish it again",
                    accepted = loops::ACCEPTED,
                    feedback = loops::FEEDBACK,
                ),
            );
        } else if let Some(t) =
            schema.and_then(|s| s.pointer(&format!("/properties/{}/type", loops::FEEDBACK)))
            && t != "string"
        {
            self.issue(
                &join(&at, "critic"),
                format!(
                    "critic `{critic}`'s `{}` must be a string: the worker reads it as text",
                    loops::FEEDBACK
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use aiplane_core::server::principal::GrantSet;
    use serde_json::json;

    use super::super::{SpecContext, SpecIssue, validate};
    use super::*;

    const CARD: &str = "https://partner.example.com/.well-known/agent-card.json";

    fn check_with(spec: Value, stage: Stage, live: &HashMap<String, Value>) -> Vec<SpecIssue> {
        let grants = GrantSet::new([
            (GrantKind::Model, "chat".to_string()),
            (GrantKind::A2aAgent, CARD.to_string()),
        ]);
        let agents: HashMap<String, bool> = live.keys().map(|k| (k.clone(), true)).collect();
        validate(
            &spec,
            &SpecContext {
                action_catalogues: &Default::default(),
                agent_id: "self",
                grants: &grants,
                agents: &agents,
                live_specs: live,
                model_defaults: &Default::default(),
                speech_voices: &HashMap::new(),
                allow_private: false,
            },
            stage,
        )
    }

    fn check(spec: Value, stage: Stage) -> Vec<SpecIssue> {
        check_with(spec, stage, &HashMap::new())
    }

    fn paths(issues: &[SpecIssue]) -> Vec<&str> {
        issues.iter().map(|i| i.path.as_str()).collect()
    }

    fn with_route(route: Value) -> Value {
        json!({
            "main": { "model": "chat", "instructions": { "orchestration": "Help." } },
            "state": {
                "issue": { "type": "string", "set_by": ["llm"] },
                "verified": { "type": "subject", "set_by": ["host"] }
            },
            "routes": { "partner": route }
        })
    }

    fn a2a_route(a2a: Value) -> Value {
        with_route(json!({
            "when": { "slot": "verified", "provenance": "host" },
            "task": "Warranty question: {issue}",
            "bind": { "customer": "state.verified.customer_id" },
            "a2a": a2a
        }))
    }

    fn schema() -> Value {
        json!({ "type": "object", "required": ["answer"],
                "properties": { "answer": { "type": "string" } } })
    }

    #[test]
    fn a_granted_a2a_route_with_auth_and_a_finish_schema_is_publishable() {
        for auth in [
            json!({ "kind": "bearer", "token": "t0ken" }),
            json!({ "kind": "api_key", "scheme": "key", "token_sealed": "sealed" }),
            json!({ "kind": "oauth_client_credentials", "client_id": "gw",
                    "client_secret": "s3cret", "scopes": ["tasks"] }),
        ] {
            let spec = a2a_route(json!({
                "card_url": CARD, "auth": auth,
                "finish": { "schema": schema() }, "budget": { "seconds": 60 }
            }));
            assert_eq!(check(spec, Stage::Publish), [], "{auth}");
        }
    }

    #[test]
    fn an_a2a_route_says_what_is_wrong_with_each_setting() {
        let spec = a2a_route(json!({
            "card_url": "http://partner.example.com/card",
            "auth": { "kind": "basic", "token": "x" },
            "finish": { "schema": { "type": "object", "minProperties": 1 } },
            "budget": { "seconds": 5000 },
            "endpoint": "https://elsewhere"
        }));
        assert_eq!(
            paths(&check(spec, Stage::Draft)),
            [
                "routes.partner.a2a.endpoint",
                "routes.partner.a2a.card_url",
                "routes.partner.a2a.auth.kind",
                "routes.partner.a2a.finish.schema",
                "routes.partner.a2a.budget.seconds",
            ]
        );
    }

    #[test]
    fn an_ungranted_card_names_the_grant_to_make() {
        let other = "https://other.example.com/.well-known/agent-card.json";
        let issues = check(
            a2a_route(json!({ "card_url": other, "finish": { "schema": schema() } })),
            Stage::Draft,
        );
        assert_eq!(paths(&issues), ["routes.partner.a2a.card_url"]);
        assert!(
            issues[0].message.contains("\"kind\": \"a2a_agent\""),
            "{}",
            issues[0].message
        );
    }

    #[test]
    fn credentials_and_a_finish_schema_are_required_where_they_apply() {
        let spec = a2a_route(json!({
            "card_url": CARD,
            "auth": { "kind": "oauth_client_credentials", "client_secret": "s",
                      "client_secret_sealed": "x" }
        }));
        assert_eq!(
            paths(&check(spec.clone(), Stage::Draft)),
            [
                "routes.partner.a2a.auth",
                "routes.partner.a2a.auth.client_id"
            ]
        );
        assert_eq!(
            paths(&check(spec, Stage::Publish)),
            [
                "routes.partner.a2a.auth",
                "routes.partner.a2a.auth.client_id",
                "routes.partner.a2a.finish.schema",
            ]
        );
        let bearer = a2a_route(json!({ "card_url": CARD, "auth": { "kind": "bearer" },
                                       "finish": { "schema": schema() } }));
        assert_eq!(
            paths(&check(bearer, Stage::Draft)),
            ["routes.partner.a2a.auth.token"]
        );
    }

    #[test]
    fn a_route_to_an_external_agent_needs_a_task_and_a_trusted_gate_for_its_binds() {
        let spec = with_route(json!({
            "when": { "slot": "issue", "set": true },
            "bind": { "customer": "state.verified.customer_id" },
            "a2a": { "card_url": CARD, "finish": { "schema": schema() } }
        }));
        assert_eq!(
            paths(&check(spec, Stage::Draft)),
            ["routes.partner.when", "routes.partner.task"]
        );
    }

    #[test]
    fn a_route_names_exactly_one_target() {
        let spec = with_route(json!({
            "when": { "slot": "issue", "set": true },
            "task": "t",
            "human": {},
            "a2a": { "card_url": CARD }
        }));
        let issues = check(spec, Stage::Draft);
        assert_eq!(paths(&issues), ["routes.partner"]);
        assert!(
            issues[0].message.contains("`human` and `a2a`"),
            "{}",
            issues[0].message
        );
    }
}
