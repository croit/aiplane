// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The `/api/v0/admin/*` operator surfaces.

use super::*;
use crate::rama_server::pages::classification::types as classification;
use crate::rama_server::pages::{branding, json_admin as admin, json_tokens};

/// The parts the logo upload reads from its multipart body.
const LOGO_PARTS: &[Part] = &[
    Part {
        name: "file",
        kind: PartKind::File,
        required: true,
    },
    Part {
        name: "slot",
        kind: PartKind::Text,
        required: false,
    },
];

const SAMPLE_PARTS: &[Part] = &[
    Part {
        name: "file",
        kind: PartKind::File,
        required: false,
    },
    Part {
        name: "text",
        kind: PartKind::Text,
        required: false,
    },
];

const CLASSIFICATION_FILE_PARTS: &[Part] = &[Part {
    name: "file",
    kind: PartKind::File,
    required: true,
}];

pub(super) fn operations() -> Vec<Op> {
    vec![
        get("/api/v0/admin/classification", Access::Admin)
            .ok(json::<classification::ClassificationState>()),
        put("/api/v0/admin/classification", Access::Admin)
            .body(json::<classification::SaveBody>())
            .ok(json::<classification::ClassificationState>())
            .errors(&[400]),
        post(
            "/api/v0/admin/classification/classes/{id}/terms",
            Access::Admin,
        )
        .body(Content::Multipart(CLASSIFICATION_FILE_PARTS))
        .ok(json::<classification::TermsUploaded>())
        .errors(&[400, 404, 413]),
        delete(
            "/api/v0/admin/classification/classes/{id}/terms",
            Access::Admin,
        )
        .no_content()
        .errors(&[404]),
        post("/api/v0/admin/classification/inspect", Access::Admin)
            .body(Content::Multipart(CLASSIFICATION_FILE_PARTS))
            .ok(json::<classification::InspectResult>())
            .errors(&[400, 413]),
        post("/api/v0/admin/classification/test", Access::Admin)
            .body(Content::Multipart(SAMPLE_PARTS))
            .ok(json::<classification::TestResult>())
            .errors(&[400, 413, 502]),
        get("/api/v0/admin/classification/events", Access::Admin)
            .ok(json::<classification::EventsView>())
            .errors(&[400]),
        get("/api/v0/admin/groups", Access::Admin).ok(json::<admin::GroupsView>()),
        put("/api/v0/admin/groups", Access::Admin)
            .body(json::<admin::GroupSaveBody>())
            .ok(json::<admin::GroupSaved>())
            .errors(&[400]),
        delete("/api/v0/admin/groups/{name}", Access::Admin)
            .no_content()
            .errors(&[400, 404]),
        get("/api/v0/admin/users", Access::Admin).ok(json::<admin::UsersView>()),
        post("/api/v0/admin/users/{id}/impersonate", Access::Admin)
            .ok(json::<admin::Impersonating>())
            .errors(&[400, 404]),
        post("/api/v0/admin/impersonate/stop", Access::Session)
            .ok(json::<admin::ImpersonationStopped>()),
        get("/api/v0/admin/models", Access::Admin).ok(json::<admin::ModelsView>()),
        put("/api/v0/admin/models", Access::Admin)
            .body(json::<admin::ModelSaveBody>())
            .ok(json::<admin::ModelSaved>())
            .errors(&[400]),
        get("/api/v0/admin/automatic-routes", Access::Admin)
            .ok(json::<admin::AutomaticRoutesView>()),
        put("/api/v0/admin/automatic-routes", Access::Admin)
            .body(json::<
                aiplane_core::server::db::automatic_routes::AutomaticRoute,
            >())
            .ok(json::<admin::AutomaticRouteSaved>())
            .errors(&[400]),
        delete("/api/v0/admin/automatic-routes/{*alias}", Access::Admin)
            .no_content()
            .errors(&[400, 404]),
        delete("/api/v0/admin/models/{name}", Access::Admin)
            .no_content()
            .errors(&[400, 404]),
        put("/api/v0/admin/model-defaults", Access::Admin)
            .body(json::<admin::FeatureDefaultBody>())
            .ok(json::<admin::FeatureDefaultSaved>())
            .errors(&[400]),
        put("/api/v0/admin/search-settings", Access::Admin)
            .body(json::<admin::SearchSettingsBody>())
            .ok(json::<aiplane_api::pages::Done>())
            .errors(&[400]),
        get("/api/v0/admin/branding", Access::Admin).ok(json::<branding::BrandingState>()),
        post("/api/v0/admin/branding", Access::Admin)
            .body(Content::Multipart(LOGO_PARTS))
            .created(json::<aiplane_api::pages::Done>())
            .errors(&[400, 413]),
        delete("/api/v0/admin/branding/{slot}", Access::Admin)
            .ok(json::<aiplane_api::pages::Done>())
            .errors(&[400]),
        get("/api/v0/admin/limits", Access::Admin).ok(json::<admin::LimitsView>()),
        post("/api/v0/admin/limits", Access::Admin)
            .body(json::<admin::LimitBody>())
            .ok(json::<aiplane_api::pages::Done>())
            .errors(&[400, 404]),
        delete("/api/v0/admin/limits/{id}", Access::Admin)
            .no_content()
            .errors(&[404]),
        get("/api/v0/admin/settings", Access::Admin).ok(json::<admin::SettingsView>()),
        post("/api/v0/admin/settings", Access::Admin)
            .body(json::<admin::SettingsSaveBody>())
            .ok(json::<aiplane_api::pages::Done>())
            .errors(&[400, 422]),
        post("/api/v0/admin/settings/clear", Access::Admin)
            .body(json::<admin::SettingsClearBody>())
            .ok(json::<admin::SettingCleared>())
            .errors(&[400]),
        get("/api/v0/admin/tokens", Access::Admin).ok(json::<admin::AdminTokensView>()),
        put("/api/v0/admin/tokens/{id}/models", Access::Admin)
            .body(json::<admin::AdminTokenModelsBody>())
            .ok(json::<admin::AdminTokenModels>())
            .errors(&[400, 404]),
        put("/api/v0/admin/tokens/{id}/priority", Access::Admin)
            .body(json::<json_tokens::TokenPriorityRequest>())
            .ok(json::<json_tokens::TokenPriorityRequest>())
            .errors(&[400, 404]),
        get("/api/v0/admin/upstreams", Access::Admin).ok(json::<admin::TopologyView>()),
        get("/api/v0/admin/upstreams/events", Access::Admin).ok(sse::<admin::BackendStatus>()),
        put("/api/v0/admin/backends", Access::Admin)
            .body(json::<admin::BackendSaveBody>())
            .ok(json::<admin::TopologySaved>())
            .errors(&[400, 409]),
        post("/api/v0/admin/backends/test", Access::Admin)
            .body(json::<admin::BackendTestBody>())
            .ok(json::<admin::BackendTest>())
            .errors(&[400, 502, 504]),
        delete("/api/v0/admin/backends/{name}", Access::Admin)
            .no_content()
            .errors(&[400, 404]),
        post("/api/v0/admin/backends/{name}/enabled", Access::Admin)
            .body(json::<aiplane_api::pages::json_workspace::EnabledBody>())
            .ok(json::<admin::BackendEnabled>())
            .errors(&[400, 404]),
        post("/api/v0/admin/backends/{name}/rename", Access::Admin)
            .body(json::<admin::RenameBody>())
            .ok(json::<admin::TopologySaved>())
            .errors(&[400, 404, 409]),
        put("/api/v0/admin/pools", Access::Admin)
            .body(json::<admin::PoolSaveBody>())
            .ok(json::<admin::TopologySaved>())
            .errors(&[400, 409]),
        delete("/api/v0/admin/pools/{name}", Access::Admin)
            .no_content()
            .errors(&[400, 404]),
        post("/api/v0/admin/pools/{name}/rename", Access::Admin)
            .body(json::<admin::RenameBody>())
            .ok(json::<admin::TopologySaved>())
            .errors(&[400, 404, 409]),
        put("/api/v0/admin/upstreams/fallback", Access::Admin)
            .body(json::<admin::FallbackBody>())
            .ok(json::<admin::FallbackSaved>())
            .errors(&[400]),
        post("/api/v0/admin/upstreams/reload", Access::Admin).ok(json::<admin::TopologyApplied>()),
    ]
}
