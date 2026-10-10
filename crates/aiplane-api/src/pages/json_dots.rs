// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Personal Dot lifecycle endpoints, backed by the existing agent registry.

use std::sync::Arc;

use rama::http::service::web::extract::State;
use rama::http::{Request, Response, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::json;
use session_core::i18n::{Lang, t};

use super::json_principals::require_agent_manager;
use super::{bad_request, internal, json_error, json_ok, not_found, raw_path_segment};
use aiplane_agents::db::agents::{self as agents_db, PersonalDotCreate};
use aiplane_agents::db::system_principals::NewPrincipal;
use aiplane_runtime::rama_server::state::RamaState;

#[derive(Deserialize, schemars::JsonSchema)]
pub struct CreateDotBody {
    pub name: String,
    #[serde(default)]
    pub display: Option<String>,
    #[serde(default)]
    pub description: String,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct DotView {
    pub id: String,
    pub name: String,
    pub display: String,
    pub description: String,
    pub active: bool,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct DotList {
    pub dots: Vec<DotView>,
    pub active_limit: u32,
}

fn view(row: &agents_db::PersonalDotRow) -> DotView {
    let p = &row.agent.principal;
    DotView {
        id: p.id.clone(),
        name: row.name.clone(),
        display: p.display.clone(),
        description: p.description.clone(),
        active: row.active,
    }
}

pub async fn list(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let user = match require_agent_manager(&state, &req).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    let active_limit = state.config().dots.max_active_per_user;
    match agents_db::list_personal_dots(&state.db, &user.id).await {
        Ok(rows) => json_ok(
            StatusCode::OK,
            DotList {
                dots: rows.iter().map(view).collect(),
                active_limit,
            },
        ),
        Err(error) => internal(error),
    }
}

pub async fn create(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let user = match require_agent_manager(&state, &req).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    let lang = Lang::from_request(req.headers());
    let body: CreateDotBody = match super::read_json(req.into_body(), "the Dot body").await {
        Ok(body) => body,
        Err(response) => return response,
    };
    let name = body.name.trim();
    if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
        return json_error(
            StatusCode::BAD_REQUEST,
            "invalid_name",
            &t(lang, "dot-error-name-invalid"),
        );
    }
    let display = body
        .display
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(name);
    match agents_db::create_personal_dot(
        &state.db,
        &NewPrincipal {
            name,
            display,
            description: body.description.trim(),
        },
        r#"{"main":{}}"#,
        &user.id,
        state.config().dots.max_active_per_user,
    )
    .await
    {
        Ok((PersonalDotCreate::Created, Some(agent))) => json_ok(
            StatusCode::CREATED,
            json!({ "dot": { "id": agent.principal.id, "name": name,
                "display": agent.principal.display, "description": agent.principal.description,
                "active": true } }),
        ),
        Ok((PersonalDotCreate::NameTaken, _)) => json_error(
            StatusCode::CONFLICT,
            "conflict",
            &t(lang, "dot-error-name-taken"),
        ),
        Ok((PersonalDotCreate::ActiveLimit, _)) => json_error(
            StatusCode::CONFLICT,
            "active_dot_limit",
            &t(lang, "dot-error-active-limit"),
        ),
        Ok((PersonalDotCreate::Created, None)) => internal("created Dot could not be loaded"),
        Err(error) => internal(error),
    }
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct ActiveBody {
    pub active: bool,
}

pub async fn set_active(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let user = match require_agent_manager(&state, &req).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    let lang = Lang::from_request(req.headers());
    let Some(id) = raw_path_segment(&req, 1) else {
        return bad_request(t(lang, "dot-error-id-missing"));
    };
    let body: ActiveBody = match super::read_json(req.into_body(), "the Dot state").await {
        Ok(body) => body,
        Err(response) => return response,
    };
    match agents_db::set_personal_dot_active(
        &state.db,
        &id,
        &user.id,
        body.active,
        state.config().dots.max_active_per_user,
    )
    .await
    {
        Ok(true) => json_ok(StatusCode::OK, json!({ "id": id, "active": body.active })),
        Ok(false) => {
            let owned = agents_db::list_personal_dots(&state.db, &user.id).await;
            match owned {
                Ok(rows) if rows.iter().any(|row| row.agent.principal.id == id) => json_error(
                    StatusCode::CONFLICT,
                    "active_dot_limit",
                    &t(lang, "dot-error-active-limit"),
                ),
                Ok(_) => not_found(t(lang, "dot-error-not-found")),
                Err(error) => internal(error),
            }
        }
        Err(error) => internal(error),
    }
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct DotMemoryList {
    pub memories: Vec<DotMemoryView>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct DotMemoryView {
    pub id: String,
    pub kind: &'static str,
    pub content: String,
    pub created_at: String,
}

fn memory_view(memory: &aiplane_core::server::db::user_memories::Memory) -> DotMemoryView {
    DotMemoryView {
        id: memory.id.clone(),
        kind: memory.kind.as_str(),
        content: memory.content.clone(),
        created_at: memory.created_at.to_string(),
    }
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct DotMemoryBody {
    pub kind: String,
    pub content: String,
}

fn parse_memory_body(
    body: DotMemoryBody,
) -> Result<(aiplane_core::server::db::user_memories::MemoryKind, String), DotMemoryInputError> {
    let kind = aiplane_core::server::db::user_memories::MemoryKind::parse(&body.kind)
        .ok_or(DotMemoryInputError::Kind)?;
    let content = body.content.trim().to_string();
    if content.is_empty() {
        return Err(DotMemoryInputError::Empty);
    }
    Ok((kind, content))
}

#[derive(Debug, Clone, Copy)]
enum DotMemoryInputError {
    Kind,
    Empty,
}

pub async fn memories_list(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_, user) = require_session_json!(state, req);
    let Some(dot_id) = raw_path_segment(&req, 1) else {
        return bad_request(t(Lang::from_request(req.headers()), "dot-error-id-missing"));
    };
    match aiplane_agents::db::dot_memories::list(&state.db, &user.id, &dot_id, 500).await {
        Ok(Some(memories)) => json_ok(
            StatusCode::OK,
            DotMemoryList {
                memories: memories.iter().map(memory_view).collect(),
            },
        ),
        Ok(None) => not_found(t(Lang::from_request(req.headers()), "dot-error-not-found")),
        Err(error) => internal(error),
    }
}

pub async fn memories_create(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_, user) = require_session_json!(state, req);
    let lang = Lang::from_request(req.headers());
    let Some(dot_id) = raw_path_segment(&req, 1) else {
        return bad_request(t(lang, "dot-error-id-missing"));
    };
    let body = match super::read_json::<DotMemoryBody>(req.into_body(), "the Dot memory body").await
    {
        Ok(body) => body,
        Err(response) => return response,
    };
    let (kind, content) = match parse_memory_body(body) {
        Ok(value) => value,
        Err(error) => {
            return bad_request(t(
                lang,
                match error {
                    DotMemoryInputError::Kind => "dot-memory-error-kind",
                    DotMemoryInputError::Empty => "dot-memory-error-empty",
                },
            ));
        }
    };
    match aiplane_agents::db::dot_memories::insert(&state.db, &user.id, &dot_id, kind, &content)
        .await
    {
        Ok(Some(memory)) => json_ok(StatusCode::CREATED, memory_view(&memory)),
        Ok(None) => not_found(t(lang, "dot-error-not-found")),
        Err(error) => internal(error),
    }
}

pub async fn memories_update(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_, user) = require_session_json!(state, req);
    let lang = Lang::from_request(req.headers());
    let (Some(dot_id), Some(memory_id)) = (raw_path_segment(&req, 2), raw_path_segment(&req, 0))
    else {
        return bad_request(t(lang, "dot-memory-error-id-missing"));
    };
    let body = match super::read_json::<DotMemoryBody>(req.into_body(), "the Dot memory body").await
    {
        Ok(body) => body,
        Err(response) => return response,
    };
    let (kind, content) = match parse_memory_body(body) {
        Ok(value) => value,
        Err(error) => {
            return bad_request(t(
                lang,
                match error {
                    DotMemoryInputError::Kind => "dot-memory-error-kind",
                    DotMemoryInputError::Empty => "dot-memory-error-empty",
                },
            ));
        }
    };
    match aiplane_agents::db::dot_memories::update(
        &state.db, &user.id, &dot_id, &memory_id, kind, &content,
    )
    .await
    {
        Ok(Some(memory)) => json_ok(StatusCode::OK, memory_view(&memory)),
        Ok(None) => not_found(t(lang, "dot-error-not-found")),
        Err(error) => internal(error),
    }
}

pub async fn memories_delete(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_, user) = require_session_json!(state, req);
    let lang = Lang::from_request(req.headers());
    let (Some(dot_id), Some(memory_id)) = (raw_path_segment(&req, 2), raw_path_segment(&req, 0))
    else {
        return bad_request(t(lang, "dot-memory-error-id-missing"));
    };
    match aiplane_agents::db::dot_memories::delete(&state.db, &user.id, &dot_id, &memory_id).await {
        Ok(true) => Response::builder()
            .status(StatusCode::NO_CONTENT)
            .body(rama::http::Body::empty())
            .expect("static empty response"),
        Ok(false) => not_found(t(lang, "dot-error-not-found")),
        Err(error) => internal(error),
    }
}

pub async fn activity(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_, user) = require_session_json!(state, req);
    let lang = Lang::from_request(req.headers());
    let Some(dot_id) = raw_path_segment(&req, 1) else {
        return bad_request(t(lang, "dot-error-id-missing"));
    };
    let dot = match agents_db::get_personal_dot(&state.db, &user.id, &dot_id).await {
        Ok(Some(dot)) => dot,
        Ok(None) => return not_found(t(lang, "dot-error-not-found")),
        Err(error) => return internal(error),
    };
    let params = super::query_map(&req);
    let limit = match super::parse_limit(&params, 100, 500) {
        Ok(limit) => limit,
        Err(response) => return response,
    };
    let cursor = match super::parse_cursor(&params) {
        Ok(cursor) => cursor,
        Err(response) => return response,
    };
    let query = aiplane_agents::db::agent_audit::ActivityQuery {
        agent_id: dot.agent.principal.id,
        cursor,
        order: aiplane_agents::db::agent_audit::Order::Desc,
        limit,
        max_bytes: 4 * 1024 * 1024,
        ..Default::default()
    };
    let page = match aiplane_agents::db::agent_audit::page(&state.db, &query).await {
        Ok(page) => page,
        Err(error) => return internal(error),
    };
    let mut reconstructor = aiplane_agents::db::agent_audit::Reconstructor::default();
    let mut events = Vec::with_capacity(page.events.len());
    for event in &page.events {
        match reconstructor.event_json(&state.db, event).await {
            Ok(event) => events.push(event),
            Err(error) => return internal(error),
        }
    }
    json_ok(
        StatusCode::OK,
        super::json_agent_activity::ActivityPageView {
            events,
            next_cursor: page.next_cursor,
            order: "desc",
        },
    )
}
