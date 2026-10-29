// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Owner-only control of a suspended personal agent browser.

use std::sync::Arc;

use super::{json_error, json_ok};
use aiplane_agents::db::{agent_sessions, agents};
use aiplane_runtime::rama_server::state::RamaState;
use aiplane_runtime::server::tools::sandbox::BrowserHandoverBinding;
use rama::http::service::web::extract::State;
use rama::http::{Request, Response, StatusCode, header};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use session_core::chrome::{CappedBodyError, read_body_capped};
use session_core::db::{self as chat, SuspensionKind, TurnStatus};
use session_core::i18n::{Lang, t};
use shared::sandbox::{BrowserControlOperation, BrowserControlRequest, BrowserInput};

const MAX_CONTROL_BODY: usize = 4096;

#[derive(Clone, Copy)]
enum ControlAction {
    Claim,
    Frame,
    Input,
    Finish,
    Abort,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrowserControlBody {
    pub controller_id: String,
    #[serde(default)]
    pub sequence: Option<u64>,
    #[serde(default)]
    pub input: Option<BrowserInput>,
}

#[derive(Serialize, JsonSchema)]
pub struct BrowserControlView {
    pub controller_id: Option<String>,
    pub expires_at: String,
    pub frame_b64: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Serialize, JsonSchema)]
pub struct BrowserHandoverReturned {
    pub returned: bool,
}

#[derive(Serialize, JsonSchema)]
pub struct BrowserHandoverAborted {
    pub cancelled: bool,
}

struct Handover {
    turn: chat::Turn,
    request_id: String,
    binding: BrowserHandoverBinding,
}

pub async fn claim(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    control(&state, req, ControlAction::Claim).await
}

pub async fn frame(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    control(&state, req, ControlAction::Frame).await
}

pub async fn input(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    control(&state, req, ControlAction::Input).await
}

pub async fn finish(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    control(&state, req, ControlAction::Finish).await
}

pub async fn abort(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    control(&state, req, ControlAction::Abort).await
}

async fn control(state: &Arc<RamaState>, req: Request, action: ControlAction) -> Response {
    let lang = Lang::from_request(req.headers());
    let (_, user) = require_session_json!(*state, req);
    if !same_origin(state, &req) {
        return private(json_error(
            StatusCode::FORBIDDEN,
            "forbidden",
            &t(lang, "agent-error-not-found"),
        ));
    }
    let handover = match resolve_handover(state, &req, &user.id, lang).await {
        Ok(handover) => handover,
        Err(response) => return private(response),
    };
    let request_ctx = super::chat::json_api::request_ctx(state, &req, false);
    let (controller_id, operation) = match action {
        ControlAction::Claim => {
            let body = match read_control_body(req.into_body()).await {
                Ok(body) if body.sequence.is_none() && body.input.is_none() => body,
                Ok(_) => {
                    return private(json_error(
                        StatusCode::BAD_REQUEST,
                        "invalid_request",
                        &t(lang, "agent-workspace-error-invalid"),
                    ));
                }
                Err(too_large) => {
                    let (status, message) = if too_large {
                        (
                            StatusCode::PAYLOAD_TOO_LARGE,
                            "agent-workspace-error-too-large",
                        )
                    } else {
                        (StatusCode::BAD_REQUEST, "agent-workspace-error-invalid")
                    };
                    return private(json_error(status, "invalid_request", &t(lang, message)));
                }
            };
            if body.controller_id.is_empty() || body.controller_id.len() > 128 {
                return private(json_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    &t(lang, "agent-workspace-error-invalid"),
                ));
            }
            (
                body.controller_id,
                BrowserControlOperation::Claim {
                    generation: handover.binding.generation,
                },
            )
        }
        ControlAction::Frame
        | ControlAction::Input
        | ControlAction::Finish
        | ControlAction::Abort => {
            let body = match read_control_body(req.into_body()).await {
                Ok(body) => body,
                Err(too_large) => {
                    let (status, message) = if too_large {
                        (
                            StatusCode::PAYLOAD_TOO_LARGE,
                            "agent-workspace-error-too-large",
                        )
                    } else {
                        (StatusCode::BAD_REQUEST, "agent-workspace-error-invalid")
                    };
                    return private(json_error(status, "invalid_request", &t(lang, message)));
                }
            };
            let operation = match action {
                ControlAction::Frame => {
                    if body.sequence.is_some() || body.input.is_some() {
                        return private(json_error(
                            StatusCode::BAD_REQUEST,
                            "invalid_request",
                            &t(lang, "agent-workspace-error-invalid"),
                        ));
                    }
                    BrowserControlOperation::Frame {
                        generation: handover.binding.generation,
                    }
                }
                ControlAction::Input => {
                    let (Some(sequence), Some(input)) = (body.sequence, body.input) else {
                        return private(json_error(
                            StatusCode::BAD_REQUEST,
                            "invalid_request",
                            &t(lang, "agent-workspace-error-invalid"),
                        ));
                    };
                    BrowserControlOperation::Input {
                        generation: handover.binding.generation,
                        sequence,
                        input,
                    }
                }
                ControlAction::Finish | ControlAction::Abort => {
                    if body.sequence.is_some() || body.input.is_some() {
                        return private(json_error(
                            StatusCode::BAD_REQUEST,
                            "invalid_request",
                            &t(lang, "agent-workspace-error-invalid"),
                        ));
                    }
                    BrowserControlOperation::Finish {
                        generation: handover.binding.generation,
                    }
                }
                ControlAction::Claim => unreachable!(),
            };
            (body.controller_id, operation)
        }
    };
    let reservation = if matches!(action, ControlAction::Finish) {
        match super::chat::reserve_browser_handover_resume(state, &user, &handover.turn) {
            Ok(reservation) => Some(reservation),
            Err(super::chat::ResumeTurnError::Busy) => {
                return private(json_error(
                    StatusCode::CONFLICT,
                    "turn_busy",
                    &t(lang, "agent-workspace-error-conflict"),
                ));
            }
            Err(_) => {
                return private(json_error(
                    StatusCode::CONFLICT,
                    "handover_unavailable",
                    &t(lang, "agent-workspace-error-conflict"),
                ));
            }
        }
    } else {
        None
    };
    let Some(client) = state.sandbox_client() else {
        return private(json_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "browser_unavailable",
            &t(lang, "agent-workspace-error-unavailable"),
        ));
    };
    let response = match client
        .browser_control(BrowserControlRequest {
            workspace: handover.binding.workspace.clone(),
            turn_id: handover.binding.turn_id.clone(),
            container_id: handover.binding.container_id.clone(),
            handover_id: handover.binding.handover_id.clone(),
            controller_id: Some(controller_id.clone()),
            operation,
        })
        .await
    {
        Ok(response) => response,
        Err(error) => return private(client_error(error, lang)),
    };
    if matches!(action, ControlAction::Finish) {
        match super::chat::resume_browser_handover(
            state,
            &user,
            &handover.turn,
            &handover.request_id,
            request_ctx,
            reservation.expect("finish reserves its resume worker"),
        )
        .await
        {
            Ok(()) => {
                return private(json_ok(
                    StatusCode::ACCEPTED,
                    BrowserHandoverReturned { returned: true },
                ));
            }
            Err(super::chat::ResumeTurnError::Busy) => {
                return private(json_error(
                    StatusCode::CONFLICT,
                    "turn_busy",
                    &t(lang, "agent-workspace-error-conflict"),
                ));
            }
            Err(_) => {
                return private(json_error(
                    StatusCode::CONFLICT,
                    "handover_unavailable",
                    &t(lang, "agent-workspace-error-conflict"),
                ));
            }
        }
    }
    if matches!(action, ControlAction::Abort) {
        if aiplane_runtime::agents::resume::cancel_personal_tree(state, &user.id, &handover.turn.id)
            .await
            .is_err()
        {
            return private(json_error(
                StatusCode::CONFLICT,
                "handover_unavailable",
                &t(lang, "agent-workspace-error-conflict"),
            ));
        }
        return private(json_ok(
            StatusCode::OK,
            BrowserHandoverAborted { cancelled: true },
        ));
    }
    private(json_ok(
        StatusCode::OK,
        BrowserControlView {
            controller_id: matches!(action, ControlAction::Claim).then_some(controller_id),
            expires_at: response.expires_at,
            frame_b64: response.frame_b64,
            width: response.width,
            height: response.height,
        },
    ))
}

async fn read_control_body(body: rama::http::Body) -> Result<BrowserControlBody, bool> {
    let bytes = read_body_capped(body, MAX_CONTROL_BODY)
        .await
        .map_err(|error| matches!(error, CappedBodyError::TooLarge { .. }))?;
    serde_json::from_slice(&bytes).map_err(|_| false)
}

async fn resolve_handover(
    state: &RamaState,
    req: &Request,
    user_id: &str,
    lang: Lang,
) -> Result<Handover, Response> {
    let segment = |from_end| {
        super::raw_path_segment(req, from_end)
            .ok_or_else(|| super::bad_request(t(lang, "agent-error-id-missing")))
    };
    let session_id = segment(5)?;
    let turn_id = segment(3)?;
    let request_id = segment(1)?;
    if segment(2)?.as_str() != "browser-handover"
        || segment(4)?.as_str() != "turns"
        || segment(6)?.as_str() != "sessions"
    {
        return Err(super::not_found(t(lang, "agent-error-not-found")));
    }
    let linked = agent_sessions::for_session(&state.db, user_id, &session_id)
        .await
        .map_err(super::internal)?;
    let Some(linked) = linked.filter(|link| link.active) else {
        return Err(super::not_found(t(lang, "agent-error-not-found")));
    };
    let Some(owned_agent) = agents::get_personal_agent(&state.db, user_id, &linked.agent_id)
        .await
        .map_err(super::internal)?
    else {
        return Err(super::not_found(t(lang, "agent-error-not-found")));
    };
    if !owned_agent.active {
        return Err(super::json_error(
            StatusCode::CONFLICT,
            "agent_inactive",
            &t(lang, "agent-error-inactive"),
        ));
    }
    if chat::get_session(&state.db, user_id, &session_id)
        .await
        .map_err(super::internal)?
        .is_none()
    {
        return Err(super::not_found(t(lang, "agent-error-not-found")));
    }
    let Some(turn) = chat::get_turn(&state.db, &session_id, &turn_id)
        .await
        .map_err(super::internal)?
    else {
        return Err(super::not_found(t(lang, "agent-error-not-found")));
    };
    if turn.status != TurnStatus::Suspended {
        return Err(super::not_found(t(lang, "agent-error-not-found")));
    }
    let Some(suspension) = chat::get_suspension(&state.db, &turn_id)
        .await
        .map_err(super::internal)?
    else {
        return Err(super::not_found(t(lang, "agent-error-not-found")));
    };
    if suspension.kind != SuspensionKind::BrowserHandover || suspension.request_id != request_id {
        return Err(super::not_found(t(lang, "agent-error-not-found")));
    }
    let binding = suspension
        .run_context
        .as_ref()
        .and_then(|context| context.get("browser_handover"))
        .cloned()
        .and_then(|value| serde_json::from_value::<BrowserHandoverBinding>(value).ok())
        .ok_or_else(|| super::not_found(t(lang, "agent-error-not-found")))?;
    if binding.workspace.owner_user_id != user_id
        || binding.workspace.agent_id != linked.agent_id
        || binding.turn_id != turn_id
    {
        return Err(super::not_found(t(lang, "agent-error-not-found")));
    }
    Ok(Handover {
        turn,
        request_id,
        binding,
    })
}

fn same_origin(state: &RamaState, req: &Request) -> bool {
    let Some(origin) = req
        .headers()
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    origin_matches(origin, &state.public_url())
}

fn origin_matches(origin: &str, public_url: &str) -> bool {
    let Ok(origin) = origin.parse::<rama::http::Uri>() else {
        return false;
    };
    let Ok(public) = public_url.parse::<rama::http::Uri>() else {
        return false;
    };
    origin.scheme_str() == public.scheme_str()
        && origin
            .authority()
            .zip(public.authority())
            .is_some_and(|(left, right)| left.as_str().eq_ignore_ascii_case(right.as_str()))
        && origin
            .path_and_query()
            .is_none_or(|path| path.as_str() == "/")
}

fn client_error(
    error: aiplane_runtime::server::tools::sandbox::WorkspaceClientError,
    lang: Lang,
) -> Response {
    use aiplane_runtime::server::tools::sandbox::WorkspaceClientError as Error;
    let (status, key) = match error {
        Error::Unavailable => (
            StatusCode::SERVICE_UNAVAILABLE,
            "agent-workspace-error-unavailable",
        ),
        Error::Conflict => (StatusCode::CONFLICT, "agent-workspace-error-conflict"),
        Error::TooLarge => (
            StatusCode::PAYLOAD_TOO_LARGE,
            "agent-workspace-error-too-large",
        ),
        Error::InvalidRequest | Error::NotFound => {
            (StatusCode::BAD_REQUEST, "agent-workspace-error-invalid")
        }
        Error::Failed => (StatusCode::BAD_GATEWAY, "agent-workspace-error-failed"),
    };
    json_error(status, "browser_control_failed", &t(lang, key))
}

fn private(mut response: Response) -> Response {
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        rama::http::HeaderValue::from_static("no-store, private"),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn browser_handover_accepts_only_the_configured_origin() {
        assert!(origin_matches(
            "https://plane.example",
            "https://plane.example/"
        ));
        assert!(!origin_matches(
            "http://plane.example",
            "https://plane.example"
        ));
        assert!(!origin_matches(
            "https://evil.example",
            "https://plane.example"
        ));
        assert!(!origin_matches(
            "https://plane.example.evil.example",
            "https://plane.example"
        ));
        assert!(!origin_matches(
            "https://plane.example/other",
            "https://plane.example"
        ));
        assert!(!origin_matches("null", "https://plane.example"));
    }

    #[test]
    fn every_browser_control_response_is_private_and_uncached() {
        let response = private(json_ok(StatusCode::OK, json!({"frame_b64": "frame"})));
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-store, private"
        );
    }
}
