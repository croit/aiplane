// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The setup wizard as JSON (issue #22 P5/P6) — the SPA at `/setup`
//! drives the first-run flow: enter provider settings → a real OIDC test
//! login → pick the admin claim → finish. Lives in the `gateway` crate (not
//! `gateway-web`) so it survived phase 6's removal of the legacy page stack.
//!
//! Auth model: the wizard is deliberately UNauthenticated on a first run
//! (an empty box has no accounts to authenticate against) and token-gated
//! during a recovery window. The `setup::access` state machine gates every
//! handler here.

use std::sync::Arc;

use rama::http::service::web::extract::State;
use rama::http::{Request, Response, StatusCode, header};

use session_core::chrome::read_cookie;

use gateway_core::rama_server::session::secure_cookies;
use gateway_core::server::auth::oidc::OidcClient;
use gateway_core::server::auth::pending::{self, Purpose};
use gateway_core::server::db;
use gateway_core::server::db::gateway_groups;
use gateway_core::server::oidc_settings;
use gateway_core::server::setup::{self, Draft, Proof, SetupAccess};
use gateway_runtime::rama_server::state::RamaState;
use gateway_runtime::server::state::RuntimeSettings;

/// Where a setup probe comes back to, stored as the pending row's `return_to`
/// and used as the redirect target once the probe's claims are recorded.
///
/// The SPA owns the root, so this is the client route the wizard lives at —
/// not an API path.
pub const SPA_RETURN_TO: &str = "/setup";

/// Carries a verified recovery claim across the wizard's requests, so the
/// one-time token `restore-setup` prints only has to be presented once.
/// Scoped to the wizard's own API prefix.
pub const SETUP_CLAIM_COOKIE: &str = "gw_setup";

fn json(status: StatusCode, body: serde_json::Value) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(body.to_string().into())
        .expect("static JSON response")
}

fn error_json(status: StatusCode, code: &str, message: &str) -> Response {
    json(
        status,
        serde_json::json!({ "error": { "message": message, "type": code, "code": code } }),
    )
}

/// What the gate resolved for one request: how setup may be reached, and the
/// recovery token this request proved (only ever `Some` in a recovery window,
/// and only when it arrived on the query string — see [`gate`]).
struct Gated {
    access: SetupAccess,
    claim: Option<String>,
}

/// The wizard's own access gate.
///
/// * **First run** — open, no token. There is no account to authenticate
///   against yet, and a token nobody can retrieve without shell access would
///   just move the lockout one step earlier.
/// * **Recovery** — the gateway is configured and `restore-setup` reopened
///   setup for 30 minutes; every request must present the one-time token that
///   command printed, either as `?claim=…` or on the [`SETUP_CLAIM_COOKIE`]
///   this handler set from a previous `?claim=…`. Without this a reopened
///   window would let any anonymous caller re-point a live gateway's identity
///   provider at their own.
/// * **Closed** — the wizard does not exist.
async fn gate(state: &RamaState, req: &Request) -> Result<Gated, Response> {
    let access = match setup::access(&state.db).await {
        Ok(a) => a,
        Err(err) => {
            return Err(error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                &format!("reading setup state: {err}"),
            ));
        }
    };
    match access {
        SetupAccess::FirstRun => Ok(Gated {
            access,
            claim: None,
        }),
        SetupAccess::Recovery => {
            let from_query = claim_from_query(req);
            let presented = from_query
                .clone()
                .or_else(|| read_cookie(req.headers(), SETUP_CLAIM_COOKIE));
            match presented {
                Some(token)
                    if setup::recovery_token_matches(&state.db, access, &token)
                        .await
                        .unwrap_or(false) =>
                {
                    Ok(Gated {
                        access,
                        // Only a token that arrived on the query string needs
                        // to be put on a cookie; one that came from the cookie
                        // is already there.
                        claim: from_query,
                    })
                }
                _ => Err(error_json(
                    StatusCode::FORBIDDEN,
                    "forbidden",
                    "This gateway is already configured. Reopening setup needs the one-time \
                     link printed by `restore-setup` on the host.",
                )),
            }
        }
        SetupAccess::Closed => Err(error_json(
            StatusCode::NOT_FOUND,
            "not_found",
            "Setup is closed on this gateway. To reconfigure it, run `restore-setup` on the \
             host — it prints a one-time link.",
        )),
    }
}

/// The `claim` query parameter, percent-decoded.
fn claim_from_query(req: &Request) -> Option<String> {
    let query = req.uri().query()?;
    query.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key == "claim").then(|| percent_decode(value))
    })
}

/// Minimal `application/x-www-form-urlencoded` value decoding — enough for a
/// recovery token, which `restore-setup` prints as hex.
fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => match u8::from_str_radix(&value[i + 1..i + 3], 16) {
                Ok(byte) => {
                    out.push(byte);
                    i += 3;
                }
                Err(_) => {
                    out.push(b'%');
                    i += 1;
                }
            },
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Attach the recovery claim to a response so the rest of the wizard's calls
/// work without repeating the token on every one of them.
///
/// `Secure` matters more here than on the session cookie: this token
/// reconfigures a *live* gateway with real users. It dies with the browser
/// session and is worthless once the window closes, so no `Max-Age`.
fn with_claim(state: &RamaState, mut resp: Response, claim: Option<&str>) -> Response {
    let Some(token) = claim else { return resp };
    let secure = if secure_cookies(&state.public_url()) {
        "; Secure"
    } else {
        ""
    };
    let cookie =
        format!("{SETUP_CLAIM_COOKIE}={token}; Path=/api/v0/setup; HttpOnly; SameSite=Lax{secure}");
    if let Ok(value) = header::HeaderValue::from_str(&cookie) {
        resp.headers_mut().append(header::SET_COOKIE, value);
    }
    resp
}

/// GET /api/v0/setup/state — where the wizard stands: the access mode, the
/// saved draft (if any), and the verified proof (if a test login landed).
/// Secrets are never echoed; the draft carries the client secret only as
/// `client_secret_set`.
pub async fn setup_state(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    let Gated { access, claim } = match gate(&state, &req).await {
        Ok(g) => g,
        Err(resp) => return resp,
    };
    let draft = setup::load_draft(&state.db, &state.crypto)
        .await
        .ok()
        .flatten();
    let proof = setup::load_proof(&state.db, &state.crypto)
        .await
        .ok()
        .flatten();
    let draft_json = draft.as_ref().map(|d| {
        serde_json::json!({
            "public_url": d.public_url,
            "issuer": d.params.issuer,
            "client_id": d.params.client_id,
            "client_secret_set": !d.params.client_secret.is_empty(),
            "scopes": d.params.scopes,
            "roles_claim": d.params.roles_claim,
        })
    });
    let proof_json = proof.as_ref().map(|p| {
        serde_json::json!({
            "subject": p.subject,
            "email": p.email,
            "name": p.name,
            "claims": p.claims,
        })
    });
    let resp = json(
        StatusCode::OK,
        serde_json::json!({
            "access": match access {
                SetupAccess::FirstRun => "first_run",
                SetupAccess::Recovery => "recovery",
                SetupAccess::Closed => "closed",
            },
            "draft": draft_json,
            "proof": proof_json,
            // The wizard form's public-url prefill: the URL the operator's
            // browser is actually using.
            "suggested_public_url": public_url_from_request(&req, &state),
        }),
    );
    with_claim(&state, resp, claim.as_deref())
}

fn public_url_from_request(req: &Request, state: &RamaState) -> String {
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost:8080");
    let forwarded_proto = req
        .headers()
        .get(header::HeaderName::from_static("x-forwarded-proto"))
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(str::trim);
    let scheme = match forwarded_proto {
        Some("https") => "https",
        Some("http") => "http",
        _ if state.public_url().starts_with("https://") => "https",
        _ => "http",
    };
    format!("{scheme}://{host}")
}

#[derive(serde::Deserialize)]
pub struct SetupTestBody {
    pub public_url: String,
    pub issuer: String,
    pub client_id: String,
    #[serde(default)]
    pub client_secret: String,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub roles_claim: String,
}

/// POST /api/v0/setup/test — validate the provider settings, save the
/// draft, and start a real OIDC round trip. Returns the authorize URL for
/// the SPA to navigate to, plus the browser-binding cookie the callback
/// will check.
pub async fn setup_test(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    if let Err(resp) = gate(&state, &req).await {
        return resp;
    }
    let (_, body) = req.into_parts();
    let bytes = match session_core::chrome::read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return error_json(StatusCode::BAD_REQUEST, "invalid_request", &msg),
    };
    let parsed: SetupTestBody = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => {
            return error_json(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                &format!("parsing the settings body: {err}"),
            );
        }
    };
    let public_url = parsed.public_url.trim().trim_end_matches('/').to_string();
    let issuer = parsed.issuer.trim().to_string();
    let client_id = parsed.client_id.trim().to_string();
    if public_url.is_empty() || issuer.is_empty() || client_id.is_empty() {
        return error_json(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Public URL, issuer and client id are all required.",
        );
    }
    // A blank secret on an existing draft keeps the stored one (the SPA never
    // echoes secrets back); on a fresh draft it is required.
    let secret = if parsed.client_secret.is_empty() {
        match setup::load_draft(&state.db, &state.crypto).await {
            Ok(Some(existing)) if existing.params.issuer == issuer => existing.params.client_secret,
            _ => {
                return error_json(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "The client secret is required.",
                );
            }
        }
    } else {
        parsed.client_secret
    };
    // Any proof on file belongs to the previous draft — the proof and the
    // draft it proves must move together.
    if let Err(err) = setup::clear_proof(&state.db).await {
        return error_json(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            &format!("could not clear the previous attempt: {err}"),
        );
    }
    let mut scopes = parsed.scopes;
    if scopes.is_empty() {
        scopes = oidc_settings::parse_scopes_or_default("");
    }
    let draft = Draft {
        public_url: public_url.clone(),
        params: gateway_core::server::auth::oidc::OidcParams {
            issuer,
            client_id,
            client_secret: secret,
            scopes,
            roles_claim: (!parsed.roles_claim.trim().is_empty())
                .then(|| parsed.roles_claim.trim().to_string()),
        },
    };
    let client = match OidcClient::build(&draft.params, &draft.public_url).await {
        Ok(c) => c,
        Err(err) => {
            // Keep the draft so the operator's typing survives.
            let _ = setup::save_draft(&state.db, &state.crypto, &draft).await;
            return error_json(
                StatusCode::BAD_GATEWAY,
                "bad_gateway",
                &format!("Could not reach that provider: {err}"),
            );
        }
    };
    if let Err(err) = setup::save_draft(&state.db, &state.crypto, &draft).await {
        return error_json(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            &format!("could not save the entered settings: {err}"),
        );
    }
    let start = client.begin();
    if let Err(err) = pending::insert(&state.db, &start, Some(SPA_RETURN_TO), Purpose::Setup).await
    {
        return error_json(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            &format!("could not start the test login: {err}"),
        );
    }
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::SET_COOKIE, pending::binding_cookie(&start.csrf))
        .body(
            serde_json::json!({ "authorize_url": start.url })
                .to_string()
                .into(),
        )
        .expect("static JSON response")
}

/// POST /api/v0/setup/restart — throw the proof away, back to screen 1.
pub async fn setup_restart(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    if let Err(resp) = gate(&state, &req).await {
        return resp;
    }
    if let Err(err) = setup::clear_proof(&state.db).await {
        return error_json(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            &format!("clearing the proof: {err}"),
        );
    }
    json(StatusCode::OK, serde_json::json!({ "ok": true }))
}

#[derive(serde::Deserialize)]
pub struct SetupFinishBody {
    /// The claim/value pair that grants admin: either a picked pair
    /// `{"claim":…, "value":…}` or a manual `{"manual_claim":…, "manual_value":…}`.
    #[serde(default)]
    pub claim: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub manual_claim: String,
    #[serde(default)]
    pub manual_value: String,
}

/// POST /api/v0/setup/finish — promote the draft to live settings, create
/// the admin + default groups, mark setup complete, hot-swap the runtime.
pub async fn setup_finish(State(state): State<Arc<RamaState>>, req: Request) -> Response {
    if let Err(resp) = gate(&state, &req).await {
        return resp;
    }
    let (_, body) = req.into_parts();
    let bytes = match session_core::chrome::read_body_to_bytes(body).await {
        Ok(b) => b,
        Err(msg) => return error_json(StatusCode::BAD_REQUEST, "invalid_request", &msg),
    };
    let parsed: SetupFinishBody = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => {
            return error_json(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                &format!("parsing the finish body: {err}"),
            );
        }
    };
    let (Ok(Some(draft)), Ok(Some(_proof))) = (
        setup::load_draft(&state.db, &state.crypto).await,
        setup::load_proof(&state.db, &state.crypto).await,
    ) else {
        return error_json(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "This setup run has expired. Start again.",
        );
    };
    // A manual pair wins over the picked one (the operator went out of their
    // way to type it).
    let admin_claim;
    let admin_value;
    if !parsed.manual_claim.trim().is_empty() && !parsed.manual_value.trim().is_empty() {
        admin_claim = parsed.manual_claim.trim().to_owned();
        admin_value = parsed.manual_value.trim().to_owned();
    } else if let (Some(c), Some(v)) = (parsed.claim.as_deref(), parsed.value.as_deref()) {
        admin_claim = c.to_owned();
        admin_value = v.to_owned();
    } else {
        return error_json(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Pick which claim value should grant administrator access, or type one in.",
        );
    }
    // The chosen claim IS the roles claim.
    let params = gateway_core::server::auth::oidc::OidcParams {
        roles_claim: Some(admin_claim.clone()),
        ..draft.params.clone()
    };
    let client = match OidcClient::build(&params, &draft.public_url).await {
        Ok(c) => c,
        Err(err) => {
            return error_json(
                StatusCode::BAD_GATEWAY,
                "bad_gateway",
                &format!("Could not reach that provider: {err}"),
            );
        }
    };
    if let Err(err) = persist(&state, &draft, &params, &admin_value).await {
        return error_json(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            &format!("could not save the configuration: {err}"),
        );
    }
    state.set_runtime(RuntimeSettings {
        public_url: draft.public_url.clone(),
        oidc: Some(client),
        setup_completed: true,
    });
    state.reload_rbac().await;
    tracing::info!(
        public_url = %draft.public_url,
        issuer = %draft.params.issuer,
        admin_claim = %admin_claim,
        "setup completed; OIDC is live"
    );
    json(
        StatusCode::OK,
        serde_json::json!({ "ok": true, "landing": "/admin/settings" }),
    )
}

/// The gateway group the wizard creates for administrators. A name, not a
/// magic value — the admin can rename or replace it at `/admin/groups`
/// afterwards; what makes it privileged is its `is_admin` flag.
const ADMIN_GROUP: &str = "admins";
/// The group every other signed-in user falls into. Created with no grants on
/// purpose: who may use which tools, pools and skills is a decision for the
/// operator, not something a wizard should presume.
const DEFAULT_GROUP: &str = "users";

/// Write everything the wizard decided, ordered so a failure part-way
/// leaves the gateway unconfigured.
async fn persist(
    state: &RamaState,
    draft: &Draft,
    params: &gateway_core::server::auth::oidc::OidcParams,
    admin_value: &str,
) -> Result<(), db::DbError> {
    setup::set_public_url(&state.db, &draft.public_url).await?;
    oidc_settings::set_params(&state.db, &state.crypto, params).await?;
    gateway_groups::upsert_group(
        &state.db,
        ADMIN_GROUP,
        "Full administrative access. Created by the setup wizard.",
        true,
        false,
    )
    .await?;
    gateway_groups::set_mappings_for_group(&state.db, ADMIN_GROUP, &[admin_value.to_owned()])
        .await?;
    gateway_groups::set_tools_for_group(&state.db, ADMIN_GROUP, &["*".to_owned()]).await?;
    gateway_groups::upsert_group(
        &state.db,
        DEFAULT_GROUP,
        "Everyone who signs in. Grant it tools and pools at /admin/groups.",
        false,
        true,
    )
    .await?;
    setup::mark_completed(&state.db).await
}

/// Called by `/auth/callback` when the in-flight row was a setup probe:
/// complete the exchange against the DRAFT provider and record the claims
/// for screen 2. No user row, no session — proving a provider authorises
/// nobody.
pub async fn setup_probe_callback(
    state: &RamaState,
    _headers: &header::HeaderMap,
    code: &str,
    verifier: &str,
    nonce: &str,
) -> Response {
    if setup::access(&state.db)
        .await
        .unwrap_or(SetupAccess::Closed)
        == SetupAccess::Closed
    {
        return error_json(
            StatusCode::NOT_FOUND,
            "not_found",
            "Setup is no longer open on this gateway, so this test sign-in was discarded.",
        );
    }
    let Ok(Some(draft)) = setup::load_draft(&state.db, &state.crypto).await else {
        return error_json(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "The setup attempt this login belongs to is gone. Start again.",
        );
    };
    let client = match OidcClient::build(&draft.params, &draft.public_url).await {
        Ok(c) => c,
        Err(err) => {
            return error_json(
                StatusCode::BAD_GATEWAY,
                "bad_gateway",
                &format!("Could not reach that provider: {err}"),
            );
        }
    };
    let claims = match client.complete(code, verifier, nonce).await {
        Ok(c) => c,
        Err(err) => {
            return error_json(
                StatusCode::BAD_GATEWAY,
                "bad_gateway",
                &format!(
                    "The provider accepted the sign-in but the gateway could not complete it: \
                     {err}. Check the client secret, and that the redirect URI is whitelisted."
                ),
            );
        }
    };
    let proof = Proof {
        subject: claims.subject,
        email: claims.email,
        name: claims.name,
        claims: claims.raw,
    };
    if let Err(err) = setup::save_proof(&state.db, &state.crypto, &proof).await {
        return error_json(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            &format!("could not record the test login: {err}"),
        );
    }
    // Back to the SPA wizard.
    Response::builder()
        .status(StatusCode::SEE_OTHER)
        .header(header::LOCATION, SPA_RETURN_TO)
        .body(rama::http::Body::empty())
        .expect("static redirect")
}
