// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Skills, connectors, integrations, memories, scheduled actions and webhooks.

use super::*;
use crate::rama_server::pages::json_skills as skills;
use crate::rama_server::pages::json_workspace as workspace;

const SKILL_ARCHIVE: &[Part] = &[Part {
    name: "file",
    kind: PartKind::File,
    required: true,
}];

pub(super) fn operations() -> Vec<Op> {
    vec![
        get("/api/v0/skills", Access::Session).ok(json::<skills::SkillList>()),
        post("/api/v0/skills", Access::Session)
            .body(json::<skills::InlineSkillBody>())
            .body(Content::Multipart(SKILL_ARCHIVE))
            .created(json::<skills::SkillSaved>())
            .errors(&[400, 404, 409, 503]),
        post("/api/v0/skills/draft", Access::Session)
            .body(json::<skills::WorkflowDraftRequest>())
            .ok(json::<skills::WorkflowDraft>())
            .errors(&[400, 429, 502, 503]),
        get("/api/v0/skills/{name}/body", Access::Session)
            .ok(json::<skills::SkillSource>())
            .errors(&[400, 404]),
        get("/api/v0/skills/{name}/archive", Access::Session)
            .ok(Content::Bytes("application/zip"))
            .errors(&[400, 404]),
        delete("/api/v0/skills/{name}", Access::Session)
            .no_content()
            .errors(&[400, 404, 503]),
        get("/api/v0/admin/skills", Access::Admin).ok(json::<skills::AdminSkillList>()),
        post("/api/v0/admin/skills", Access::Admin)
            .body(Content::Multipart(SKILL_ARCHIVE))
            .created(json::<skills::SkillSaved>())
            .errors(&[400, 503]),
        get("/api/v0/admin/skills/{name}/archive", Access::Admin)
            .ok(Content::Bytes("application/zip"))
            .errors(&[400, 404, 503]),
        delete("/api/v0/admin/skills/{name}", Access::Admin)
            .no_content()
            .errors(&[400, 404, 503]),
        put("/api/v0/admin/skills/grants", Access::Admin)
            .body(json::<skills::SkillGrantsBody>())
            .ok(json::<skills::SkillGrantsBody>())
            .errors(&[400, 404, 503]),
        get("/api/v0/admin/connectors", Access::Admin).ok(json::<skills::ConnectorList>()),
        put("/api/v0/admin/connectors", Access::Admin)
            .body(json::<skills::ConnectorInputBody>())
            .ok(json::<skills::ConnectorKey>())
            .errors(&[400, 409]),
        post("/api/v0/admin/connectors/restore-defaults", Access::Admin)
            .ok(json::<skills::ConnectorsSeeded>()),
        post("/api/v0/admin/connectors/{key}/toggle", Access::Admin)
            .body(json::<workspace::EnabledBody>())
            .ok(json::<skills::ConnectorToggled>())
            .errors(&[400, 404]),
        get("/api/v0/admin/connectors/{key}/audit", Access::Admin)
            .ok(json::<skills::ConnectorAudit>())
            .errors(&[400, 404]),
        delete("/api/v0/admin/connectors/{key}", Access::Admin)
            .no_content()
            .errors(&[400, 404]),
        get("/api/v0/integrations", Access::Session).ok(json::<skills::IntegrationList>()),
        post("/api/v0/integrations/{key}/token", Access::Session)
            .body(json::<skills::TokenConnectBody>())
            .ok(json::<skills::IntegrationConnected>())
            .errors(&[400, 403, 404, 409]),
        post("/api/v0/integrations/{key}/disconnect", Access::Session)
            .no_content()
            .errors(&[400, 404]),
        post("/api/v0/integrations/{key}/retry", Access::Session)
            .no_content()
            .errors(&[400, 403, 404]),
        post("/api/v0/integrations/{key}/tools/mode", Access::Session)
            .body(json::<skills::ToolModeBody>())
            .no_content()
            .errors(&[400, 403, 404]),
        post("/api/v0/integrations/{key}/tools/all", Access::Session)
            .body(json::<skills::ToolsAllBody>())
            .no_content()
            .errors(&[400, 404, 502]),
        get("/api/v0/memories", Access::Session).ok(json::<workspace::MemoryList>()),
        post("/api/v0/memories", Access::Session)
            .body(json::<workspace::MemoryBody>())
            .created(json::<workspace::MemoryView>())
            .errors(&[400]),
        put("/api/v0/memories/{id}", Access::Session)
            .body(json::<workspace::MemoryBody>())
            .ok(json::<workspace::MemoryView>())
            .errors(&[400, 404]),
        delete("/api/v0/memories/{id}", Access::Session)
            .no_content()
            .errors(&[404]),
        get("/api/v0/scheduled", Access::Session)
            .param("principal_id", Scalar::String, false)
            .ok(json::<workspace::ScheduledList>())
            .errors(&[400, 403, 404]),
        post("/api/v0/scheduled", Access::Session)
            .body(json::<workspace::ScheduledBody>())
            .created(json::<workspace::ActionView>())
            .errors(&[400, 403, 404]),
        post("/api/v0/scheduled/preview", Access::Session)
            .body(json::<workspace::CronPreviewBody>())
            .ok(json::<workspace::CronPreview>())
            .errors(&[400]),
        put("/api/v0/scheduled/{id}", Access::Session)
            .body(json::<workspace::ScheduledUpdateBody>())
            .ok(json::<workspace::Updated<workspace::ActionView>>())
            .errors(&[400, 403, 404, 409]),
        post("/api/v0/scheduled/{id}/toggle", Access::Session)
            .body(json::<workspace::ScheduledEnabledBody>())
            .ok(json::<workspace::ScheduledEnabledView>())
            .errors(&[400, 403, 404, 409]),
        delete("/api/v0/scheduled/{id}", Access::Session)
            .no_content()
            .errors(&[404, 409]),
        get("/api/v0/scheduled/{id}/runs", Access::Session)
            .ok(json::<workspace::ScheduledRuns>())
            .errors(&[404]),
        get("/api/v0/webhooks", Access::Session)
            .param("principal_id", Scalar::String, false)
            .ok(json::<workspace::WebhookList>())
            .errors(&[400, 403, 404]),
        post("/api/v0/webhooks", Access::Session)
            .body(json::<workspace::WebhookBody>())
            .created(json::<workspace::WebhookCreated>())
            .errors(&[400, 403, 404]),
        put("/api/v0/webhooks/{id}", Access::Session)
            .body(json::<workspace::WebhookUpdateBody>())
            .ok(json::<workspace::Updated<workspace::WebhookView>>())
            .errors(&[400, 403, 404, 409]),
        post("/api/v0/webhooks/{id}/toggle", Access::Session)
            .body(json::<workspace::WebhookEnabledBody>())
            .ok(json::<workspace::WebhookEnabledView>())
            .errors(&[400, 403, 404, 409]),
        post("/api/v0/webhooks/{id}/rotate", Access::Session)
            .body(json::<workspace::WebhookRotateBody>())
            .ok(json::<workspace::WebhookSecret>())
            .errors(&[400, 403, 404, 409]),
        delete("/api/v0/webhooks/{id}", Access::Session)
            .no_content()
            .errors(&[404, 409]),
        get("/api/v0/webhooks/{id}/runs", Access::Session)
            .ok(json::<workspace::WebhookRuns>())
            .errors(&[404]),
        post("/api/v0/webhooks/{id}/rerun", Access::Session)
            .body(json::<workspace::RerunBody>())
            .ok(json::<workspace::RerunOutcome>())
            .accepted(json::<workspace::RerunAccepted>())
            .errors(&[400, 404, 409, 502]),
    ]
}
