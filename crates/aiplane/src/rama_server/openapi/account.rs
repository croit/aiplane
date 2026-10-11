// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The signed-in user's own account: profile, tokens, preferences, voice, push, feedback, tools, ComfyUI, setup, build.

use super::*;
use crate::rama_server::pages::{feedback, json_tokens, tools};
use crate::rama_server::{api, comfyui_api as comfyui, proxy, setup_api as setup};

pub(super) fn operations() -> Vec<Op> {
    vec![
        get("/api/v0/build", Access::Public).ok(json::<aiplane_api::build_info::BuildMetadata>()),
        get("/api/v0/me", Access::Session).ok(json::<shared::api::Me>()),
        get("/api/v0/tokens", Access::Session).ok(json::<Vec<shared::api::TokenSummary>>()),
        post("/api/v0/tokens", Access::Session)
            .body(json::<api::CreateTokenRequest>())
            .ok(json::<shared::api::CreateTokenResponse>())
            .errors(&[400, 403]),
        post("/api/v0/tokens/{id}/revoke", Access::Session)
            .ok(json::<shared::api::RevokeResponse>())
            .errors(&[404, 409]),
        post("/api/v0/tokens/{id}/rotate", Access::Session)
            .ok(json::<shared::api::CreateTokenResponse>())
            .errors(&[404]),
        put("/api/v0/tokens/{id}/tools", Access::Session)
            .body(json::<shared::api::UpdateTokenToolsRequest>())
            .ok(json::<api::TokenToolsSaved>())
            .errors(&[400, 404]),
        put("/api/v0/tokens/{id}/priority", Access::Session)
            .body(json::<json_tokens::TokenPriorityRequest>())
            .ok(json::<json_tokens::TokenPriorityRequest>())
            .errors(&[400, 403, 404]),
        delete("/api/v0/tokens/{id}", Access::Session)
            .ok(json::<shared::api::DeleteResponse>())
            .errors(&[404, 409]),
        post("/api/v0/transcriptions", Access::Session)
            .body(Content::Multipart(&[
                Part {
                    name: "model",
                    kind: PartKind::Text,
                    required: true,
                },
                Part {
                    name: "file",
                    kind: PartKind::File,
                    required: true,
                },
            ]))
            .empty(200)
            .errors(&[400, 404, 502, 503])
            .unsupported(
                "the answer is the transcription backend's own body, relayed unchanged; the \
                 gateway has no type for it",
            ),
        get("/api/v0/transcription_models", Access::Session).ok(json::<api::VoiceModels>()),
        post("/api/v0/speech", Access::Session)
            .body(json::<proxy::SpeechBody>())
            .ok(Content::Bytes("audio/mpeg"))
            .no_content()
            .errors(&[400, 404, 502, 503]),
        get("/api/v0/push/config", Access::Session).ok(json::<api::PushConfig>()),
        post("/api/v0/push/subscribe", Access::Session)
            .body(json::<api::PushSubscribeBody>())
            .ok(json::<aiplane_api::pages::Done>())
            .errors(&[400, 503]),
        post("/api/v0/push/unsubscribe", Access::Session)
            .body(json::<api::PushUnsubscribeBody>())
            .ok(json::<aiplane_api::pages::Done>())
            .errors(&[400]),
        post("/api/v0/me/timezone", Access::Session)
            .body(json::<api::TimezoneBody>())
            .ok(json::<api::TimezoneSaved>())
            .errors(&[400]),
        post("/api/v0/me/speech_voice", Access::Session)
            .body(json::<api::SpeechVoiceBody>())
            .ok(json::<api::SpeechVoiceSaved>())
            .errors(&[400]),
        post("/api/v0/me/location", Access::Session)
            .body(json::<api::LocationBody>())
            .ok(json::<aiplane_api::pages::Done>())
            .errors(&[400]),
        delete("/api/v0/me/location", Access::Session).ok(json::<aiplane_api::pages::Done>()),
        post("/api/v0/me/location/feedback/{turn_id}", Access::Session)
            .body(json::<api::LocationFeedbackBody>())
            .ok(json::<aiplane_api::pages::Done>())
            .errors(&[400, 404]),
        post("/api/v0/me/ask/feedback/{turn_id}", Access::Session)
            .body(json::<api::AskFeedbackBody>())
            .ok(json::<aiplane_api::pages::Done>())
            .errors(&[400, 404]),
        post("/api/v0/me/browser/feedback/{turn_id}", Access::Session)
            .body(json::<api::BrowserFeedbackBody>())
            .ok(json::<aiplane_api::pages::Done>())
            .errors(&[400, 404]),
        get(
            "/api/v0/chat/attachment/{turn_id}/{filename}",
            Access::Session,
        )
        .ok(Content::Bytes("*/*"))
        .errors(&[400, 404, 502, 503]),
        get("/api/v0/feedback/config", Access::Session).ok(json::<feedback::FeedbackConfigView>()),
        post("/api/v0/feedback/extract", Access::Session)
            .body(json::<feedback::ExtractRequest>())
            .ok(json::<feedback::ExtractedFields>())
            .errors(&[400, 429, 502, 503]),
        post("/api/v0/feedback", Access::Session)
            .body(json::<feedback::SubmitRequest>())
            .ok(json::<feedback::FiledIssue>())
            .errors(&[400, 502, 503]),
        post("/api/v0/comfyui/reload", Access::Admin)
            .ok(json::<comfyui::ReloadResponse>())
            .errors(&[503]),
        get("/api/v0/comfyui/catalog", Access::Admin).ok(json::<comfyui::CatalogResponse>()),
        get("/api/v0/comfyui/health", Access::Admin)
            .ok(json::<comfyui::HealthResponse>())
            .errors(&[503]),
        get("/api/v0/models", Access::Session).ok(json::<api::ChatModels>()),
        get("/api/v0/usage", Access::Session)
            .query::<api::UsageQuery>()
            .ok(json::<api::UsageView>()),
        get("/api/v0/setup/state", Access::Setup)
            .ok(json::<setup::SetupState>())
            .errors(&[403, 404, 500]),
        post("/api/v0/setup/test", Access::Setup)
            .body(json::<setup::SetupTestBody>())
            .ok(json::<setup::SetupTestStarted>())
            .errors(&[400, 403, 404, 500, 502]),
        post("/api/v0/setup/restart", Access::Setup)
            .ok(json::<aiplane_api::pages::Done>())
            .errors(&[403, 404, 500]),
        post("/api/v0/setup/finish", Access::Setup)
            .body(json::<setup::SetupFinishBody>())
            .ok(json::<setup::SetupFinished>())
            .errors(&[400, 403, 404, 409, 500, 502]),
        get("/api/v0/tools", Access::Session).ok(json::<tools::ToolsView>()),
        post("/api/v0/tools/toggle", Access::Session)
            .body(json::<tools::ToolsToggleBody>())
            .ok(json::<tools::ToolToggled>())
            .errors(&[400, 404]),
        get("/api/v0/tokens/details", Access::Session).ok(json::<json_tokens::TokenDetails>()),
    ]
}
