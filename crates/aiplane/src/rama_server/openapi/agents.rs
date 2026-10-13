// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Agents, their tests, inbox, system principals, embed keys and the architect.
//!
//! Every route but the inbox's needs the agent-management permission on top
//! of the session (403 without it); a route on one agent also needs a share
//! on it, answering 404 for an agent the caller cannot see and 403 for one
//! they may read but not change.

use super::*;
use crate::rama_server::pages::architect;
use crate::rama_server::pages::json_agent_activity as activity;
use crate::rama_server::pages::json_agent_assist as assist;
use crate::rama_server::pages::json_agent_resources as resources;
use crate::rama_server::pages::json_agent_test as test_chat;
use crate::rama_server::pages::json_agent_tests as tests;
use crate::rama_server::pages::json_agents as agents;
use crate::rama_server::pages::json_embed_keys as embed_keys;
use crate::rama_server::pages::json_inbox as inbox;
use crate::rama_server::pages::json_organizations as organizations;
use crate::rama_server::pages::json_principals as principals;

pub(super) fn operations() -> Vec<Op> {
    vec![
        get("/api/v0/admin/organizations", Access::Admin)
            .ok(json::<organizations::AdminOrganizationList>())
            .errors(&[404]),
        post("/api/v0/admin/organizations", Access::Admin)
            .body(json::<organizations::OrganizationBody>())
            .created(json::<organizations::AdminOrganizationReply>())
            .errors(&[400, 404, 413]),
        put("/api/v0/admin/organizations/{id}", Access::Admin)
            .body(json::<organizations::OrganizationEditBody>())
            .ok(json::<organizations::AdminOrganizationReply>())
            .errors(&[400, 404, 409, 413]),
        get("/api/v0/organizations", Access::Session)
            .ok(json::<organizations::OrganizationList>())
            .errors(&[404]),
        get("/api/v0/organizations/{id}", Access::Session)
            .ok(json::<organizations::OrganizationDetail>())
            .errors(&[400, 404]),
        post("/api/v0/organizations/{id}/dots", Access::Session)
            .body(json::<organizations::OrganizationDotBody>())
            .created(json::<organizations::OrganizationDotReply>())
            .errors(&[400, 403, 404, 409, 413]),
        post(
            "/api/v0/organizations/{id}/dots/{dot}/active",
            Access::Session,
        )
        .body(json::<organizations::OrganizationDotActiveBody>())
        .ok(json::<organizations::OrganizationDotReply>())
        .errors(&[400, 404, 409, 413]),
        get("/api/v0/system-principals", Access::Session)
            .ok(json::<principals::PrincipalList>())
            .errors(&[403]),
        post("/api/v0/system-principals", Access::Session)
            .body(json::<principals::CreateBody>())
            .created(json::<principals::CreatedPrincipal>())
            .errors(&[400, 403, 409]),
        get("/api/v0/system-principals/{id}", Access::Session)
            .ok(json::<principals::PrincipalDetailReply>())
            .errors(&[400, 403, 404]),
        post("/api/v0/system-principals/{id}/disable", Access::Session)
            .ok(json::<principals::DisabledPrincipal>())
            .errors(&[400, 403, 404]),
        post("/api/v0/system-principals/{id}/grants", Access::Session)
            .body(json::<principals::GrantBody>())
            .created(json::<principals::GrantOutcome>())
            .ok(json::<principals::GrantOutcome>())
            .errors(&[400, 403, 404]),
        post(
            "/api/v0/system-principals/{id}/grants/revoke",
            Access::Session,
        )
        .body(json::<principals::GrantBody>())
        .no_content()
        .errors(&[400, 403, 404]),
        post("/api/v0/system-principals/{id}/tokens", Access::Session)
            .body(json::<principals::TokenBody>())
            .created(json::<principals::IssuedSystemToken>())
            .errors(&[400, 403, 404, 409]),
        post(
            "/api/v0/system-principals/{id}/tokens/{token_id}/revoke",
            Access::Session,
        )
        .no_content()
        .errors(&[400, 403, 404]),
        get("/api/v0/agent-resources", Access::Session)
            .query::<resources::AgentResourcesQuery>()
            .ok(json::<resources::AgentResources>())
            .errors(&[403, 404]),
        post("/api/v0/agent-architect", Access::Session)
            .body(json::<architect::StartBody>())
            .ok(json::<architect::ArchitectConversation>())
            .created(json::<architect::ArchitectConversation>())
            .errors(&[400, 403, 404, 503]),
        get(
            "/api/v0/agent-architect/{session_id}/dot-plan",
            Access::Session,
        )
        .ok(json::<architect::DotPlanReply>())
        .errors(&[403, 404]),
        post(
            "/api/v0/agent-architect/{session_id}/dot-plan",
            Access::Session,
        )
        .body(json::<architect::ApplyDotPlanBody>())
        .ok(json::<architect::AppliedDotPlan>())
        .created(json::<architect::AppliedDotPlan>())
        .errors(&[400, 403, 404, 409]),
        get("/api/v0/agents/inbox", Access::Session)
            .ok(json::<aiplane_runtime::agents::inbox::InboxList>()),
        get("/api/v0/agents/inbox/events", Access::Session).ok(sse::<inbox::InboxEvent>()),
        post("/api/v0/agents/inbox/{id}/answer", Access::Session)
            .body(json::<inbox::AnswerBody>())
            .accepted(json::<inbox::Resumed>())
            .errors(&[400, 403, 404, 409, 503]),
        get("/api/v0/dots", Access::Session)
            .ok(json::<aiplane_api::pages::json_dots::DotList>())
            .errors(&[403]),
        post("/api/v0/dots", Access::Session)
            .body(json::<aiplane_api::pages::json_dots::CreateDotBody>())
            .created(json::<serde_json::Value>())
            .errors(&[400, 403, 409]),
        put("/api/v0/dots/{id}/active", Access::Session)
            .body(json::<aiplane_api::pages::json_dots::ActiveBody>())
            .ok(json::<serde_json::Value>())
            .errors(&[400, 403, 404, 409]),
        get("/api/v0/dots/{id}/workspace", Access::Session)
            .ok(json::<
                aiplane_api::pages::json_dot_workspace::DotWorkspaceView,
            >())
            .errors(&[400, 404, 409, 413, 503]),
        get("/api/v0/dots/{id}/workspace/file", Access::Session)
            .ok(Content::Bytes("application/octet-stream"))
            .errors(&[400, 404, 409, 413, 503]),
        put("/api/v0/dots/{id}/workspace/file", Access::Session)
            .body(Content::Bytes("application/octet-stream"))
            .no_content()
            .errors(&[400, 404, 409, 413, 503]),
        post("/api/v0/dots/{id}/workspace/folder", Access::Session)
            .body(json::<
                aiplane_api::pages::json_dot_workspace::WorkspaceFolderBody,
            >())
            .no_content()
            .errors(&[400, 404, 409, 413, 503]),
        delete("/api/v0/dots/{id}/workspace/entry", Access::Session)
            .no_content()
            .errors(&[400, 404, 409, 413, 503]),
        get("/api/v0/dots/{id}/access", Access::Session)
            .ok(json::<aiplane_api::pages::json_dots::DotAccessView>())
            .errors(&[400, 404]),
        get("/api/v0/dots/{id}/connections", Access::Session)
            .ok(json::<aiplane_api::pages::json_dots::DotConnectionsView>())
            .errors(&[400, 404]),
        put("/api/v0/dots/{id}/connections/{key}", Access::Session)
            .body(json::<
                aiplane_api::pages::json_dots::DotConnectionSelectionBody,
            >())
            .ok(json::<aiplane_api::pages::json_dots::DotConnectionsView>())
            .errors(&[400, 403, 404, 409, 413]),
        delete("/api/v0/dots/{id}/connections/{key}", Access::Session)
            .body(json::<
                aiplane_api::pages::json_dots::DotConnectionVersionBody,
            >())
            .ok(json::<aiplane_api::pages::json_dots::DotConnectionsView>())
            .errors(&[400, 403, 404, 409, 413]),
        post(
            "/api/v0/dots/{id}/connections/{key}/connect",
            Access::Session,
        )
        .body(json::<
            aiplane_api::pages::json_dots::DotConnectionVersionBody,
        >())
        .ok(json::<aiplane_api::pages::json_dots::DotAuthorizationView>())
        .errors(&[400, 403, 404, 409, 413, 502]),
        get("/api/v0/dots/{id}/communication", Access::Session)
            .ok(json::<aiplane_api::pages::json_dots::DotCommunicationView>())
            .errors(&[400, 404]),
        put("/api/v0/dots/{id}/communication", Access::Session)
            .body(json::<aiplane_api::pages::json_dots::DotCommunicationBody>())
            .ok(json::<aiplane_api::pages::json_dots::DotCommunicationView>())
            .errors(&[400, 403, 404, 409, 413]),
        put("/api/v0/dots/{id}/access", Access::Session)
            .body(json::<aiplane_api::pages::json_dots::DotAccessBody>())
            .ok(json::<aiplane_api::pages::json_dots::DotAccessView>())
            .errors(&[400, 404, 409]),
        post("/api/v0/dots/{id}/sessions", Access::Session)
            .created(json::<aiplane_api::pages::chat::json_api::SessionEnvelope>())
            .errors(&[400, 403, 404, 409, 500]),
        get("/api/v0/dots/{id}/memories", Access::Session)
            .ok(json::<aiplane_api::pages::json_dots::DotMemoryList>())
            .errors(&[400, 404]),
        post("/api/v0/dots/{id}/memories", Access::Session)
            .body(json::<aiplane_api::pages::json_dots::DotMemoryBody>())
            .created(json::<aiplane_api::pages::json_dots::DotMemoryView>())
            .errors(&[400, 404]),
        put("/api/v0/dots/{id}/memories/{memory_id}", Access::Session)
            .body(json::<aiplane_api::pages::json_dots::DotMemoryBody>())
            .ok(json::<aiplane_api::pages::json_dots::DotMemoryView>())
            .errors(&[400, 404]),
        delete("/api/v0/dots/{id}/memories/{memory_id}", Access::Session)
            .no_content()
            .errors(&[400, 404]),
        get("/api/v0/dots/{id}/activity", Access::Session)
            .ok(json::<
                aiplane_api::pages::json_agent_activity::ActivityPageView,
            >())
            .errors(&[400, 404]),
        get("/api/v0/agents", Access::Session)
            .ok(json::<agents::AgentList>())
            .errors(&[403]),
        post("/api/v0/agents", Access::Session)
            .body(json::<agents::CreateBody>())
            .created(json::<agents::CreatedAgentReply>())
            .errors(&[400, 403, 409, 422]),
        get("/api/v0/agents/{id}", Access::Session)
            .ok(json::<agents::AgentDetailReply>())
            .errors(&[400, 403, 404]),
        delete("/api/v0/agents/{id}", Access::Session)
            .no_content()
            .errors(&[400, 403, 404]),
        put("/api/v0/agents/{id}/draft", Access::Session)
            .body(json::<agents::DraftBody>())
            .ok(json::<agents::SavedDraft>())
            .errors(&[400, 403, 404, 409, 422]),
        post("/api/v0/agents/{id}/draft/restore", Access::Session)
            .body(json::<agents::RestoreBody>())
            .ok(json::<agents::RestoredDraft>())
            .errors(&[400, 403, 404, 409, 422]),
        post("/api/v0/agents/{id}/publish", Access::Session)
            .created(json::<agents::Published>())
            .errors(&[400, 403, 404, 409, 422]),
        post("/api/v0/agents/{id}/test/messages", Access::Session)
            .body(json::<test_chat::TestMessageBody>())
            .accepted(json::<test_chat::TestTurnStarted>())
            .errors(&[400, 403, 404, 409, 422, 503]),
        get("/api/v0/agents/{id}/test/{session}/events", Access::Session)
            .ok(sse::<session_core::chat_json::ChatEvent>())
            .errors(&[400, 403, 404]),
        post("/api/v0/agents/{id}/test/{session}/cancel", Access::Session)
            .body(json::<test_chat::PrivateTurnCancelBody>())
            .ok(json::<crate::rama_server::pages::Done>())
            .errors(&[400, 403, 404]),
        get(
            "/api/v0/agents/{id}/test/{session}/turns/{turn}/debug",
            Access::Session,
        )
        .ok(json::<test_chat::TurnDebugView>())
        .errors(&[400, 403, 404, 409]),
        post("/api/v0/agents/{id}/assist/suggest", Access::Session)
            .body(json::<assist::SuggestBody>())
            .ok(json::<aiplane_runtime::agents::assist::Suggested>())
            .errors(&[400, 403, 404, 429, 502, 503]),
        post("/api/v0/agents/{id}/assist/improve", Access::Session)
            .body(json::<assist::ImproveBody>())
            .ok(json::<aiplane_runtime::agents::assist::ImprovedText>())
            .errors(&[400, 403, 404, 429, 502, 503]),
        post(
            "/api/v0/agents/{id}/conversations/{session}/turns/{turn}/resume",
            Access::Session,
        )
        .body(json::<test_chat::StaffResumeBody>())
        .accepted(json::<inbox::Resumed>())
        .errors(&[400, 403, 404, 409, 503]),
        get("/api/v0/agents/{id}/tests", Access::Session)
            .ok(json::<tests::TestSuite>())
            .errors(&[400, 403, 404]),
        post("/api/v0/agents/{id}/tests", Access::Session)
            .body(json::<tests::CaseDto>())
            .created(json::<tests::CaseReply>())
            .errors(&[400, 403, 404, 409, 422]),
        post("/api/v0/agents/{id}/tests/run", Access::Session)
            .body(json::<tests::RunBody>())
            .created(json::<tests::RunDetail>())
            .errors(&[400, 403, 404]),
        put("/api/v0/agents/{id}/tests/{case}", Access::Session)
            .body(json::<tests::CaseDto>())
            .ok(json::<tests::CaseReply>())
            .errors(&[400, 403, 404, 409, 422]),
        delete("/api/v0/agents/{id}/tests/{case}", Access::Session)
            .no_content()
            .errors(&[400, 403, 404]),
        get("/api/v0/agents/{id}/test-runs", Access::Session)
            .ok(json::<tests::RunList>())
            .errors(&[400, 403, 404]),
        get("/api/v0/agents/{id}/test-runs/{run}", Access::Session)
            .ok(json::<tests::RunDetail>())
            .errors(&[400, 403, 404]),
        get("/api/v0/agents/{id}/versions", Access::Session)
            .ok(json::<agents::VersionList>())
            .errors(&[400, 403, 404]),
        get("/api/v0/agents/{id}/analytics", Access::Session)
            .param("from", Scalar::String, false)
            .param("to", Scalar::String, false)
            .param("version", Scalar::Integer, false)
            .ok(json::<agents::AnalyticsView>())
            .errors(&[400, 403, 404]),
        get("/api/v0/agents/{id}/activity", Access::Session)
            .param("conversation", Scalar::String, false)
            .param("kind", Scalar::String, false)
            .param("from", Scalar::String, false)
            .param("to", Scalar::String, false)
            .param("cursor", Scalar::Integer, false)
            .param("order", Scalar::String, false)
            .param("limit", Scalar::Integer, false)
            .ok(json::<activity::ActivityPageView>())
            .errors(&[400, 403, 404]),
        get("/api/v0/agents/{id}/activity/export", Access::Session)
            .param("conversation", Scalar::String, false)
            .param("kind", Scalar::String, false)
            .param("from", Scalar::String, false)
            .param("to", Scalar::String, false)
            .ok(Content::Text("application/x-ndjson"))
            .errors(&[400, 403, 404]),
        get("/api/v0/agents/{id}/activity/verify", Access::Session)
            .param("full", Scalar::Boolean, false)
            .ok(json::<activity::Verified>())
            .errors(&[400, 403, 404]),
        post("/api/v0/agents/{id}/live", Access::Session)
            .body(json::<agents::LiveBody>())
            .ok(json::<agents::LiveVersion>())
            .errors(&[400, 403, 404]),
        get("/api/v0/agents/{id}/shares", Access::Session)
            .ok(json::<agents::ShareList>())
            .errors(&[400, 403, 404]),
        get("/api/v0/agents/{id}/communication", Access::Session)
            .ok(json::<agents::AgentCommunicationView>())
            .errors(&[400, 403, 404]),
        put("/api/v0/agents/{id}/communication", Access::Session)
            .body(json::<agents::AgentCommunicationBody>())
            .ok(json::<agents::AgentCommunicationView>())
            .errors(&[400, 403, 404, 409]),
        post("/api/v0/agents/{id}/shares", Access::Session)
            .body(json::<agents::ShareBody>())
            .created(json::<agents::ShareSet>())
            .ok(json::<agents::ShareSet>())
            .errors(&[400, 403, 404, 409, 422]),
        post("/api/v0/agents/{id}/shares/revoke", Access::Session)
            .body(json::<agents::ShareBody>())
            .no_content()
            .errors(&[400, 403, 404, 409]),
        get("/api/v0/agents/{id}/share-subjects", Access::Session)
            .param("q", Scalar::String, false)
            .ok(json::<agents::ShareSubjects>())
            .errors(&[400, 403, 404]),
        get("/api/v0/agents/{id}/channels", Access::Session)
            .ok(json::<inbox::ChannelList>())
            .errors(&[400, 403, 404]),
        post("/api/v0/agents/{id}/channels", Access::Session)
            .body(json::<inbox::ChannelBody>())
            .created(json::<inbox::CreatedChannel>())
            .errors(&[400, 403, 404, 409, 422]),
        delete("/api/v0/agents/{id}/channels/{channel_id}", Access::Session)
            .no_content()
            .errors(&[400, 403, 404]),
        get("/api/v0/agents/{id}/embed-keys", Access::Session)
            .ok(json::<embed_keys::EmbedKeyList>())
            .errors(&[400, 403, 404]),
        post("/api/v0/agents/{id}/embed-keys", Access::Session)
            .body(json::<embed_keys::EmbedKeyBody>())
            .created(json::<embed_keys::CreatedEmbedKey>())
            .errors(&[400, 403, 404]),
        post(
            "/api/v0/agents/{id}/embed-keys/{key_id}/revoke",
            Access::Session,
        )
        .no_content()
        .errors(&[400, 403, 404]),
    ]
}
