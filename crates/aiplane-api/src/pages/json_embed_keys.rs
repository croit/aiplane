// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! `/api/v0/agents/{id}/embed-keys` — the keys a website's widget starts
//! visitor conversations with. They follow the agent's share rules
//! (`pages::json_agents`): `read` lists them, `write` creates and revokes
//! them. The public side is `pages::embed`.

use std::sync::Arc;

use rama::http::service::web::extract::State;
use rama::http::{Request, Response, StatusCode};
use serde::Deserialize;

use super::json_agents::agent_at;
use super::json_principals::require_agent_manager;
use super::{bad_request, internal, json_ok, no_content, not_found, raw_path_segment};
use aiplane_agents::db::agents::Access;
use aiplane_agents::db::embed_keys;
use aiplane_core::server::auth::token;
use aiplane_runtime::agents::spec;
use aiplane_runtime::rama_server::state::RamaState;

/// An embed key, never the key itself.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct EmbedKeyView {
    pub id: String,
    pub name: String,
    /// The `Origin`s a website may use the key from.
    pub origins: Vec<String>,
    pub created_by: String,
    #[schemars(with = "shared::api::TimestampSchema")]
    pub created_at: jiff::Timestamp,
    #[schemars(with = "Option<shared::api::TimestampSchema>")]
    pub revoked_at: Option<jiff::Timestamp>,
}

impl EmbedKeyView {
    fn of(k: &embed_keys::EmbedKey) -> Self {
        Self {
            id: k.id.clone(),
            name: k.name.clone(),
            origins: k.origins.clone(),
            created_by: k.created_by.clone(),
            created_at: k.created_at,
            revoked_at: k.revoked_at,
        }
    }
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct EmbedKeyList {
    /// Revoked ones included.
    pub embed_keys: Vec<EmbedKeyView>,
}

/// A new embed key. `key` (`gwe_…`) is shown here and nowhere else.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct CreatedEmbedKey {
    pub embed_key: EmbedKeyView,
    pub key: String,
}

/// GET /api/v0/agents/{id}/embed-keys — every key, revoked ones included,
/// never the key itself.
pub async fn list(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let user = or_return!(require_agent_manager(&state, &req).await);
    let (agent, _) = or_return!(agent_at(&state, &req, &user, 1, Access::Read).await);
    match embed_keys::list(&state.db, &agent.principal.id).await {
        Ok(keys) => json_ok(
            StatusCode::OK,
            EmbedKeyList {
                embed_keys: keys.iter().map(EmbedKeyView::of).collect(),
            },
        ),
        Err(err) => internal(err),
    }
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EmbedKeyBody {
    pub name: String,
    /// Each exactly `scheme://host[:port]`, at least one.
    pub origins: Vec<String>,
}

/// The origins as stored: trimmed, de-duplicated, each exactly
/// `scheme://host[:port]`, at least one.
fn parse_origins(origins: &[String]) -> Result<Vec<String>, Response> {
    let mut out: Vec<String> = Vec::new();
    for origin in origins.iter().map(|o| o.trim()) {
        if !spec::is_origin(origin) {
            return Err(bad_request(format!(
                "`{origin}` is not an origin — write exactly what the browser sends in `Origin`: \
                 `https://host` or `https://host:port`, without a path or trailing slash"
            )));
        }
        if !out.iter().any(|o| o == origin) {
            out.push(origin.to_string());
        }
    }
    if out.is_empty() {
        return Err(bad_request(
            "an embed key needs at least one origin, e.g. `https://www.example.com` — no website \
             could use it otherwise",
        ));
    }
    Ok(out)
}

/// POST /api/v0/agents/{id}/embed-keys — a new `gwe_` key for the listed
/// origins. The key is in this response only.
pub async fn create(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let user = or_return!(require_agent_manager(&state, &req).await);
    let (agent, _) = or_return!(agent_at(&state, &req, &user, 1, Access::Write).await);
    let body: EmbedKeyBody =
        or_return!(super::read_json(req.into_body(), "the embed key body").await);
    let name = body.name.trim();
    if name.is_empty() {
        return bad_request("an embed key needs a `name`, e.g. the website it is for");
    }
    let origins = or_return!(parse_origins(&body.origins));
    let (key, key_hash) = token::mint_embed_key();
    match embed_keys::create(
        &state.db,
        &embed_keys::NewEmbedKey {
            principal_id: &agent.principal.id,
            name,
            origins: &origins,
            key_hash: &key_hash,
        },
        &user.id,
    )
    .await
    {
        Ok(k) => json_ok(
            StatusCode::CREATED,
            CreatedEmbedKey {
                embed_key: EmbedKeyView::of(&k),
                key,
            },
        ),
        Err(err) => internal(err),
    }
}

/// POST /api/v0/agents/{id}/embed-keys/{key_id}/revoke — websites using the
/// key stop working at once, open visitor conversations included.
pub async fn revoke(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let user = or_return!(require_agent_manager(&state, &req).await);
    let (agent, _) = or_return!(agent_at(&state, &req, &user, 3, Access::Write).await);
    let Some(key_id) = raw_path_segment(&req, 1) else {
        return bad_request("the URL is missing the embed key id");
    };
    match embed_keys::revoke(&state.db, &agent.principal.id, &key_id, &user.id).await {
        Ok(true) => no_content(),
        Ok(false) => not_found(format!(
            "`{}` has no live embed key `{key_id}` — list them with GET /api/v0/agents/{}/embed-keys",
            agent.principal.name, agent.principal.id
        )),
        Err(err) => internal(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_are_exact_deduplicated_and_never_empty() {
        let o = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            parse_origins(&o(&[
                " https://a.example ",
                "https://a.example",
                "http://localhost:5173"
            ]))
            .unwrap(),
            o(&["https://a.example", "http://localhost:5173"])
        );
        for bad in ["https://a.example/", "a.example", "*", "ftp://a.example"] {
            assert!(parse_origins(&o(&[bad])).is_err(), "{bad} accepted");
        }
        assert!(parse_origins(&[]).is_err());
    }
}
