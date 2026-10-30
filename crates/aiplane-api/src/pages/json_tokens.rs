// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use std::collections::HashMap;
use std::sync::Arc;

use jiff::Timestamp;
use rama::http::service::web::extract::State;
use rama::http::{Request, Response, StatusCode};
use session_core::i18n::Lang;

use super::{bad_request, internal, json_error, json_ok, raw_path_segment};
use aiplane_core::server::db::{
    DbError, limits, model_defaults, token_models, token_tool_prefs, tokens, usage, user_mcp,
};
use aiplane_core::server::limits::LimitStatus;
use aiplane_core::server::priority::Priority;
use aiplane_core::server::upstreams::PoolAccess;
use aiplane_runtime::rama_server::state::RamaState;

/// The models a token can be limited to, each with what the person choosing
/// needs to weigh: its kind, where its data goes, and its price. An alias is
/// priced as the model it resolves to, since that is what it is metered as.
pub(crate) async fn model_catalog(
    state: &RamaState,
    access: &PoolAccess,
) -> Result<Vec<CatalogModelView>, DbError> {
    let prices = model_defaults::all_prices(&state.db).await?;
    Ok(state
        .upstreams
        .model_catalog_for(access)
        .into_iter()
        .map(|model| {
            let price = prices.get(model.alias_of.as_deref().unwrap_or(&model.id));
            CatalogModelView {
                kind: model.kind.as_str(),
                gdpr: model.compliance.gdpr,
                nda: model.compliance.nda,
                price: price.map(|price| ModelPriceView {
                    input: price.input_price,
                    output: price.output_price,
                    unit: price.pricing_unit.as_str(),
                }),
                id: model.id,
                alias_of: model.alias_of,
            }
        })
        .collect())
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct CatalogModelView {
    pub id: String,
    pub kind: &'static str,
    /// Whether the model's backend is flagged GDPR-compliant.
    pub gdpr: bool,
    /// Whether the model's backend is flagged as covered by an NDA.
    pub nda: bool,
    /// The model an alias resolves to; `null` for a real model.
    pub alias_of: Option<String>,
    pub price: Option<ModelPriceView>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct ModelPriceView {
    pub input: Option<f64>,
    pub output: Option<f64>,
    pub unit: &'static str,
}

/// One in-force limit with what has been spent against it, as `/usage` and
/// the token editor both draw it.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct LimitStatusView {
    /// The model the limit covers; `null` for all metered models together.
    pub model: Option<String>,
    pub dimension: &'static str,
    pub window: &'static str,
    pub limit: f64,
    pub used: f64,
    /// When the window next advances.
    pub refreshes_at: String,
}

impl From<&LimitStatus> for LimitStatusView {
    fn from(status: &LimitStatus) -> Self {
        Self {
            model: status.model.clone(),
            dimension: status.dimension.as_str(),
            window: status.window.as_str(),
            limit: status.limit,
            used: status.used,
            refreshes_at: status.refreshes_at.to_string(),
        }
    }
}

pub fn priority_limits(
    state: &RamaState,
    user: &aiplane_core::server::db::users::User,
) -> (Priority, Priority) {
    let inherited = state
        .rbac
        .request_priority(&user.roles, state.config().gateway.default_priority);
    let maximum = if super::is_admin(state, user) {
        Priority::High
    } else {
        inherited
    };
    (inherited, maximum)
}

#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TokenPriorityRequest {
    pub priority: Option<Priority>,
}

pub async fn priority(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, user) = require_session_json!(state, req);
    let lang = session_core::i18n::Lang::from_headers(req.headers());
    let Some(id) = raw_path_segment(&req, 1) else {
        return bad_request(session_core::i18n::t(lang, "priority-error-body"));
    };
    let (_, body) = req.into_parts();
    let parsed: TokenPriorityRequest = match super::read_json(body, "the token priority").await {
        Ok(parsed) => parsed,
        Err(_) => return bad_request(session_core::i18n::t(lang, "priority-error-body")),
    };
    match tokens::find_by_id(&state.db, &id).await {
        Ok(Some(token)) if token.user_id == user.id => {}
        Ok(_) => {
            return json_error(
                StatusCode::NOT_FOUND,
                "not_found",
                &session_core::i18n::t(lang, "priority-error-not-found"),
            );
        }
        Err(err) => {
            tracing::error!(%err, "reading personal token priority");
            return internal(session_core::i18n::t(lang, "priority-error-internal"));
        }
    }
    let (_, maximum) = priority_limits(&state, &user);
    if parsed.priority.is_some_and(|priority| priority > maximum) {
        return json_error(
            StatusCode::FORBIDDEN,
            "forbidden",
            &session_core::i18n::t(lang, "priority-error-ceiling"),
        );
    }
    match tokens::set_priority(&state.db, &id, parsed.priority).await {
        Ok(()) => json_ok(StatusCode::OK, parsed),
        Err(err) => {
            tracing::error!(%err, "saving personal token priority");
            internal(session_core::i18n::t(lang, "priority-error-internal"))
        }
    }
}

pub async fn details(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let lang = Lang::from_request(req.headers());
    let (session, user) = require_session_json!(state, req);
    let token_rows = match tokens::list_for_user(&state.db, &user.id).await {
        Ok(rows) => rows,
        Err(err) => return internal(err),
    };
    let model_lists = match token_models::lists_for_user(&state.db, &user.id).await {
        Ok(lists) => lists,
        Err(err) => return internal(err),
    };
    let mut quotas: HashMap<String, Vec<limits::LimitRule>> = HashMap::new();
    match limits::for_tokens_of_user(&state.db, &user.id).await {
        Ok(rules) => {
            for rule in rules {
                quotas
                    .entry(rule.subject_id.clone())
                    .or_default()
                    .push(rule);
            }
        }
        Err(err) => return internal(err),
    }

    let timezone = session
        .timezone
        .as_deref()
        .or(user.timezone.as_deref())
        .unwrap_or("UTC");
    let now = Timestamp::now();
    let usage_by_token = if state.usage.is_enabled() {
        let bounds = usage::period_bounds(usage::Period::ThisMonth, timezone, now);
        match usage::by_token(
            &state.db,
            bounds,
            Some(&user.id),
            state.config().usage.retention_days,
            now,
        )
        .await
        {
            Ok(rows) => rows.into_iter().map(|row| (row.key.clone(), row)).collect(),
            Err(err) => return internal(err),
        }
    } else {
        HashMap::new()
    };

    let capabilities =
        super::tool_toggles::token_capabilities_for_user(&state, &user.roles, &user.id, lang).await;
    let mut token_details = Vec::with_capacity(token_rows.len());
    for token in token_rows {
        let lists = model_lists.get(&token.id).cloned().unwrap_or_default();
        let tool_states = match token_tool_prefs::states_for_token(&state.db, &token.id).await {
            Ok(states) => states
                .into_iter()
                .map(|(key, mode)| {
                    (
                        key,
                        match mode {
                            1 => "on",
                            2 => "auto",
                            _ => "off",
                        },
                    )
                })
                .collect::<HashMap<_, _>>(),
            Err(err) => return internal(err),
        };
        let mcp_allow = match user_mcp::token_ask_policy(&state.db, &token.id, "*").await {
            Ok(user_mcp::AskOverApi::Allow) => true,
            Ok(user_mcp::AskOverApi::Block) => false,
            Err(err) => return internal(err),
        };
        let token_quotas = quotas.remove(&token.id).unwrap_or_default();
        let quota_status = state.enforcer.token_statuses(&token.id).await;
        let usage = usage_by_token.get(&token.id);
        token_details.push(TokenDetail {
            priority: token.priority,
            id: token.id,
            name: token.name,
            created_at: token.created_at,
            last_used_at: token.last_used_at,
            expires_at: token.expires_at,
            revoked: token.revoked_at.is_some(),
            tools_enabled: token.tools_enabled,
            tool_states,
            owner_models: lists.owner,
            admin_models: lists.admin,
            mcp_allow,
            quotas: token_quotas
                .into_iter()
                .map(|rule| TokenQuota {
                    id: rule.id,
                    model: rule.model,
                    dimension: rule.dimension.as_str(),
                    window: rule.window.as_str(),
                    value: rule.value,
                    managed_by: rule.managed_by.as_str(),
                })
                .collect(),
            quota_status: quota_status.iter().map(LimitStatusView::from).collect(),
            usage: usage.map(|row| TokenUsage {
                requests: row.requests,
                tokens: row.total_tokens,
                cost: row.cost,
            }),
        });
    }
    let role_ids = state.rbac.role_ids_for(&user.roles);
    let models = match model_catalog(&state, &state.pool_access_for(&user.roles)).await {
        Ok(models) => models,
        Err(err) => return internal(err),
    };
    let owner_limits = state.enforcer.statuses(&user.id, &role_ids).await;
    let (owner_priority, max_priority) = priority_limits(&state, &user);
    json_ok(
        StatusCode::OK,
        TokenDetails {
            owner_priority,
            max_priority,
            priority_enabled: state.config().gateway.priority_enabled,
            tokens: token_details,
            capabilities,
            models,
            owner_limits: owner_limits.iter().map(LimitStatusView::from).collect(),
            usage_enabled: state.usage.is_enabled(),
            currency: state.config().usage.currency.clone(),
            push_enabled: state.push.is_some(),
            timezone: timezone.to_string(),
            account: AccountView {
                email: user.email,
                user_id: user.id,
                oidc_roles: user.roles,
                is_admin: state.rbac.is_admin(&role_ids),
                rbac_roles: role_ids,
            },
        },
    )
}

/// What `GET /api/v0/tokens/details` answers: the caller's tokens with
/// everything the token editor shows, and the option sets it picks from.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct TokenDetails {
    pub owner_priority: Priority,
    pub max_priority: Priority,
    pub priority_enabled: bool,
    pub tokens: Vec<TokenDetail>,
    /// The capabilities a token can switch on, off or to automatic.
    pub capabilities: Vec<super::tool_toggles::CapabilityEntry>,
    pub models: Vec<CatalogModelView>,
    /// The limits in force on the caller themselves.
    pub owner_limits: Vec<LimitStatusView>,
    pub usage_enabled: bool,
    pub currency: String,
    pub push_enabled: bool,
    pub timezone: String,
    pub account: AccountView,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct TokenDetail {
    pub priority: Option<Priority>,
    pub id: String,
    pub name: String,
    pub created_at: Timestamp,
    pub last_used_at: Option<Timestamp>,
    pub expires_at: Timestamp,
    pub revoked: bool,
    pub tools_enabled: bool,
    /// Capability key → `on`, `off` or `auto`, for the keys set explicitly.
    pub tool_states: HashMap<String, &'static str>,
    /// The owner's model allowlist; `null` allows every model.
    pub owner_models: Option<Vec<String>>,
    /// An admin's model allowlist; `null` allows every model.
    pub admin_models: Option<Vec<String>>,
    /// Whether MCP tools that ask first may run over the API with this token.
    pub mcp_allow: bool,
    pub quotas: Vec<TokenQuota>,
    pub quota_status: Vec<LimitStatusView>,
    /// This month's usage; `null` when usage metering is off or the token
    /// has none.
    pub usage: Option<TokenUsage>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct TokenQuota {
    pub id: String,
    pub model: Option<String>,
    pub dimension: &'static str,
    pub window: &'static str,
    pub value: f64,
    /// Who set the rule, and so who may change it.
    pub managed_by: &'static str,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct TokenUsage {
    pub requests: i64,
    pub tokens: i64,
    pub cost: f64,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct AccountView {
    pub email: String,
    pub user_id: String,
    /// The OIDC group claim values.
    pub oidc_roles: Vec<String>,
    /// The RBAC roles those resolve to.
    pub rbac_roles: Vec<String>,
    /// Whether one of those roles is flagged `is_admin`.
    pub is_admin: bool,
}
