// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! `/api/v0/admin/classification`: the one editor of the data classification
//! scheme. The policy fields live in the settings rows, written in the same
//! transaction as the levels they refer to.

mod templates;
pub mod types;
mod validate;

use std::collections::{HashMap, HashSet};
use std::convert::Infallible;
use std::sync::Arc;

use aiplane_core::server::classification::admin;
use aiplane_core::server::classification::{self, DestinationKind, Over, Scheme, marking_rules};
use aiplane_core::server::db;
use aiplane_core::server::upstreams::PoolKind;
use aiplane_features::server::classification::{
    TermError, TermMatcher, TermUpload, parse_term_upload, read_markings,
};
use aiplane_runtime::classification_upload::dry_run;
use aiplane_runtime::rama_server::state::RamaState;
use rama::http::service::web::extract::{Path, State};
use rama::http::{Request, Response, StatusCode, header};
use session_core::chrome::{CappedBodyError, read_body_capped};

use super::{
    bad_request, internal, json_error, json_error_with, json_ok, no_content, not_found,
    parse_limit, query_map, read_json,
};
use types::*;
use validate::{Context, Invalid, validate};

/// A word list is one term per line; eight MiB is far beyond any real one.
const TERMS_UPLOAD_MAX_BYTES: usize = 8 * 1024 * 1024;
/// The first terms of an upload shown back to the operator.
const TERMS_SAMPLE: usize = 10;
/// The largest sample file `inspect` and `test` read, as large as an upload
/// inspection reads.
const SAMPLE_UPLOAD_MAX_BYTES: usize = 64 * 1024 * 1024;
const EVENTS_DEFAULT: usize = 100;
const EVENTS_MAX: usize = 500;

fn refuse(invalid: Invalid) -> Response {
    let mut extra = serde_json::Map::new();
    extra.insert("field".into(), invalid.field.into());
    json_error_with(
        StatusCode::BAD_REQUEST,
        "invalid_classification",
        &invalid.message,
        extra,
    )
}

struct Destinations {
    views: Vec<DestinationView>,
}

impl Destinations {
    fn known(&self) -> HashSet<(String, String)> {
        self.views
            .iter()
            .map(|d| (d.kind.to_owned(), d.id.clone()))
            .collect()
    }
}

fn rated(
    scheme: &Scheme,
    kind: DestinationKind,
    id: &str,
    name: &str,
    description: &str,
    egress: bool,
) -> DestinationView {
    let assignment = scheme.assignment(kind, id);
    DestinationView {
        kind: kind.as_str(),
        id: id.to_owned(),
        name: name.to_owned(),
        description: description.to_owned(),
        level_id: assignment.map(|a| a.level_id.clone()),
        over: assignment.map_or(Over::Hide, |a| a.over).as_str(),
        egress,
    }
}

async fn destinations(state: &RamaState, scheme: &Scheme) -> Result<Destinations, Response> {
    let mut views = Vec::new();
    let mut pools = state.upstreams.pools();
    pools.sort_by(|a, b| a.name.cmp(&b.name));
    for pool in pools {
        views.push(rated(
            scheme,
            DestinationKind::Pool,
            &pool.name,
            &pool.name,
            pool.kind.as_str(),
            false,
        ));
    }
    let tools = state.tools();
    let mut ids: Vec<&str> = tools.ids().collect();
    ids.sort_unstable();
    for id in ids {
        let Some(tool) = tools.get(id) else { continue };
        let def = tool.schema();
        views.push(rated(
            scheme,
            DestinationKind::Tool,
            id,
            &def.function.name,
            &def.function.description,
            tool.egress(),
        ));
    }
    for connector in db::mcp_catalog::list_all(&state.db)
        .await
        .map_err(internal)?
    {
        views.push(rated(
            scheme,
            DestinationKind::Connector,
            &connector.key,
            &connector.name,
            connector.description.as_deref().unwrap_or(""),
            true,
        ));
    }
    for collection in db::rag::list_collections(&state.db)
        .await
        .map_err(internal)?
    {
        views.push(rated(
            scheme,
            DestinationKind::Collection,
            &collection.id.to_string(),
            &collection.name,
            collection.description.as_deref().unwrap_or(""),
            false,
        ));
    }
    Ok(Destinations { views })
}

/// For each System One model, the pools serving it.
fn system_one_pools(state: &RamaState) -> HashMap<String, Vec<String>> {
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    for pool in state.upstreams.pools() {
        if pool.kind != PoolKind::SystemOne {
            continue;
        }
        for backend in &pool.backends {
            for model in backend.listed_models() {
                let pools = out.entry(model).or_default();
                if !pools.contains(&pool.name) {
                    pools.push(pool.name.clone());
                }
            }
        }
    }
    out
}

fn system_one_models(state: &RamaState, scheme: &Scheme) -> Vec<SystemOneModelView> {
    let highest = scheme.highest().rank;
    let mut out: Vec<SystemOneModelView> = system_one_pools(state)
        .into_iter()
        .map(|(id, pools)| {
            let assignments: Vec<_> = pools
                .iter()
                .map(|pool| scheme.assignment(DestinationKind::Pool, pool))
                .collect();
            let rank = assignments
                .iter()
                .map(|a| a.map_or(0, |a| scheme.rank_of(&a.level_id).unwrap_or(0)))
                .min()
                .unwrap_or(0);
            let level_id = if assignments.iter().any(Option::is_none) {
                None
            } else {
                scheme
                    .levels()
                    .iter()
                    .find(|l| l.rank == rank)
                    .map(|l| l.id.clone())
            };
            SystemOneModelView {
                id,
                level_id,
                cleared_for_highest: rank >= highest,
            }
        })
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

fn policy_view(state: &RamaState, scheme: &Scheme) -> PolicyBody {
    let config = state.config();
    let policy = &config.classification;
    PolicyBody {
        default_level: scheme.resolve(&policy.default_level).id.clone(),
        lowering: policy.lowering.as_str().to_owned(),
        gate_on_match: policy.gate_on_match.as_str().to_owned(),
        link_share_max_level: scheme.resolve(&policy.link_share_max_level).id.clone(),
        unknown_label_level: scheme.resolve(&policy.unknown_label_level).id.clone(),
        unreadable_level: scheme.resolve(&policy.unreadable_level).id.clone(),
        system_one_model: policy.system_one_model.clone(),
        mode: policy.mode.as_str().to_owned(),
        mip_tenant_id: policy.mip_tenant_id.clone(),
        output: OutputBody {
            visible: policy.output.visible,
            metadata: policy.output.metadata,
            filename: policy.output.filename,
        },
    }
}

async fn current_state(state: &RamaState) -> Result<ClassificationState, Response> {
    let scheme = state.scheme();
    let classes = admin::list_classes(&state.db).await.map_err(internal)?;
    let markings = marking_rules(&state.db)
        .await
        .map_err(internal)?
        .into_iter()
        .map(|rule| MarkingBody {
            kind: rule.kind.as_str().to_owned(),
            value: rule.value,
            level_id: rule.level_id,
        })
        .collect();
    Ok(ClassificationState {
        active: scheme.is_active(),
        levels: scheme.levels().iter().map(LevelBody::from).collect(),
        classes: classes.iter().map(ClassView::from).collect(),
        markings,
        destinations: destinations(state, &scheme).await?.views,
        policy: policy_view(state, &scheme),
        templates: templates::all(),
        system_one_models: system_one_models(state, &scheme),
    })
}

/// GET /api/v0/admin/classification — the scheme, every destination that can
/// be rated, the templates and the System One models.
pub async fn get(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, _admin) = require_admin_json!(state, req);
    match current_state(&state).await {
        Ok(view) => json_ok(StatusCode::OK, view),
        Err(response) => response,
    }
}

/// PUT /api/v0/admin/classification — replace the whole scheme in one
/// transaction. A conversation at a removed level moves up and the move is
/// audited.
pub async fn save(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, admin_user) = require_admin_json!(state, req);
    let (_, body) = req.into_parts();
    let parsed: SaveBody = or_return!(read_json(body, "the classification body").await);
    let scheme = state.scheme();
    let known = or_return!(destinations(&state, &scheme).await).known();
    let pools = system_one_pools(&state);
    let replacement = match validate(
        &parsed,
        &Context {
            destinations: Some(&known),
            system_one_pools: &pools,
        },
    ) {
        Ok(replacement) => replacement,
        Err(invalid) => return refuse(invalid),
    };
    let moved = match admin::replace(&state.db, &replacement, &admin_user.id).await {
        Ok(moved) => moved,
        Err(err) => return internal(err),
    };
    match classification::pool_ranks(&state.db).await {
        Ok(ranks) => state.upstreams.apply_pool_levels(&ranks),
        Err(err) => return internal(err),
    }
    state.reload_settings().await;
    tracing::info!(
        admin = %admin_user.id,
        levels = replacement.levels.len(),
        classes = replacement.classes.len(),
        conversations_moved = moved.len(),
        "classification scheme replaced"
    );
    match current_state(&state).await {
        Ok(view) => json_ok(StatusCode::OK, view),
        Err(response) => response,
    }
}

struct Part {
    name: String,
    filename: Option<String>,
    mime: String,
    bytes: Vec<u8>,
}

async fn multipart(req: Request, max: usize) -> Result<Vec<Part>, Response> {
    let content_type = req
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    let boundary = match multer::parse_boundary(&content_type) {
        Ok(boundary) if !boundary.is_empty() => boundary,
        _ => {
            return Err(bad_request(
                "this endpoint takes a multipart/form-data upload; send the file as a form part named `file`",
            ));
        }
    };
    let (_, body) = req.into_parts();
    let bytes = match read_body_capped(body, max).await {
        Ok(bytes) => bytes,
        Err(CappedBodyError::TooLarge { max }) => {
            return Err(json_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "payload_too_large",
                &format!(
                    "the upload is larger than {} MiB; split the file or remove what is not needed",
                    max / (1024 * 1024)
                ),
            ));
        }
        Err(CappedBodyError::Read(message)) => return Err(bad_request(message)),
    };
    let stream = rama::futures::stream::once(async move { Ok::<_, Infallible>(bytes) });
    let mut multipart = multer::Multipart::new(stream, boundary);
    let mut out = Vec::new();
    loop {
        match multipart.next_field().await {
            Ok(Some(field)) => {
                let name = field.name().map(str::to_owned).unwrap_or_default();
                let filename = field.file_name().map(str::to_owned);
                let mime = field
                    .content_type()
                    .map(ToString::to_string)
                    .unwrap_or_default();
                match field.bytes().await {
                    Ok(data) => out.push(Part {
                        name,
                        filename,
                        mime,
                        bytes: data.to_vec(),
                    }),
                    Err(err) => {
                        return Err(bad_request(format!(
                            "reading the upload failed: {err}; send it again"
                        )));
                    }
                }
            }
            Ok(None) => break,
            Err(err) => {
                return Err(bad_request(format!(
                    "the upload is not valid multipart data: {err}"
                )));
            }
        }
    }
    Ok(out)
}

fn file_part(parts: &[Part]) -> Option<&Part> {
    parts.iter().find(|p| p.name == "file")
}

fn term_failure(error: TermError) -> Response {
    bad_request(error.to_string())
}

fn preview_of(upload: &TermUpload) -> TermsPreview {
    TermsPreview {
        count: upload.terms.len(),
        duplicates: upload.duplicates.len(),
        too_short: upload.too_short.clone(),
        sample: upload.terms.iter().take(TERMS_SAMPLE).cloned().collect(),
    }
}

/// POST /api/v0/admin/classification/classes/{id}/terms — read an uploaded
/// word list. With `?preview=1` nothing is saved; otherwise it replaces the
/// class's list. `column` names the CSV column; without it the file is one
/// term per line.
pub async fn terms_upload(
    Path(class_id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, admin_user) = require_admin_json!(state, req);
    let query = query_map(&req);
    let preview = query
        .get("preview")
        .is_some_and(|v| v == "1" || v == "true");
    let column = query
        .get("column")
        .map(|c| c.trim())
        .filter(|c| !c.is_empty());
    let classes = match admin::list_classes(&state.db).await {
        Ok(classes) => classes,
        Err(err) => return internal(err),
    };
    if !classes.iter().any(|c| c.class.id == class_id) {
        return not_found(format!(
            "there is no data class `{class_id}`; save the class before uploading its word list"
        ));
    }
    let parts = or_return!(multipart(req, TERMS_UPLOAD_MAX_BYTES).await);
    let Some(file) = file_part(&parts) else {
        return bad_request(
            "the upload has no `file` part; send the word list as a form part named `file`",
        );
    };
    let upload = match parse_term_upload(&file.bytes, column) {
        Ok(upload) => upload,
        Err(error) => return term_failure(error),
    };
    if let Err(error) = TermMatcher::new(&upload.terms) {
        return term_failure(error);
    }
    let preview_view = preview_of(&upload);
    if preview {
        return json_ok(StatusCode::OK, preview_view);
    }
    match admin::replace_terms(&state.db, &class_id, &upload.terms, &admin_user.id).await {
        Ok(true) => {}
        Ok(false) => return not_found(format!("there is no data class `{class_id}`")),
        Err(err) => return internal(err),
    }
    state.reload_scheme().await;
    let uploaded_at = admin::list_classes(&state.db)
        .await
        .ok()
        .and_then(|all| all.into_iter().find(|c| c.class.id == class_id))
        .and_then(|c| c.terms.uploaded_at)
        .unwrap_or_default();
    json_ok(
        StatusCode::OK,
        TermsUploaded {
            preview: preview_view,
            uploaded_at,
        },
    )
}

/// DELETE /api/v0/admin/classification/classes/{id}/terms — empty the list.
pub async fn terms_clear(
    Path(class_id): Path<String>,
    State(state): State<Arc<RamaState>>,
    req: Request,
) -> Response {
    let (_session, _admin) = require_admin_json!(state, req);
    match admin::clear_terms(&state.db, &class_id).await {
        Ok(true) => {
            state.reload_scheme().await;
            no_content()
        }
        Ok(false) => not_found(format!("there is no data class `{class_id}`")),
        Err(err) => internal(err),
    }
}

/// POST /api/v0/admin/classification/inspect — the markings a sample file
/// carries, to map them to levels.
pub async fn inspect(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, _admin) = require_admin_json!(state, req);
    let parts = or_return!(multipart(req, SAMPLE_UPLOAD_MAX_BYTES).await);
    let Some(file) = file_part(&parts) else {
        return bad_request(
            "the upload has no `file` part; send the sample as a form part named `file`",
        );
    };
    let (name, mime, bytes) = (
        file.filename.clone().unwrap_or_default(),
        file.mime.clone(),
        file.bytes.clone(),
    );
    let read = match tokio::task::spawn_blocking(move || read_markings(&name, &mime, &bytes)).await
    {
        Ok(read) => read,
        Err(err) => return internal(err),
    };
    json_ok(
        StatusCode::OK,
        InspectResult {
            markings: read
                .found
                .into_iter()
                .map(|m| InspectedMarking {
                    kind: m.kind.as_str(),
                    value: m.value,
                    name: m.name,
                })
                .collect(),
            unreadable: read.unreadable,
        },
    )
}

/// POST /api/v0/admin/classification/test — what the saved scheme makes of a
/// file and/or a text. Nothing is stored.
pub async fn test(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, _admin) = require_admin_json!(state, req);
    let parts = or_return!(multipart(req, SAMPLE_UPLOAD_MAX_BYTES).await);
    let file = file_part(&parts).filter(|p| !p.bytes.is_empty());
    let text = parts
        .iter()
        .find(|p| p.name == "text")
        .map(|p| String::from_utf8_lossy(&p.bytes).into_owned());
    if file.is_none() && text.as_deref().is_none_or(|t| t.trim().is_empty()) {
        return bad_request("send a `file` part, a `text` part, or both; there is nothing to test");
    }
    let name = file.and_then(|f| f.filename.clone()).unwrap_or_default();
    let outcome = dry_run(
        &state,
        file.map(|f| (name.as_str(), f.mime.as_str(), f.bytes.as_slice())),
        text.as_deref(),
    )
    .await;
    match outcome {
        Ok(run) => json_ok(
            StatusCode::OK,
            TestResult {
                level_id: run.level_id,
                causes: run.causes,
                classes: run.classes,
            },
        ),
        Err(message) => json_error(
            StatusCode::BAD_GATEWAY,
            "system_one_unavailable",
            &format!(
                "{message}; check the System One model under the policy, or test without text the model would read"
            ),
        ),
    }
}

/// GET /api/v0/admin/classification/events?session=&limit= — the level audit,
/// newest first.
pub async fn events(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let (_session, _admin) = require_admin_json!(state, req);
    let query = query_map(&req);
    let limit = or_return!(parse_limit(&query, EVENTS_DEFAULT, EVENTS_MAX));
    let session = query
        .get("session")
        .map(String::as_str)
        .filter(|s| !s.is_empty());
    match admin::events(&state.db, session, limit as u32).await {
        Ok(rows) => json_ok(
            StatusCode::OK,
            EventsView {
                events: rows
                    .into_iter()
                    .map(|e| EventView {
                        session_id: e.session_id,
                        at: e.at,
                        from_level: e.from_level,
                        to_level: e.to_level,
                        cause: e.cause,
                        actor: e.actor,
                        reason: e.reason,
                    })
                    .collect(),
            },
        ),
        Err(err) => internal(err),
    }
}
