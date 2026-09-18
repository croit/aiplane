// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use std::collections::HashMap;
use std::sync::Arc;

use jiff::Timestamp;
use rama::http::service::web::extract::State;
use rama::http::{Request, Response, StatusCode};

use super::{entries_for_roles, internal, json_ok};
use aiplane_core::server::db::{limits, token_models, token_tool_prefs, tokens, usage, user_mcp};
use aiplane_runtime::rama_server::state::RamaState;

pub async fn details(State(state): State<Arc<RamaState>>, req: Request) -> Response {
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

    let entries = entries_for_roles(&state, &user.roles);
    let tools = entries
        .iter()
        .map(|entry| {
            serde_json::json!({
                "key": entry.key,
                "title": entry.title,
                "tech": entry.tech,
                "description": entry.description,
                "category": entry.category.label(),
            })
        })
        .collect::<Vec<_>>();
    let mut token_details = Vec::with_capacity(token_rows.len());
    for token in token_rows {
        let lists = model_lists.get(&token.id).cloned().unwrap_or_default();
        let disabled_tools = match token_tool_prefs::disabled_for_token(&state.db, &token.id).await
        {
            Ok(keys) => keys,
            Err(err) => return internal(err),
        };
        let mcp_allow = match user_mcp::token_ask_policy(&state.db, &token.id, "*").await {
            Ok(user_mcp::AskOverApi::Allow) => true,
            Ok(user_mcp::AskOverApi::Block) => false,
            Err(err) => return internal(err),
        };
        let token_quotas = quotas.remove(&token.id).unwrap_or_default();
        let usage = usage_by_token.get(&token.id);
        token_details.push(serde_json::json!({
            "id": token.id,
            "name": token.name,
            "created_at": token.created_at,
            "last_used_at": token.last_used_at,
            "expires_at": token.expires_at,
            "revoked": token.revoked_at.is_some(),
            "tools_enabled": token.tools_enabled,
            "disabled_tools": disabled_tools,
            "owner_models": lists.owner,
            "admin_models": lists.admin,
            "mcp_allow": mcp_allow,
            "quotas": token_quotas.into_iter().map(|rule| serde_json::json!({
                "id": rule.id,
                "model": rule.model,
                "dimension": rule.dimension.as_str(),
                "window": rule.window.as_str(),
                "value": rule.value,
                "managed_by": rule.managed_by.as_str(),
            })).collect::<Vec<_>>(),
            "usage": usage.map(|row| serde_json::json!({
                "requests": row.requests,
                "tokens": row.total_tokens,
                "cost": row.cost,
            })),
        }));
    }
    let role_ids = state.rbac.role_ids_for(&user.roles);
    let models = state
        .upstreams
        .all_models_for(&state.pool_access_for(&user.roles));
    json_ok(
        StatusCode::OK,
        serde_json::json!({
            "tokens": token_details,
            "tools": tools,
            "models": models,
            "usage_enabled": state.usage.is_enabled(),
        "currency": state.config().usage.currency,
        "push_enabled": state.push.is_some(),
        "timezone": timezone,
        "account": {
                "email": user.email,
                "user_id": user.id,
                "oidc_roles": user.roles,
                "rbac_roles": role_ids,
            },
        }),
    )
}
