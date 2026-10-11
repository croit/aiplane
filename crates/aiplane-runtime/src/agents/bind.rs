// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Bound arguments and task templates (`docs/agent-runs.md` → "Bound arguments", trust rules 2
//! and 3).
//!
//! A bound argument is filled in by the gateway: it is removed from the
//! schema the model sees, and whatever the model sends for it is overwritten.
//! Its value is a `{const: …}` from the spec, or a slot path read from verified
//! state at call time. A sub-agent has no state of its own; the route that
//! dispatched it resolves its `bind` from the main agent's state once, and
//! those values reach every tool of the sub-agent that declares the argument.
//!
//! A task template is the only thing a sub-agent learns of the conversation:
//! `{slot}` and `{slot.field}` placeholders filled from the main agent's state.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::{Map, Value};
use shared::api::ToolDef;

use super::spec::AgentSpec;
use super::state::{AgentState, StateSchema, StateSnapshot};
use crate::server::tools::{Tool, ToolContext, ToolError, ToolFuture};

/// Where one bound argument's value comes from.
#[derive(Debug, Clone, PartialEq)]
pub enum BindSource {
    Const(Value),
    /// `state.<path>`: a slot path, `verified` or `verified.customer_id`.
    State(String),
    /// `route.<name>`: a value the dispatching route passes.
    Route(String),
}

impl BindSource {
    /// Read a spec's bind source: `"state.<path>"`, `"route.<name>"` or
    /// `{"const": v}`.
    pub fn parse(v: &Value) -> Option<Self> {
        match v {
            Value::String(src) => src
                .strip_prefix("state.")
                .map(|p| Self::State(p.to_string()))
                .or_else(|| {
                    src.strip_prefix("route.")
                        .map(|n| Self::Route(n.to_string()))
                }),
            Value::Object(m) if m.len() == 1 => m.get("const").cloned().map(Self::Const),
            _ => None,
        }
    }

    /// Whether the value says whose data a tool touches: anything read from
    /// state or passed down a route, as opposed to a fixed setting.
    fn carries_subject(&self) -> bool {
        !matches!(self, Self::Const(_))
    }

    /// The value from `state`, or what is missing.
    pub fn resolve(&self, state: &AgentState) -> Result<Value, String> {
        match self {
            Self::Const(v) => Ok(v.clone()),
            Self::State(path) => resolve_path(state, path),
            Self::Route(name) => Err(format!(
                "`route.{name}` was not passed by a route; this agent runs that tool only when a \
                 route that binds `{name}` dispatches it"
            )),
        }
    }
}

/// The value at `slot.field…` among the valid slots of `state`.
pub fn resolve_path(state: &AgentState, path: &str) -> Result<Value, String> {
    let mut parts = path.split('.');
    let slot = parts.next().unwrap_or_default();
    let Some(entry) = state.valid(slot) else {
        return Err(format!("`{slot}` is not set"));
    };
    let mut value = &entry.value;
    for field in parts {
        value = value
            .get(field)
            .ok_or_else(|| format!("`{slot}` has no `{field}`"))?;
    }
    Ok(value.clone())
}

/// Fill every `{slot}` / `{slot.field}` of `template` from `state`. Every
/// placeholder that cannot be filled is reported, so the caller can say all
/// of what is missing at once.
pub fn render_task(template: &str, state: &AgentState) -> Result<String, Vec<String>> {
    let mut out = String::new();
    let mut missing = Vec::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            out.push_str(&rest[open..]);
            rest = "";
            break;
        };
        match resolve_path(state, after[..close].trim()) {
            Ok(Value::String(s)) => out.push_str(&s),
            Ok(other) => out.push_str(&other.to_string()),
            Err(why) => missing.push(why),
        }
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    if missing.is_empty() {
        Ok(out)
    } else {
        Err(missing)
    }
}

/// Every bound argument of one run, mapped explicitly per tool by
/// `tool_resources.<tool>.bind`, with `route.<name>` sources filled from the
/// dispatching route's values.
///
/// A **subject parameter** is a parameter name some tool of the run binds
/// from state or from a route. Any other tool that declares a parameter of
/// that name must bind it as well, or it does not run: the name already says
/// whose data it selects, and the model must not choose it there either.
#[derive(Debug, Clone, Default)]
pub struct ToolBinds {
    per_tool: BTreeMap<String, BTreeMap<String, BindSource>>,
    route_values: BTreeMap<String, Value>,
}

impl ToolBinds {
    /// The binds a spec's `main.tool_resources` declares.
    pub fn from_spec(spec: &AgentSpec) -> Self {
        let per_tool = spec
            .main
            .tool_resources
            .iter()
            .filter(|(_, r)| !r.bind.is_empty())
            .map(|(tool, r)| (tool.clone(), r.bind.clone()))
            .collect();
        Self {
            per_tool,
            route_values: BTreeMap::new(),
        }
    }

    /// Add the values the dispatching route resolved.
    pub fn with_route(mut self, values: BTreeMap<String, Value>) -> Self {
        self.route_values = values;
        self
    }

    fn subject_params(&self) -> impl Iterator<Item = &str> {
        self.per_tool
            .values()
            .flatten()
            .filter(|(_, src)| src.carries_subject())
            .map(|(param, _)| param.as_str())
    }

    /// The binds of `tool`, whose model-facing schema is `def`: exactly what
    /// its `tool_resources` maps, with route values filled in. `Err` names
    /// the subject parameters `def` declares and leaves unbound.
    pub fn for_tool(
        &self,
        tool: &str,
        def: &ToolDef,
    ) -> Result<BTreeMap<String, BindSource>, Vec<String>> {
        let binds: BTreeMap<String, BindSource> = self
            .per_tool
            .get(tool)
            .into_iter()
            .flatten()
            .map(|(param, src)| {
                let src = match src {
                    BindSource::Route(name) => self
                        .route_values
                        .get(name)
                        .map_or_else(|| src.clone(), |v| BindSource::Const(v.clone())),
                    other => other.clone(),
                };
                (param.clone(), src)
            })
            .collect();
        let declared = def.function.parameters.get("properties");
        let mut unbound: Vec<String> = self
            .subject_params()
            .filter(|p| !binds.contains_key(*p))
            .filter(|p| declared.and_then(|d| d.get(*p)).is_some())
            .map(str::to_string)
            .collect();
        unbound.sort();
        unbound.dedup();
        if unbound.is_empty() {
            Ok(binds)
        } else {
            Err(unbound)
        }
    }
}

/// A tool that may not run in this agent run, with the reason.
pub struct WithheldTool {
    inner: Arc<dyn Tool>,
    unbound: Vec<String>,
}

impl WithheldTool {
    pub fn new(inner: Arc<dyn Tool>, unbound: Vec<String>) -> Self {
        Self { inner, unbound }
    }
}

impl Tool for WithheldTool {
    fn phase(&self) -> crate::server::tools::ToolPhase {
        self.inner.phase()
    }

    fn id(&self) -> &str {
        self.inner.id()
    }

    fn action_catalogue(&self) -> Option<&'static [crate::server::tools::ToolAction]> {
        self.inner.action_catalogue()
    }
    fn classify_actions<'a>(
        &'a self,
        ctx: ToolContext,
        args: Value,
    ) -> crate::server::tools::ToolActionFuture<'a> {
        Box::pin(async move {
            self.run(ctx, args).await?;
            Ok(None)
        })
    }

    fn schema(&self) -> ToolDef {
        self.inner.schema()
    }

    fn run<'a>(&'a self, _ctx: ToolContext, _args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let params: Vec<String> = self.unbound.iter().map(|p| format!("`{p}`")).collect();
            Err(ToolError::Failed(format!(
                "`{}` is not available in this agent: {} selects whose data it touches, and the \
                 agent's spec does not bind it for this tool. Do not retry.",
                self.inner.id(),
                params.join(", "),
            )))
        })
    }
}

/// A tool with some arguments filled in by the gateway.
pub struct BoundTool {
    inner: Arc<dyn Tool>,
    binds: BTreeMap<String, BindSource>,
    /// For slot sources, read from the run's state at call time.
    schema: Option<Arc<StateSchema>>,
    snapshot: Arc<StateSnapshot>,
}

impl BoundTool {
    pub fn new(
        inner: Arc<dyn Tool>,
        binds: BTreeMap<String, BindSource>,
        schema: Option<Arc<StateSchema>>,
        snapshot: Arc<StateSnapshot>,
    ) -> Self {
        Self {
            inner,
            binds,
            schema,
            snapshot,
        }
    }

    async fn bound_values(&self, ctx: &ToolContext) -> Result<Map<String, Value>, ToolError> {
        let needs_state = self
            .binds
            .values()
            .any(|s| matches!(s, BindSource::State(_)));
        let state = match (&self.schema, ctx.session_id.as_deref()) {
            (Some(schema), Some(session)) if needs_state => self
                .snapshot
                .get(&ctx.db, schema, session)
                .await
                .map_err(|e| {
                    ToolError::Failed(format!(
                        "reading the conversation state to fill `{}`'s bound arguments: {e}",
                        self.inner.id()
                    ))
                })?,
            _ => Arc::default(),
        };
        let mut values = Map::new();
        for (arg, source) in &self.binds {
            let value = source.resolve(&state).map_err(|why| {
                ToolError::Failed(format!(
                    "`{}` cannot run yet: the gateway fills its `{arg}` from the conversation \
                     state, and {why}. Do not retry until that is set.",
                    self.inner.id()
                ))
            })?;
            values.insert(arg.clone(), value);
        }
        Ok(values)
    }
}

/// `def` without the bound arguments: not offered, not required.
pub fn without_bound(mut def: ToolDef, bound: &BTreeMap<String, BindSource>) -> ToolDef {
    let params = &mut def.function.parameters;
    if let Some(props) = params.get_mut("properties").and_then(Value::as_object_mut) {
        props.retain(|k, _| !bound.contains_key(k));
    }
    if let Some(req) = params.get_mut("required").and_then(Value::as_array_mut) {
        req.retain(|k| k.as_str().is_none_or(|k| !bound.contains_key(k)));
    }
    def
}

impl Tool for BoundTool {
    fn phase(&self) -> crate::server::tools::ToolPhase {
        self.inner.phase()
    }

    fn id(&self) -> &str {
        self.inner.id()
    }

    fn action_catalogue(&self) -> Option<&'static [crate::server::tools::ToolAction]> {
        self.inner.action_catalogue()
    }
    fn classify_actions<'a>(
        &'a self,
        ctx: ToolContext,
        args: Value,
    ) -> crate::server::tools::ToolActionFuture<'a> {
        Box::pin(async move {
            let mut args = match args {
                Value::Object(map) => map,
                _ => Map::new(),
            };
            args.extend(self.bound_values(&ctx).await?);
            self.inner.classify_actions(ctx, Value::Object(args)).await
        })
    }
    fn read_only(&self) -> bool {
        self.inner.read_only()
    }

    fn schema(&self) -> ToolDef {
        without_bound(self.inner.schema(), &self.binds)
    }

    fn run<'a>(&'a self, ctx: ToolContext, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let mut args = match args {
                Value::Object(map) => map,
                _ => Map::new(),
            };
            args.extend(self.bound_values(&ctx).await?);
            self.inner.run(ctx, Value::Object(args)).await
        })
    }

    fn max_duration(&self) -> Option<std::time::Duration> {
        self.inner.max_duration()
    }

    fn sensitive_args(&self) -> bool {
        self.inner.sensitive_args()
    }

    fn changes_state(&self) -> bool {
        self.inner.changes_state()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn bound_and_withheld_wrappers_preserve_the_tools_phase() {
        let inner: Arc<dyn Tool> = Arc::new(crate::server::tools::tests::PhasedEcho);
        let bound = BoundTool::new(inner.clone(), BTreeMap::new(), None, Arc::default());
        let withheld = WithheldTool::new(inner, vec!["message".into()]);
        assert_eq!(bound.phase(), crate::server::tools::ToolPhase::ActsOnState);
        assert_eq!(
            withheld.phase(),
            crate::server::tools::ToolPhase::ActsOnState
        );
    }
    use super::*;
    use crate::agents::state::tests::{at, schema};
    use aiplane_agents::db::agent_state::StoredSlot;
    use serde_json::json;
    use std::sync::Mutex;

    fn state() -> AgentState {
        AgentState::from_rows(
            &schema(),
            vec![
                StoredSlot {
                    slot: "verified".into(),
                    value: json!({"customer_id": "K-12345"}),
                    provenance: "verifier:otp".into(),
                    set_at: at("2026-10-02T11:50:00Z"),
                },
                StoredSlot {
                    slot: "issue".into(),
                    value: json!("billing"),
                    provenance: "llm".into(),
                    set_at: at("2026-10-02T11:50:00Z"),
                },
            ],
        )
    }

    #[test]
    fn a_task_is_filled_from_state_and_reports_everything_missing() {
        assert_eq!(
            render_task(
                "Customer {verified.customer_id} asks about {issue}.",
                &state()
            ),
            Ok("Customer K-12345 asks about billing.".into())
        );
        assert_eq!(
            render_task("{email} / {verified.tier} / {issue}", &state()),
            Err(vec![
                "`email` is not set".to_string(),
                "`verified` has no `tier`".to_string()
            ])
        );
    }

    /// Records the arguments it was run with.
    struct Lookup(Mutex<Vec<Value>>);

    impl Tool for Lookup {
        fn id(&self) -> &str {
            "bound_fixture"
        }
        fn schema(&self) -> ToolDef {
            ToolDef::function(
                "bound_fixture",
                "Look up invoices.",
                json!({"type": "object",
                       "properties": {"customer_id": {"type": "string"}, "year": {"type": "integer"}},
                       "required": ["customer_id", "year"]}),
            )
        }
        fn run<'a>(&'a self, _ctx: ToolContext, args: Value) -> ToolFuture<'a> {
            self.0.lock().unwrap().push(args.clone());
            Box::pin(async move { Ok(args) })
        }
    }

    fn def(name: &str, params: &[&str]) -> ToolDef {
        let props: Map<String, Value> = params
            .iter()
            .map(|p| (p.to_string(), json!({"type": "string"})))
            .collect();
        ToolDef::function(name, "t", json!({"type": "object", "properties": props}))
    }

    #[test]
    fn a_tool_binds_exactly_the_parameters_its_tool_resources_map() {
        let spec = json!({"main": {"tool_resources": {
            "bound_fixture": {"bind": {
                "customer_id": "route.customer",
                "year": {"const": 2026}
            }}
        }}});
        let binds = ToolBinds::from_spec(&AgentSpec::from_value(&spec).unwrap())
            .with_route(BTreeMap::from([("customer".into(), json!("K-1"))]));
        assert_eq!(
            binds.for_tool(
                "bound_fixture",
                &def("bound_fixture", &["customer_id", "year"])
            ),
            Ok(BTreeMap::from([
                ("customer_id".into(), BindSource::Const(json!("K-1"))),
                ("year".into(), BindSource::Const(json!(2026))),
            ]))
        );
        assert_eq!(
            binds.for_tool("other", &def("other", &["customer"])),
            Ok(BTreeMap::new()),
            "a route value reaches no tool by name, only by an explicit mapping"
        );
    }

    #[test]
    fn a_tool_declaring_a_subject_parameter_another_tool_binds_must_bind_it_too() {
        let spec = json!({"main": {"tool_resources": {
            "invoices": {"bind": {"customer_id": "route.customer"}},
            "pinned": {"bind": {"tenant": {"const": "t1"}}}
        }}});
        let binds = ToolBinds::from_spec(&AgentSpec::from_value(&spec).unwrap())
            .with_route(BTreeMap::from([("customer".into(), json!("K-1"))]));
        assert_eq!(
            binds.for_tool("tickets", &def("tickets", &["customer_id", "text"])),
            Err(vec!["customer_id".to_string()])
        );
        assert_eq!(
            binds.for_tool("docs", &def("docs", &["query", "tenant"])),
            Ok(BTreeMap::new()),
            "a const bind fixes a value; it does not make the name a subject"
        );
    }

    #[test]
    fn a_route_value_the_route_did_not_pass_refuses_the_call() {
        assert!(
            BindSource::Route("customer".into())
                .resolve(&AgentState::default())
                .is_err()
        );
        assert_eq!(
            BindSource::parse(&json!("state.verified.customer_id")),
            Some(BindSource::State("verified.customer_id".into()))
        );
        assert_eq!(BindSource::parse(&json!("verified.customer_id")), None);
    }

    #[tokio::test]
    async fn a_bound_argument_is_hidden_from_the_model_and_overrides_what_it_sends() {
        let db = aiplane_core::server::db::open(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        let inner = Arc::new(Lookup(Mutex::default()));
        let bound = BoundTool::new(
            inner.clone(),
            BTreeMap::from([("customer_id".into(), BindSource::Const(json!("K-12345")))]),
            None,
            Arc::default(),
        );
        let params = bound.schema().function.parameters;
        assert_eq!(params["properties"], json!({"year": {"type": "integer"}}));
        assert_eq!(params["required"], json!(["year"]));

        bound
            .run(
                ToolContext::for_test(db),
                json!({"customer_id": "K-99999", "year": 2025}),
            )
            .await
            .unwrap();
        assert_eq!(
            inner.0.lock().unwrap().as_slice(),
            [json!({"customer_id": "K-12345", "year": 2025})]
        );
    }

    #[tokio::test]
    async fn a_slot_bound_argument_is_read_at_call_time_and_refused_while_unset() {
        let session = "s1".to_string();
        let db = crate::agents::state::tests::pool_with_session(&session).await;
        let schema = Arc::new(schema());
        let inner = Arc::new(Lookup(Mutex::default()));
        let snapshot = Arc::new(StateSnapshot::default());
        let bound = BoundTool::new(
            inner.clone(),
            BTreeMap::from([(
                "customer_id".into(),
                BindSource::State("verified.customer_id".into()),
            )]),
            Some(schema.clone()),
            snapshot.clone(),
        );
        let ctx = ToolContext {
            session_id: Some(session.clone()),
            ..ToolContext::for_test(db.clone())
        };
        let err = bound
            .run(ctx.clone(), json!({"year": 2026}))
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("`verified` is not set"), "{err}");
        assert!(inner.0.lock().unwrap().is_empty());

        crate::agents::state::write_trusted(
            &db,
            &schema,
            &session,
            "verified",
            json!({"customer_id": "K-777"}),
            crate::agents::state::TrustedWriter::Verifier("otp".into()),
            at("2026-10-02T11:50:00Z"),
        )
        .await
        .unwrap();
        snapshot.written();
        bound.run(ctx, json!({"year": 2026})).await.unwrap();
        assert_eq!(
            inner.0.lock().unwrap().as_slice(),
            [json!({"customer_id": "K-777", "year": 2026})]
        );
    }
}
