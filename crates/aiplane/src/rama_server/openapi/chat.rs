// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Conversations and their turns, and the public embed widget's visitor conversations.

use super::*;
use crate::rama_server::pages::chat::json_api as chat;
use crate::rama_server::pages::embed;
use session_core::chat_json::ChatEvent;

/// The parts `parse_chat_submit` reads from a multipart message.
const SUBMIT_PARTS: &[Part] = &[
    Part {
        name: "model",
        kind: PartKind::Text,
        required: true,
    },
    Part {
        name: "message",
        kind: PartKind::Text,
        required: false,
    },
    Part {
        name: "voice",
        kind: PartKind::Text,
        required: false,
    },
    Part {
        name: "attachment",
        kind: PartKind::Files,
        required: false,
    },
];

pub(super) fn operations() -> Vec<Op> {
    vec![
        post("/api/v0/embed/sessions", Access::EmbedKey)
            .body(json::<embed::StartBody>())
            .created(json::<embed::VisitorSessionStarted>())
            .errors(&[400, 401, 403, 409, 429, 500, 503]),
        get("/api/v0/embed/session", Access::Visitor)
            .ok(json::<embed::VisitorConversation>())
            .errors(&[403, 409, 500]),
        post("/api/v0/embed/messages", Access::Visitor)
            .body(json::<embed::MessageBody>())
            .accepted(json::<embed::VisitorMessageAccepted>())
            .errors(&[400, 403, 409, 429, 500, 503]),
        get("/api/v0/embed/events", Access::Visitor)
            .ok(sse::<ChatEvent>())
            .errors(&[403, 409, 500]),
        post("/api/v0/embed/resume", Access::Visitor)
            .body(json::<embed::ResumeBody>())
            .accepted(json::<embed::VisitorTurnResumed>())
            .errors(&[400, 403, 409, 429, 500, 503]),
        post("/api/v0/embed/identity", Access::Visitor)
            .body(json::<embed::IdentityBody>())
            .ok(json::<embed::IdentityAccepted>())
            .errors(&[400, 403, 409, 422, 500, 503]),
        post("/api/v0/embed/agent", Access::EmbedKey)
            .body(json::<embed::StartBody>())
            .ok(json::<embed::AgentDescribed>())
            .errors(&[400, 401, 403, 409, 500]),
        post("/api/v0/embed/transcribe", Access::Visitor)
            .body(Content::Bytes("audio/wav"))
            .ok(json::<embed::Transcript>())
            .errors(&[400, 403, 404, 409, 415, 429, 500, 503]),
        post("/api/v0/embed/speak", Access::Visitor)
            .body(json::<embed::SpeakBody>())
            .ok(Content::Bytes("audio/mpeg"))
            .no_content()
            .errors(&[400, 403, 404, 409, 429, 500, 503]),
        get("/api/v0/embed/recorder.js", Access::Public).ok(Content::Text("text/javascript")),
        get("/api/v0/chat/landing", Access::Session).ok(json::<chat::SessionEnvelope>()),
        get("/api/v0/chat/sessions", Access::Session)
            .query::<chat::SessionsQuery>()
            .ok(json::<chat::SessionsList>()),
        post("/api/v0/chat/sessions", Access::Session)
            .ok(json::<chat::SessionEnvelope>())
            .created(json::<chat::SessionEnvelope>()),
        get("/api/v0/chat/sessions/{id}", Access::Session)
            .ok(json::<chat::SessionDetail>())
            .errors(&[404]),
        delete("/api/v0/chat/sessions/{id}", Access::Session)
            .no_content()
            .errors(&[404]),
        post("/api/v0/chat/sessions/{id}/pin", Access::Session)
            .body(json::<chat::PinBody>())
            .ok(json::<chat::Pinned>())
            .errors(&[400, 404]),
        post("/api/v0/chat/sessions/{id}/messages", Access::Session)
            .body(json::<chat::MessageBody>())
            .body(Content::Multipart(SUBMIT_PARTS))
            .accepted(json::<chat::Submitted>())
            .errors(&[400, 404, 409, 429]),
        post("/api/v0/chat/sessions/{id}/steer", Access::Session)
            .body(json::<chat::SteerBody>())
            .accepted(json::<chat::SteerAccepted>())
            .errors(&[400, 404, 409, 429]),
        post(
            "/api/v0/chat/sessions/{id}/steer/{steer_id}/discard",
            Access::Session,
        )
        .ok(json::<chat::SteerDiscarded>())
        .errors(&[404, 409]),
        post("/api/v0/chat/sessions/{id}/cancel", Access::Session)
            .ok(json::<chat::Cancelled>())
            .errors(&[404]),
        post(
            "/api/v0/chat/sessions/{id}/turns/{turn_id}/resume",
            Access::Session,
        )
        .body(json::<chat::ResumeBody>())
        .accepted(json::<chat::Resumed>())
        .errors(&[400, 404, 409]),
        post(
            "/api/v0/chat/sessions/{session}/turns/{turn}/browser-handover/{request}/claim",
            Access::Session,
        )
        .body(json::<
            aiplane_api::pages::json_dot_browser::BrowserControlBody,
        >())
        .ok(json::<
            aiplane_api::pages::json_dot_browser::BrowserControlView,
        >())
        .errors(&[400, 403, 404, 409, 413, 503]),
        post(
            "/api/v0/chat/sessions/{session}/turns/{turn}/browser-handover/{request}/frame",
            Access::Session,
        )
        .body(json::<
            aiplane_api::pages::json_dot_browser::BrowserControlBody,
        >())
        .ok(json::<
            aiplane_api::pages::json_dot_browser::BrowserControlView,
        >())
        .errors(&[400, 403, 404, 409, 503]),
        post(
            "/api/v0/chat/sessions/{session}/turns/{turn}/browser-handover/{request}/input",
            Access::Session,
        )
        .body(json::<
            aiplane_api::pages::json_dot_browser::BrowserControlBody,
        >())
        .ok(json::<
            aiplane_api::pages::json_dot_browser::BrowserControlView,
        >())
        .errors(&[400, 403, 404, 409, 503]),
        post(
            "/api/v0/chat/sessions/{session}/turns/{turn}/browser-handover/{request}/finish",
            Access::Session,
        )
        .body(json::<
            aiplane_api::pages::json_dot_browser::BrowserControlBody,
        >())
        .accepted(json::<
            aiplane_api::pages::json_dot_browser::BrowserHandoverReturned,
        >())
        .errors(&[400, 403, 404, 409, 503]),
        post(
            "/api/v0/chat/sessions/{session}/turns/{turn}/browser-handover/{request}/abort",
            Access::Session,
        )
        .body(json::<
            aiplane_api::pages::json_dot_browser::BrowserControlBody,
        >())
        .ok(json::<
            aiplane_api::pages::json_dot_browser::BrowserHandoverAborted,
        >())
        .errors(&[400, 403, 404, 409, 503]),
        get("/api/v0/chat/sessions/{id}/events", Access::Session)
            .ok(sse::<ChatEvent>())
            .errors(&[404]),
        post("/api/v0/chat/sessions/{id}/share", Access::Session)
            .body(json::<chat::ShareBody>())
            .ok(json::<chat::Shared>())
            .errors(&[400, 404]),
        post("/api/v0/chat/sessions/{id}/effort", Access::Session)
            .body(json::<chat::EffortBody>())
            .ok(json::<chat::EffortBody>())
            .errors(&[400, 404]),
        post("/api/v0/chat/sessions/{id}/confidential", Access::Session)
            .body(json::<chat::ConfidentialBody>())
            .ok(json::<chat::ConfidentialBody>())
            .errors(&[400, 404]),
        get("/api/v0/chat/sessions/{id}/capabilities", Access::Session)
            .ok(json::<chat::Capabilities>())
            .errors(&[404]),
        post("/api/v0/chat/sessions/{id}/capabilities", Access::Session)
            .body(json::<chat::CapabilityBody>())
            .ok(json::<chat::CapabilityBody>())
            .errors(&[400, 404]),
        put("/api/v0/tokens/{id}/models", Access::Session)
            .body(json::<chat::OwnerModelsBody>())
            .ok(json::<chat::OwnerModels>())
            .errors(&[400, 404]),
        post("/api/v0/tokens/{id}/quota", Access::Session)
            .body(json::<chat::OwnerQuotaBody>())
            .ok(json::<aiplane_api::pages::Done>())
            .errors(&[400, 404]),
        put("/api/v0/tokens/{id}/mcp-policy", Access::Session)
            .body(json::<chat::McpPolicyBody>())
            .ok(json::<chat::McpPolicyBody>())
            .errors(&[400, 404]),
        delete("/api/v0/tokens/{id}/quota/{rule_id}", Access::Session)
            .ok(json::<chat::QuotaRuleDeleted>())
            .errors(&[404]),
        get("/api/v0/chat/sessions/{id}/export.md", Access::Session)
            .ok(Content::Text("text/markdown"))
            .errors(&[404]),
        get("/api/v0/chat/sessions/{id}/export.pdf", Access::Session)
            .ok(Content::Bytes("application/pdf"))
            .errors(&[404, 503]),
        post("/api/v0/chat/sessions/{id}/fork", Access::Session)
            .created(json::<chat::Forked>())
            .errors(&[404, 409]),
        get("/api/v0/chat/sessions/{id}/documents", Access::Session)
            .ok(json::<chat::DocumentsList>())
            .errors(&[404]),
        get(
            "/api/v0/chat/sessions/{id}/documents/{doc_id}",
            Access::Session,
        )
        .param("version", Scalar::Integer, false)
        .ok(json::<chat::DocumentDetail>())
        .errors(&[404]),
        put(
            "/api/v0/chat/sessions/{id}/documents/{doc_id}",
            Access::Session,
        )
        .body(json::<chat::DocumentEditBody>())
        .ok(json::<chat::DocumentSaved>())
        .errors(&[400, 404]),
        delete(
            "/api/v0/chat/sessions/{id}/turns/{turn_id}/attachments/{filename}",
            Access::Session,
        )
        .ok(json::<chat::AttachmentRemoved>())
        .errors(&[400, 404]),
        delete(
            "/api/v0/chat/sessions/{id}/turns/{turn_id}",
            Access::Session,
        )
        .ok(json::<chat::TurnDeleted>())
        .errors(&[400, 404, 409]),
        post(
            "/api/v0/chat/sessions/{id}/turns/{turn_id}/retry",
            Access::Session,
        )
        .body(json::<chat::RetryBody>())
        .accepted(json::<chat::Regenerated>())
        .errors(&[400, 404, 409]),
        post(
            "/api/v0/chat/sessions/{id}/turns/{turn_id}/edit",
            Access::Session,
        )
        .body(json::<chat::EditBody>())
        .body(Content::Multipart(SUBMIT_PARTS))
        .accepted(json::<chat::Regenerated>())
        .errors(&[400, 404, 409]),
    ]
}
