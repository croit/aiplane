// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use super::{DbError, Pool, WriteTx, parse_ts};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use sqlx::{Row, sqlite::SqliteRow};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "id",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ConnectionOwner {
    User(String),
    Principal(String),
}
impl ConnectionOwner {
    fn columns(&self) -> (Option<&str>, Option<&str>) {
        match self {
            Self::User(id) => (Some(id), None),
            Self::Principal(id) => (None, Some(id)),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectionIdentity {
    pub owner: ConnectionOwner,
    pub connector_key: String,
    pub connection_id: String,
    pub generation: String,
}

/// `status` of a usable connection.
pub const STATUS_CONNECTED: &str = "connected";
/// `status` after a failure we might recover from on our own (server down,
/// TLS/transport error, a token the server refused for an unclear reason).
pub const STATUS_ERROR: &str = "error";
/// `status` after the authorization server declared the stored credential dead
/// (`invalid_grant` / `invalid_client` / `unauthorized_client`). Nothing but a
/// fresh user authorization brings it back, so the store says so plainly
/// instead of pointing at the URL or the token.
pub const STATUS_REAUTH: &str = "reauth";

/// A user's connection to one connector.
#[derive(Debug, Clone)]
pub struct Connection {
    pub catalog_generation: Option<String>,
    pub id: String,
    pub generation: String,
    pub authorization_generation: Option<String>,
    pub authorization_expires_at: Option<Timestamp>,
    pub owner: ConnectionOwner,
    pub connector_key: String,
    pub status: String,
    pub access_token_ct: Option<Vec<u8>>,
    pub access_token_nonce: Option<Vec<u8>>,
    pub refresh_token_ct: Option<Vec<u8>>,
    pub refresh_token_nonce: Option<Vec<u8>>,
    pub token_expires_at: Option<Timestamp>,
    pub scopes: Vec<String>,
    pub dcr_client_id: Option<String>,
    pub dcr_client_secret_ct: Option<Vec<u8>>,
    pub dcr_client_secret_nonce: Option<Vec<u8>>,
    /// Resolved OAuth token endpoint, persisted so refresh skips re-discovery.
    pub token_url: Option<String>,
    pub last_error: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl Connection {
    pub fn identity(&self) -> ConnectionIdentity {
        ConnectionIdentity {
            owner: self.owner.clone(),
            connector_key: self.connector_key.clone(),
            connection_id: self.id.clone(),
            generation: self.generation.clone(),
        }
    }
    /// Anything but `connected` — the store shows "Needs reconnect".
    pub fn is_errored(&self) -> bool {
        self.status != STATUS_CONNECTED
    }

    /// The credential is dead; only re-authorizing helps. See [`STATUS_REAUTH`].
    pub fn needs_reauth(&self) -> bool {
        self.status == STATUS_REAUTH
    }
}

/// Everything needed to persist a freshly-completed OAuth connection. Tokens
/// arrive pre-sealed.
pub struct NewConnection {
    pub catalog_generation: String,
    pub owner: ConnectionOwner,
    pub connector_key: String,
    pub access_token_ct: Vec<u8>,
    pub access_token_nonce: Vec<u8>,
    pub refresh_token_ct: Option<Vec<u8>>,
    pub refresh_token_nonce: Option<Vec<u8>>,
    pub token_expires_at: Option<Timestamp>,
    pub scopes: Vec<String>,
    pub dcr_client_id: Option<String>,
    pub dcr_client_secret_ct: Option<Vec<u8>>,
    pub dcr_client_secret_nonce: Option<Vec<u8>>,
    pub token_url: Option<String>,
}

pub const STATUS_PENDING: &str = "pending";
fn map_conn(row: &SqliteRow) -> Result<Connection, DbError> {
    let scopes_json: String = row.try_get("scopes_json")?;
    Ok(Connection {
        catalog_generation: row.try_get("catalog_generation")?,
        id: row.try_get("id")?,
        owner: map_owner(row)?,
        generation: row.try_get("generation")?,
        authorization_generation: row.try_get("authorization_generation")?,
        authorization_expires_at: super::parse_optional_ts(
            row.try_get("authorization_expires_at")?,
            "authorization_expires_at",
        )?,
        connector_key: row.try_get("connector_key")?,
        status: row.try_get("status")?,
        access_token_ct: row.try_get("access_token_ct")?,
        access_token_nonce: row.try_get("access_token_nonce")?,
        refresh_token_ct: row.try_get("refresh_token_ct")?,
        refresh_token_nonce: row.try_get("refresh_token_nonce")?,
        token_expires_at: super::parse_optional_ts(
            row.try_get("token_expires_at")?,
            "token_expires_at",
        )?,
        scopes: serde_json::from_str(&scopes_json).unwrap_or_default(),
        dcr_client_id: row.try_get("dcr_client_id")?,
        dcr_client_secret_ct: row.try_get("dcr_client_secret_ct")?,
        dcr_client_secret_nonce: row.try_get("dcr_client_secret_nonce")?,
        token_url: row.try_get("token_url")?,
        last_error: row.try_get("last_error")?,
        created_at: super::parse_ts(row.try_get("created_at")?, "created_at")?,
        updated_at: super::parse_ts(row.try_get("updated_at")?, "updated_at")?,
    })
}

const CONN_COLS: &str = "catalog_generation, id, user_id, principal_id, generation, authorization_generation, authorization_expires_at, connector_key, status, access_token_ct, \
     access_token_nonce, refresh_token_ct, refresh_token_nonce, token_expires_at, \
     scopes_json, dcr_client_id, dcr_client_secret_ct, dcr_client_secret_nonce, \
     token_url, last_error, created_at, updated_at";

fn map_owner(row: &SqliteRow) -> Result<ConnectionOwner, DbError> {
    match (
        row.try_get::<Option<String>, _>("user_id")?,
        row.try_get::<Option<String>, _>("principal_id")?,
    ) {
        (Some(id), None) => Ok(ConnectionOwner::User(id)),
        (None, Some(id)) => Ok(ConnectionOwner::Principal(id)),
        _ => Err(DbError::Decode {
            column: "connection_owner",
            source: anyhow::anyhow!("connection requires exactly one owner"),
        }),
    }
}
pub struct RefreshTokens<'a> {
    pub access_token_ct: &'a [u8],
    pub access_token_nonce: &'a [u8],
    pub refresh_token_ct: Option<&'a [u8]>,
    pub refresh_token_nonce: Option<&'a [u8]>,
    pub token_expires_at: Option<Timestamp>,
}
#[derive(Debug, Clone)]
pub struct PendingOauth {
    pub catalog_generation: Option<String>,
    pub state: String,
    pub initiator_user_id: String,
    pub owner: ConnectionOwner,
    pub connector_key: String,
    pub subject_principal_id: Option<String>,
    pub connection_id: String,
    pub authorization_generation: String,
    pub expected_connection_generation: String,
    pub expires_at: Timestamp,
    pub pkce_verifier: String,
    pub redirect_uri: String,
    pub token_url: String,
    pub resource: Option<String>,
    pub dcr_client_id: Option<String>,
    pub dcr_client_secret_ct: Option<Vec<u8>>,
    pub dcr_client_secret_nonce: Option<Vec<u8>>,
    pub return_to: Option<String>,
}
pub struct NewAuthorization {
    pub catalog_generation: String,
    pub state: String,
    pub initiator_user_id: String,
    pub owner: ConnectionOwner,
    pub connector_key: String,
    pub subject_principal_id: Option<String>,
    pub pkce_verifier: String,
    pub redirect_uri: String,
    pub token_url: String,
    pub resource: Option<String>,
    pub dcr_client_id: Option<String>,
    pub dcr_client_secret_ct: Option<Vec<u8>>,
    pub dcr_client_secret_nonce: Option<Vec<u8>>,
    pub return_to: Option<String>,
}
fn map_pending(row: &SqliteRow) -> Result<PendingOauth, DbError> {
    Ok(PendingOauth {
        catalog_generation: row.try_get("catalog_generation")?,
        state: row.try_get("state")?,
        initiator_user_id: row.try_get("initiator_user_id")?,
        owner: map_owner(row)?,
        connector_key: row.try_get("connector_key")?,
        subject_principal_id: row.try_get("subject_principal_id")?,
        connection_id: row.try_get("connection_id")?,
        authorization_generation: row.try_get("authorization_generation")?,
        expected_connection_generation: row.try_get("expected_connection_generation")?,
        expires_at: parse_ts(row.try_get("expires_at")?, "expires_at")?,
        pkce_verifier: row.try_get("pkce_verifier")?,
        redirect_uri: row.try_get("redirect_uri")?,
        token_url: row.try_get("token_url")?,
        resource: row.try_get("resource")?,
        dcr_client_id: row.try_get("dcr_client_id")?,
        dcr_client_secret_ct: row.try_get("dcr_client_secret_ct")?,
        dcr_client_secret_nonce: row.try_get("dcr_client_secret_nonce")?,
        return_to: row.try_get("return_to")?,
    })
}
pub async fn get_connection(
    pool: &Pool,
    owner: &ConnectionOwner,
    key: &str,
) -> Result<Option<Connection>, DbError> {
    let (user, principal) = owner.columns();
    let row=sqlx::query(&format!("SELECT {CONN_COLS} FROM mcp_connections WHERE user_id IS ? AND principal_id IS ? AND connector_key=?"))
        .bind(user).bind(principal).bind(key).fetch_optional(pool).await?;
    row.as_ref().map(map_conn).transpose()
}
pub async fn get_connection_in(
    tx: &mut WriteTx,
    owner: &ConnectionOwner,
    key: &str,
) -> Result<Option<Connection>, DbError> {
    let (user, principal) = owner.columns();
    let row=sqlx::query(&format!("SELECT {CONN_COLS} FROM mcp_connections WHERE user_id IS ? AND principal_id IS ? AND connector_key=?"))
        .bind(user).bind(principal).bind(key).fetch_optional(&mut **tx).await?;
    row.as_ref().map(map_conn).transpose()
}
pub async fn list_connections(
    pool: &Pool,
    owner: &ConnectionOwner,
) -> Result<Vec<Connection>, DbError> {
    list_connections_with(pool, owner).await
}

pub async fn list_connections_in(
    tx: &mut WriteTx,
    owner: &ConnectionOwner,
) -> Result<Vec<Connection>, DbError> {
    list_connections_with(&mut **tx, owner).await
}

async fn list_connections_with<'e, E>(
    executor: E,
    owner: &ConnectionOwner,
) -> Result<Vec<Connection>, DbError>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let (user, principal) = owner.columns();
    let rows=sqlx::query(&format!("SELECT {CONN_COLS} FROM mcp_connections WHERE user_id IS ? AND principal_id IS ? ORDER BY connector_key"))
        .bind(user).bind(principal).fetch_all(executor).await?;
    rows.iter().map(map_conn).collect()
}
pub async fn connected_keys(pool: &Pool, owner: &ConnectionOwner) -> Result<Vec<String>, DbError> {
    let (user, principal) = owner.columns();
    let rows = sqlx::query("SELECT c.connector_key FROM mcp_connections c JOIN mcp_catalog_connectors k ON k.key=c.connector_key AND k.dispatch_generation=c.catalog_generation WHERE c.user_id IS ? AND c.principal_id IS ? AND c.status='connected' ORDER BY c.connector_key")
        .bind(user).bind(principal).fetch_all(pool).await?;
    rows.iter()
        .map(|row| row.try_get("connector_key").map_err(DbError::from))
        .collect()
}
pub async fn upsert_connection(
    pool: &Pool,
    new: NewConnection,
) -> Result<Option<ConnectionIdentity>, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let result = upsert_connection_in(&mut tx, new).await?;
    tx.commit().await?;
    Ok(result)
}

pub async fn upsert_connection_in(
    tx: &mut WriteTx,
    new: NewConnection,
) -> Result<Option<ConnectionIdentity>, DbError> {
    if !super::mcp_catalog::dispatch_matches_in(tx, &new.connector_key, &new.catalog_generation)
        .await?
    {
        return Ok(None);
    }
    let current = get_connection_in(tx, &new.owner, &new.connector_key).await?;
    let id = current
        .map(|c| c.id)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let generation = uuid::Uuid::new_v4().to_string();
    let now = Timestamp::now().to_string();
    let (user, principal) = new.owner.columns();
    sqlx::query("DELETE FROM pending_mcp_oauth WHERE user_id IS ? AND principal_id IS ? AND connector_key=?")
        .bind(user).bind(principal).bind(&new.connector_key).execute(&mut **tx).await?;
    sqlx::query("INSERT INTO mcp_connections(id,user_id,principal_id,connector_key,generation,catalog_generation,status,access_token_ct,access_token_nonce,refresh_token_ct,refresh_token_nonce,token_expires_at,scopes_json,dcr_client_id,dcr_client_secret_ct,dcr_client_secret_nonce,token_url,created_at,updated_at) VALUES(?,?,?,?,?,?,'connected',?,?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET generation=excluded.generation,catalog_generation=excluded.catalog_generation,status='connected',authorization_generation=NULL,authorization_expires_at=NULL,access_token_ct=excluded.access_token_ct,access_token_nonce=excluded.access_token_nonce,refresh_token_ct=excluded.refresh_token_ct,refresh_token_nonce=excluded.refresh_token_nonce,token_expires_at=excluded.token_expires_at,scopes_json=excluded.scopes_json,dcr_client_id=excluded.dcr_client_id,dcr_client_secret_ct=excluded.dcr_client_secret_ct,dcr_client_secret_nonce=excluded.dcr_client_secret_nonce,token_url=excluded.token_url,last_error=NULL,updated_at=excluded.updated_at")
        .bind(&id).bind(user).bind(principal).bind(&new.connector_key).bind(&generation).bind(&new.catalog_generation)
        .bind(&new.access_token_ct).bind(&new.access_token_nonce).bind(&new.refresh_token_ct).bind(&new.refresh_token_nonce)
        .bind(new.token_expires_at.map(|t|t.to_string())).bind(serde_json::to_string(&new.scopes).expect("string vector serializes"))
        .bind(&new.dcr_client_id).bind(&new.dcr_client_secret_ct).bind(&new.dcr_client_secret_nonce).bind(&new.token_url).bind(&now).bind(&now)
        .execute(&mut **tx).await?;
    Ok(Some(ConnectionIdentity {
        owner: new.owner,
        connector_key: new.connector_key,
        connection_id: id,
        generation,
    }))
}
async fn credential_catalog_matches_in(
    tx: &mut WriteTx,
    key: &str,
    generation: &str,
    allowed_auth: &[super::mcp_catalog::AuthKind],
) -> Result<bool, DbError> {
    Ok(super::mcp_catalog::get_in(tx, key).await?.is_some_and(|c| {
        c.dispatch_generation == generation
            && c.enabled
            && c.is_per_user()
            && allowed_auth.contains(&c.auth)
    }))
}

pub async fn update_tokens(
    pool: &Pool,
    expected: &ConnectionIdentity,
    expected_catalog_generation: &str,
    tokens: RefreshTokens<'_>,
) -> Result<bool, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    if !credential_catalog_matches_in(
        &mut tx,
        &expected.connector_key,
        expected_catalog_generation,
        &[super::mcp_catalog::AuthKind::OAuth2],
    )
    .await?
    {
        tx.commit().await?;
        return Ok(false);
    }
    let (user, principal) = expected.owner.columns();
    let changed=sqlx::query("UPDATE mcp_connections SET status='connected',access_token_ct=?,access_token_nonce=?,refresh_token_ct=CASE WHEN ? THEN ? ELSE refresh_token_ct END,refresh_token_nonce=CASE WHEN ? THEN ? ELSE refresh_token_nonce END,token_expires_at=?,last_error=NULL,updated_at=? WHERE id=? AND user_id IS ? AND principal_id IS ? AND connector_key=? AND generation=? AND status='connected' AND catalog_generation=?")
        .bind(tokens.access_token_ct).bind(tokens.access_token_nonce).bind(tokens.refresh_token_ct.is_some()).bind(tokens.refresh_token_ct)
        .bind(tokens.refresh_token_ct.is_some()).bind(tokens.refresh_token_nonce).bind(tokens.token_expires_at.map(|t|t.to_string())).bind(Timestamp::now().to_string())
        .bind(&expected.connection_id).bind(user).bind(principal).bind(&expected.connector_key).bind(&expected.generation).bind(expected_catalog_generation).execute(&mut *tx).await?.rows_affected();
    tx.commit().await?;
    Ok(changed == 1)
}
pub async fn mark_error(
    pool: &Pool,
    expected: &ConnectionIdentity,
    expected_catalog_generation: &str,
    error: &str,
    needs_reauth: bool,
) -> Result<bool, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    if !credential_catalog_matches_in(
        &mut tx,
        &expected.connector_key,
        expected_catalog_generation,
        &[
            super::mcp_catalog::AuthKind::OAuth2,
            super::mcp_catalog::AuthKind::StaticBearer,
        ],
    )
    .await?
    {
        tx.commit().await?;
        return Ok(false);
    }
    let (user, principal) = expected.owner.columns();
    let changed=sqlx::query("UPDATE mcp_connections SET status=?,last_error=?,updated_at=? WHERE id=? AND user_id IS ? AND principal_id IS ? AND connector_key=? AND generation=? AND status='connected' AND catalog_generation=?")
        .bind(if needs_reauth {STATUS_REAUTH}else{STATUS_ERROR}).bind(error).bind(Timestamp::now().to_string())
        .bind(&expected.connection_id).bind(user).bind(principal).bind(&expected.connector_key).bind(&expected.generation).bind(expected_catalog_generation).execute(&mut *tx).await?.rows_affected();
    tx.commit().await?;
    Ok(changed == 1)
}
pub async fn delete_connection_in(
    tx: &mut WriteTx,
    expected: &ConnectionIdentity,
) -> Result<bool, DbError> {
    let (user, principal) = expected.owner.columns();
    let changed=sqlx::query("DELETE FROM mcp_connections WHERE id=? AND user_id IS ? AND principal_id IS ? AND connector_key=? AND generation=?")
        .bind(&expected.connection_id).bind(user).bind(principal).bind(&expected.connector_key).bind(&expected.generation)
        .execute(&mut **tx).await?.rows_affected();
    Ok(changed == 1)
}
pub async fn delete_connection(
    pool: &Pool,
    owner: &ConnectionOwner,
    key: &str,
) -> Result<bool, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let (user, principal) = owner.columns();
    sqlx::query("DELETE FROM pending_mcp_oauth WHERE user_id IS ? AND principal_id IS ? AND connector_key=?").bind(user).bind(principal).bind(key).execute(&mut *tx).await?;
    let n = sqlx::query(
        "DELETE FROM mcp_connections WHERE user_id IS ? AND principal_id IS ? AND connector_key=?",
    )
    .bind(user)
    .bind(principal)
    .bind(key)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    tx.commit().await?;
    Ok(n > 0)
}
pub async fn delete_all_for_connector(pool: &Pool, key: &str) -> Result<u64, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    sqlx::query("DELETE FROM pending_mcp_oauth WHERE connector_key=?")
        .bind(key)
        .execute(&mut *tx)
        .await?;
    let n = sqlx::query("DELETE FROM mcp_connections WHERE connector_key=?")
        .bind(key)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    sqlx::query("DELETE FROM user_mcp_tool_prefs WHERE connector_key=?")
        .bind(key)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM token_mcp_policy WHERE connector_key=?")
        .bind(key)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(n)
}
pub async fn connections_due_for_refresh(
    pool: &Pool,
    expiring_before: Timestamp,
    stale_before: Timestamp,
) -> Result<Vec<Connection>, DbError> {
    let rows=sqlx::query(&format!("SELECT {CONN_COLS} FROM mcp_connections WHERE status='connected' AND refresh_token_ct IS NOT NULL AND EXISTS(SELECT 1 FROM mcp_catalog_connectors k WHERE k.key=mcp_connections.connector_key AND k.dispatch_generation=mcp_connections.catalog_generation AND k.enabled=1 AND k.scope='per_user' AND k.auth='oauth2')")).fetch_all(pool).await?;
    let connections: Vec<Connection> = rows.iter().map(map_conn).collect::<Result<_, _>>()?;
    Ok(connections
        .into_iter()
        .filter(|c| {
            c.token_expires_at
                .map(|t| t <= expiring_before)
                .unwrap_or(c.updated_at <= stale_before)
        })
        .collect())
}
const PENDING_TTL_SECS: i64 = 600;
pub async fn begin_authorization(
    pool: &Pool,
    new: NewAuthorization,
) -> Result<Option<PendingOauth>, DbError> {
    begin_authorization_at(pool, new, Timestamp::now()).await
}
async fn begin_authorization_at(
    pool: &Pool,
    new: NewAuthorization,
    now: Timestamp,
) -> Result<Option<PendingOauth>, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let pending = begin_authorization_at_in(&mut tx, new, now).await?;
    tx.commit().await?;
    Ok(pending)
}
pub async fn begin_authorization_in(
    tx: &mut WriteTx,
    new: NewAuthorization,
) -> Result<Option<PendingOauth>, DbError> {
    begin_authorization_at_in(tx, new, Timestamp::now()).await
}
async fn begin_authorization_at_in(
    tx: &mut WriteTx,
    new: NewAuthorization,
    now: Timestamp,
) -> Result<Option<PendingOauth>, DbError> {
    if !super::mcp_catalog::dispatch_matches_in(tx, &new.connector_key, &new.catalog_generation)
        .await?
    {
        return Ok(None);
    }
    let expires = now + jiff::Span::new().seconds(PENDING_TTL_SECS);
    let current = get_connection_in(tx, &new.owner, &new.connector_key).await?;
    let (id, generation) = current.map(|c| (c.id, c.generation)).unwrap_or_else(|| {
        (
            uuid::Uuid::new_v4().to_string(),
            uuid::Uuid::new_v4().to_string(),
        )
    });
    let authorization_generation = uuid::Uuid::new_v4().to_string();
    let (user, principal) = new.owner.columns();
    sqlx::query("DELETE FROM pending_mcp_oauth WHERE user_id IS ? AND principal_id IS ? AND connector_key=?").bind(user).bind(principal).bind(&new.connector_key).execute(&mut **tx).await?;
    sqlx::query("INSERT INTO mcp_connections(id,user_id,principal_id,connector_key,generation,catalog_generation,authorization_generation,authorization_expires_at,status,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,'pending',?,?) ON CONFLICT(id) DO UPDATE SET authorization_generation=excluded.authorization_generation,authorization_expires_at=excluded.authorization_expires_at")
        .bind(&id).bind(user).bind(principal).bind(&new.connector_key).bind(&generation).bind(&new.catalog_generation).bind(&authorization_generation).bind(expires.to_string()).bind(now.to_string()).bind(now.to_string()).execute(&mut **tx).await?;
    sqlx::query("INSERT INTO pending_mcp_oauth(state,initiator_user_id,user_id,principal_id,subject_principal_id,connector_key,connection_id,authorization_generation,expected_connection_generation,pkce_verifier,redirect_uri,token_url,resource,dcr_client_id,dcr_client_secret_ct,dcr_client_secret_nonce,return_to,created_at,expires_at,catalog_generation) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)")
        .bind(&new.state).bind(&new.initiator_user_id).bind(user).bind(principal).bind(&new.subject_principal_id).bind(&new.connector_key).bind(&id).bind(&authorization_generation).bind(&generation)
        .bind(&new.pkce_verifier).bind(&new.redirect_uri).bind(&new.token_url).bind(&new.resource).bind(&new.dcr_client_id).bind(&new.dcr_client_secret_ct).bind(&new.dcr_client_secret_nonce).bind(&new.return_to).bind(now.to_string()).bind(expires.to_string()).bind(&new.catalog_generation).execute(&mut **tx).await?;
    Ok(Some(PendingOauth {
        catalog_generation: Some(new.catalog_generation),
        state: new.state,
        initiator_user_id: new.initiator_user_id,
        owner: new.owner,
        connector_key: new.connector_key,
        subject_principal_id: new.subject_principal_id,
        connection_id: id,
        authorization_generation,
        expected_connection_generation: generation,
        expires_at: expires,
        pkce_verifier: new.pkce_verifier,
        redirect_uri: new.redirect_uri,
        token_url: new.token_url,
        resource: new.resource,
        dcr_client_id: new.dcr_client_id,
        dcr_client_secret_ct: new.dcr_client_secret_ct,
        dcr_client_secret_nonce: new.dcr_client_secret_nonce,
        return_to: new.return_to,
    }))
}
pub async fn take_pending(pool: &Pool, state: &str) -> Result<Option<PendingOauth>, DbError> {
    take_pending_at(pool, state, Timestamp::now()).await
}
async fn take_pending_at(
    pool: &Pool,
    state: &str,
    now: Timestamp,
) -> Result<Option<PendingOauth>, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let row = sqlx::query("DELETE FROM pending_mcp_oauth WHERE state=? RETURNING *")
        .bind(state)
        .fetch_optional(&mut *tx)
        .await?;
    let Some(row) = row else {
        tx.commit().await?;
        return Ok(None);
    };
    let pending = map_pending(&row)?;
    if now >= pending.expires_at {
        cancel_authorization_in(&mut tx, &pending).await?;
        tx.commit().await?;
        return Ok(None);
    }
    tx.commit().await?;
    Ok(Some(pending))
}
pub async fn sweep_expired_pending(pool: &Pool) -> Result<u64, DbError> {
    sweep_expired_pending_at(pool, Timestamp::now()).await
}
async fn sweep_expired_pending_at(pool: &Pool, now: Timestamp) -> Result<u64, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let mut swept = 0;
    let mut after = 0_i64;
    loop {
        let rows=sqlx::query("SELECT rowid,id,status,authorization_generation,authorization_expires_at FROM mcp_connections WHERE rowid>? AND authorization_generation IS NOT NULL ORDER BY rowid LIMIT 128").bind(after).fetch_all(&mut *tx).await?;
        if rows.is_empty() {
            break;
        }
        for row in rows {
            after = row.try_get("rowid")?;
            let deadline = parse_ts(
                row.try_get("authorization_expires_at")?,
                "authorization_expires_at",
            )?;
            if now < deadline {
                continue;
            }
            let id: String = row.try_get("id")?;
            let auth: String = row.try_get("authorization_generation")?;
            sqlx::query("DELETE FROM pending_mcp_oauth WHERE connection_id=? AND authorization_generation=?").bind(&id).bind(&auth).execute(&mut *tx).await?;
            if row.try_get::<String, _>("status")? == STATUS_PENDING {
                sqlx::query(
                    "DELETE FROM mcp_connections WHERE id=? AND authorization_generation=?",
                )
                .bind(&id)
                .bind(&auth)
                .execute(&mut *tx)
                .await?;
            } else {
                sqlx::query("UPDATE mcp_connections SET authorization_generation=NULL,authorization_expires_at=NULL WHERE id=? AND authorization_generation=?").bind(&id).bind(&auth).execute(&mut *tx).await?;
            }
            swept += 1;
        }
    }
    after = 0;
    loop {
        let rows=sqlx::query("SELECT rowid,state,expires_at FROM pending_mcp_oauth WHERE rowid>? ORDER BY rowid LIMIT 128").bind(after).fetch_all(&mut *tx).await?;
        if rows.is_empty() {
            break;
        }
        for row in rows {
            after = row.try_get("rowid")?;
            if now >= parse_ts(row.try_get("expires_at")?, "expires_at")? {
                sqlx::query("DELETE FROM pending_mcp_oauth WHERE state=?")
                    .bind(row.try_get::<String, _>("state")?)
                    .execute(&mut *tx)
                    .await?;
                swept += 1;
            }
        }
    }
    tx.commit().await?;
    Ok(swept)
}
pub async fn authorization_is_current_in(
    tx: &mut WriteTx,
    pending: &PendingOauth,
) -> Result<bool, DbError> {
    let Some(generation) = pending.catalog_generation.as_deref() else {
        return Ok(false);
    };
    Ok(
        super::mcp_catalog::dispatch_matches_in(tx, &pending.connector_key, generation).await?
            && reservation_matches_in(tx, pending, Some(Timestamp::now())).await?,
    )
}
async fn reservation_matches_in(
    conn: &mut WriteTx,
    pending: &PendingOauth,
    now: Option<Timestamp>,
) -> Result<bool, DbError> {
    let (user, principal) = pending.owner.columns();
    let row=sqlx::query("SELECT authorization_expires_at FROM mcp_connections WHERE id=? AND user_id IS ? AND principal_id IS ? AND connector_key=? AND generation=? AND authorization_generation=?")
        .bind(&pending.connection_id).bind(user).bind(principal).bind(&pending.connector_key)
        .bind(&pending.expected_connection_generation).bind(&pending.authorization_generation)
        .fetch_optional(&mut **conn).await?;
    let Some(row) = row else { return Ok(false) };
    let expires = parse_ts(
        row.try_get("authorization_expires_at")?,
        "authorization_expires_at",
    )?;
    Ok(expires == pending.expires_at && now.is_none_or(|now| now < expires))
}

pub async fn cancel_authorization(pool: &Pool, pending: &PendingOauth) -> Result<bool, DbError> {
    let mut tx = WriteTx::begin(pool).await?;
    let cancelled = cancel_authorization_in(&mut tx, pending).await?;
    tx.commit().await?;
    Ok(cancelled)
}
pub async fn cancel_authorization_in(
    conn: &mut WriteTx,
    pending: &PendingOauth,
) -> Result<bool, DbError> {
    if !reservation_matches_in(conn, pending, None).await? {
        return Ok(false);
    }
    let (user, principal) = pending.owner.columns();
    let removed=sqlx::query("DELETE FROM mcp_connections WHERE id=? AND user_id IS ? AND principal_id IS ? AND connector_key=? AND generation=? AND authorization_generation=? AND status='pending'")
        .bind(&pending.connection_id).bind(user).bind(principal).bind(&pending.connector_key)
        .bind(&pending.expected_connection_generation).bind(&pending.authorization_generation)
        .execute(&mut **conn).await?.rows_affected();
    if removed == 0 {
        sqlx::query("UPDATE mcp_connections SET authorization_generation=NULL,authorization_expires_at=NULL WHERE id=? AND user_id IS ? AND principal_id IS ? AND connector_key=? AND generation=? AND authorization_generation=?")
            .bind(&pending.connection_id).bind(user).bind(principal).bind(&pending.connector_key)
            .bind(&pending.expected_connection_generation).bind(&pending.authorization_generation)
            .execute(&mut **conn).await?;
    }
    sqlx::query("DELETE FROM pending_mcp_oauth WHERE connection_id=? AND authorization_generation=? AND expected_connection_generation=?")
        .bind(&pending.connection_id).bind(&pending.authorization_generation).bind(&pending.expected_connection_generation)
        .execute(&mut **conn).await?;
    Ok(true)
}

pub async fn complete_authorization_in(
    conn: &mut WriteTx,
    pending: &PendingOauth,
    new: NewConnection,
) -> Result<Option<ConnectionIdentity>, DbError> {
    complete_authorization_at_in(conn, pending, new, Timestamp::now()).await
}
async fn complete_authorization_at_in(
    conn: &mut WriteTx,
    pending: &PendingOauth,
    new: NewConnection,
    now: Timestamp,
) -> Result<Option<ConnectionIdentity>, DbError> {
    if pending.catalog_generation.as_deref() != Some(new.catalog_generation.as_str())
        || !super::mcp_catalog::dispatch_matches_in(
            conn,
            &pending.connector_key,
            &new.catalog_generation,
        )
        .await?
        || new.owner != pending.owner
        || new.connector_key != pending.connector_key
        || !reservation_matches_in(conn, pending, Some(now)).await?
    {
        return Ok(None);
    }
    let generation = uuid::Uuid::new_v4().to_string();
    let (user, principal) = pending.owner.columns();
    let changed=sqlx::query("UPDATE mcp_connections SET status='connected',generation=?,catalog_generation=?,authorization_generation=NULL,authorization_expires_at=NULL,access_token_ct=?,access_token_nonce=?,refresh_token_ct=?,refresh_token_nonce=?,token_expires_at=?,token_url=?,scopes_json=?,dcr_client_id=?,dcr_client_secret_ct=?,dcr_client_secret_nonce=?,last_error=NULL,updated_at=? WHERE id=? AND user_id IS ? AND principal_id IS ? AND connector_key=? AND generation=? AND authorization_generation=?")
        .bind(&generation).bind(&new.catalog_generation).bind(&new.access_token_ct).bind(&new.access_token_nonce)
        .bind(&new.refresh_token_ct).bind(&new.refresh_token_nonce).bind(new.token_expires_at.map(|t|t.to_string()))
        .bind(&new.token_url).bind(serde_json::to_string(&new.scopes).expect("string vector serializes"))
        .bind(&new.dcr_client_id).bind(&new.dcr_client_secret_ct).bind(&new.dcr_client_secret_nonce).bind(now.to_string())
        .bind(&pending.connection_id).bind(user).bind(principal).bind(&pending.connector_key)
        .bind(&pending.expected_connection_generation).bind(&pending.authorization_generation)
        .execute(&mut **conn).await?.rows_affected();
    if changed != 1 {
        return Ok(None);
    }
    sqlx::query(
        "DELETE FROM pending_mcp_oauth WHERE connection_id=? AND authorization_generation=?",
    )
    .bind(&pending.connection_id)
    .bind(&pending.authorization_generation)
    .execute(&mut **conn)
    .await?;
    Ok(Some(ConnectionIdentity {
        owner: pending.owner.clone(),
        connector_key: pending.connector_key.clone(),
        connection_id: pending.connection_id.clone(),
        generation,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::db::{self, WriteTx};
    use jiff::Timestamp;

    async fn pool() -> Pool {
        seed_pool(db::open(std::path::Path::new(":memory:")).await.unwrap()).await
    }

    async fn seed_pool(p: Pool) -> Pool {
        sqlx::query("INSERT INTO users(id,email,roles_json,created_at,updated_at) VALUES('same','same@example.test','[]','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')").execute(&p).await.unwrap();
        sqlx::query("INSERT INTO system_principals(id,name,display,created_by,created_at) VALUES('same','same','Same','same','2026-01-01T00:00:00Z')").execute(&p).await.unwrap();
        sqlx::query("INSERT INTO mcp_catalog_connectors(key,name,url,auth,scope,enabled,created_at,updated_at,dispatch_generation) VALUES('gmail','Gmail','https://provider.test/mcp','oauth2','per_user',1,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z','fixture-catalog-generation')").execute(&p).await.unwrap();
        p
    }
    #[tokio::test]
    async fn owned_connection_inventory_uses_the_authority_transaction_with_one_connection() {
        let p = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                sqlx::sqlite::SqliteConnectOptions::new()
                    .filename(":memory:")
                    .foreign_keys(true),
            )
            .await
            .unwrap();
        db::migrate(&p).await.unwrap();
        let p = seed_pool(p).await;
        let personal = upsert_connection(&p, credential(user(), 1))
            .await
            .unwrap()
            .unwrap();
        let owned = upsert_connection(&p, credential(principal(), 2))
            .await
            .unwrap()
            .unwrap();
        let mut tx = WriteTx::begin(&p).await.unwrap();
        sqlx::query("UPDATE mcp_connections SET status='reauth' WHERE principal_id='same'")
            .execute(&mut *tx)
            .await
            .unwrap();
        let rows = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            list_connections_in(&mut tx, &principal()),
        )
        .await
        .expect("inventory must use the held authority connection")
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].identity(), owned);
        assert_eq!(rows[0].status, STATUS_REAUTH);
        let rows = list_connections_in(&mut tx, &user()).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].identity(), personal);
        assert_eq!(rows[0].status, STATUS_CONNECTED);
        assert!(
            list_connections_in(&mut tx, &ConnectionOwner::Principal("missing".into()))
                .await
                .unwrap()
                .is_empty()
        );
        drop(tx);
        let rows = list_connections(&p, &principal()).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].identity(), owned);
        assert_eq!(rows[0].status, STATUS_CONNECTED);
        p.close().await;
    }

    fn user() -> ConnectionOwner {
        ConnectionOwner::User("same".into())
    }
    fn principal() -> ConnectionOwner {
        ConnectionOwner::Principal("same".into())
    }
    fn timestamp(s: &str) -> Timestamp {
        s.parse().unwrap()
    }
    fn credential(owner: ConnectionOwner, value: u8) -> NewConnection {
        NewConnection {
            catalog_generation: "fixture-catalog-generation".into(),
            owner,
            connector_key: "gmail".into(),
            access_token_ct: vec![value, 0, 255],
            access_token_nonce: vec![2, 0],
            refresh_token_ct: Some(vec![3, 0, 254]),
            refresh_token_nonce: Some(vec![4]),
            token_expires_at: Some(timestamp("2030-01-01T00:00:00Z")),
            scopes: vec!["mail.read".into()],
            dcr_client_id: Some("client".into()),
            dcr_client_secret_ct: Some(vec![5, 0]),
            dcr_client_secret_nonce: Some(vec![6]),
            token_url: Some("https://provider.test/token".into()),
        }
    }
    fn authorization(owner: ConnectionOwner, state: &str) -> NewAuthorization {
        NewAuthorization {
            catalog_generation: "fixture-catalog-generation".into(),
            state: state.into(),
            initiator_user_id: "same".into(),
            subject_principal_id: match &owner {
                ConnectionOwner::Principal(id) => Some(id.clone()),
                _ => None,
            },
            owner,
            connector_key: "gmail".into(),
            pkce_verifier: "verifier".into(),
            redirect_uri: "https://gateway.test/integrations/callback".into(),
            token_url: "https://provider.test/token".into(),
            resource: Some("https://provider.test/mcp".into()),
            dcr_client_id: Some("client".into()),
            dcr_client_secret_ct: Some(vec![5]),
            dcr_client_secret_nonce: Some(vec![6]),
            return_to: None,
        }
    }
    async fn complete(p: &Pool, pending: &PendingOauth, value: u8) -> Option<ConnectionIdentity> {
        let mut tx = WriteTx::begin(p).await.unwrap();
        let result =
            complete_authorization_in(&mut tx, pending, credential(pending.owner.clone(), value))
                .await
                .unwrap();
        tx.commit().await.unwrap();
        result
    }
    #[tokio::test]
    async fn equal_owner_ids_are_independent_and_sealed_bytes_round_trip() {
        let p = pool().await;
        let a = upsert_connection(&p, credential(user(), 1))
            .await
            .unwrap()
            .unwrap();
        let b = upsert_connection(&p, credential(principal(), 7))
            .await
            .unwrap()
            .unwrap();
        assert_ne!(a.connection_id, b.connection_id);
        for (owner, value) in [(user(), 1), (principal(), 7)] {
            let c = get_connection(&p, &owner, "gmail").await.unwrap().unwrap();
            assert_eq!(c.access_token_ct, Some(vec![value, 0, 255]));
            assert_eq!(c.refresh_token_ct, Some(vec![3, 0, 254]));
            assert_eq!(c.dcr_client_secret_ct, Some(vec![5, 0]));
            assert_eq!(connected_keys(&p, &owner).await.unwrap(), vec!["gmail"]);
        }
        assert!(
            sqlx::query("UPDATE mcp_connections SET principal_id='same' WHERE user_id='same'")
                .execute(&p)
                .await
                .is_err()
        );
        assert!(
            sqlx::query("UPDATE mcp_connections SET user_id=NULL WHERE user_id='same'")
                .execute(&p)
                .await
                .is_err()
        );
    }
    #[tokio::test]
    async fn deleting_authorization_initiator_cancels_only_matching_principal_reservations() {
        for retained in [false, true] {
            let p = pool().await;
            if retained {
                upsert_connection(&p, credential(principal(), 7))
                    .await
                    .unwrap();
            }
            let pending = begin_authorization(&p, authorization(principal(), "initial"))
                .await
                .unwrap()
                .unwrap();
            sqlx::query("DELETE FROM users WHERE id='same'")
                .execute(&p)
                .await
                .unwrap();
            assert!(take_pending(&p, "initial").await.unwrap().is_none());
            let saved = get_connection(&p, &principal(), "gmail").await.unwrap();
            if retained {
                let saved = saved.unwrap();
                assert_eq!(saved.id, pending.connection_id);
                assert_eq!(saved.generation, pending.expected_connection_generation);
                assert_eq!(saved.access_token_ct, Some(vec![7, 0, 255]));
                assert!(saved.authorization_generation.is_none());
                assert!(saved.authorization_expires_at.is_none());
            } else {
                assert!(saved.is_none());
            }
            p.close().await;
        }
    }
    #[tokio::test]
    async fn consumed_initial_reservation_expires_without_pending_state() {
        let p = pool().await;
        let pending = begin_authorization(&p, authorization(principal(), "initial"))
            .await
            .unwrap()
            .unwrap();
        let taken = take_pending(&p, "initial").await.unwrap().unwrap();
        assert_eq!(taken.expires_at, pending.expires_at);
        assert!(take_pending(&p, "initial").await.unwrap().is_none());
        let mut tx = WriteTx::begin(&p).await.unwrap();
        assert!(
            complete_authorization_at_in(
                &mut tx,
                &taken,
                credential(principal(), 9),
                taken.expires_at
            )
            .await
            .unwrap()
            .is_none()
        );
        tx.commit().await.unwrap();
        assert_eq!(
            sweep_expired_pending_at(&p, taken.expires_at)
                .await
                .unwrap(),
            1
        );
        assert!(
            get_connection(&p, &principal(), "gmail")
                .await
                .unwrap()
                .is_none()
        );
    }
    #[tokio::test]
    async fn deadline_comparisons_are_strict_and_fractional() {
        for now in [
            "2030-01-01T00:00:00Z",
            "2030-01-01T00:00:00.1Z",
            "2030-01-01T00:00:00.100000001Z",
        ] {
            let p = pool().await;
            let pending = begin_authorization_at(
                &p,
                authorization(user(), "deadline"),
                timestamp("2029-12-31T23:50:00.1Z"),
            )
            .await
            .unwrap()
            .unwrap();
            assert_eq!(pending.expires_at, timestamp("2030-01-01T00:00:00.1Z"));
            let taken = take_pending_at(&p, "deadline", timestamp(now))
                .await
                .unwrap();
            assert_eq!(taken.is_some(), timestamp(now) < pending.expires_at);
        }
    }
    #[tokio::test]
    async fn expired_reauthorization_preserves_saved_credential() {
        let p = pool().await;
        let identity = upsert_connection(&p, credential(user(), 1))
            .await
            .unwrap()
            .unwrap();
        let before = get_connection(&p, &user(), "gmail").await.unwrap().unwrap();
        let pending = begin_authorization(&p, authorization(user(), "reauth"))
            .await
            .unwrap()
            .unwrap();
        take_pending(&p, "reauth").await.unwrap().unwrap();
        assert_eq!(
            sweep_expired_pending_at(&p, pending.expires_at)
                .await
                .unwrap(),
            1
        );
        let after = get_connection(&p, &user(), "gmail").await.unwrap().unwrap();
        assert_eq!(after.generation, identity.generation);
        assert_eq!(after.status, before.status);
        assert_eq!(after.access_token_ct, before.access_token_ct);
        assert_eq!(after.refresh_token_ct, before.refresh_token_ct);
        assert_eq!(after.updated_at, before.updated_at);
        assert!(after.authorization_generation.is_none());
        assert!(after.authorization_expires_at.is_none());
    }
    #[tokio::test]
    async fn stale_cancel_and_completion_cannot_change_new_authorization() {
        let p = pool().await;
        let old = begin_authorization(&p, authorization(user(), "old"))
            .await
            .unwrap()
            .unwrap();
        take_pending(&p, "old").await.unwrap().unwrap();
        let new = begin_authorization(&p, authorization(user(), "new"))
            .await
            .unwrap()
            .unwrap();
        assert!(!cancel_authorization(&p, &old).await.unwrap());
        assert!(complete(&p, &old, 8).await.is_none());
        assert!(complete(&p, &new, 9).await.is_some());
        assert_eq!(
            get_connection(&p, &user(), "gmail")
                .await
                .unwrap()
                .unwrap()
                .access_token_ct,
            Some(vec![9, 0, 255])
        );
    }
    #[tokio::test]
    async fn disconnect_or_immediate_replacement_defeats_consumed_completion() {
        for replace in [false, true] {
            let p = pool().await;
            let pending = begin_authorization(&p, authorization(user(), "race"))
                .await
                .unwrap()
                .unwrap();
            take_pending(&p, "race").await.unwrap().unwrap();
            assert!(delete_connection(&p, &user(), "gmail").await.unwrap());
            if replace {
                upsert_connection(&p, credential(user(), 7))
                    .await
                    .unwrap()
                    .unwrap();
            }
            assert!(complete(&p, &pending, 9).await.is_none());
            let c = get_connection(&p, &user(), "gmail").await.unwrap();
            assert_eq!(c.is_some(), replace);
            if let Some(c) = c {
                assert_eq!(c.access_token_ct, Some(vec![7, 0, 255]));
            }
        }
    }
    #[tokio::test]
    async fn refresh_and_error_cas_preserve_new_identity_and_optional_refresh() {
        let p = pool().await;
        let old = upsert_connection(&p, credential(user(), 1))
            .await
            .unwrap()
            .unwrap();
        let tokens = || RefreshTokens {
            access_token_ct: &[8],
            access_token_nonce: &[9],
            refresh_token_ct: None,
            refresh_token_nonce: None,
            token_expires_at: None,
        };
        assert!(
            update_tokens(&p, &old, "fixture-catalog-generation", tokens())
                .await
                .unwrap()
        );
        let c = get_connection(&p, &user(), "gmail").await.unwrap().unwrap();
        assert_eq!(c.generation, old.generation);
        assert_eq!(c.refresh_token_ct, Some(vec![3, 0, 254]));
        let new = upsert_connection(&p, credential(user(), 7))
            .await
            .unwrap()
            .unwrap();
        assert_ne!(new.generation, old.generation);
        assert!(
            !update_tokens(&p, &old, "fixture-catalog-generation", tokens())
                .await
                .unwrap()
        );
        assert!(
            !mark_error(
                &p,
                &old,
                "fixture-catalog-generation",
                "stale provider failure",
                true
            )
            .await
            .unwrap()
        );
        let c = get_connection(&p, &user(), "gmail").await.unwrap().unwrap();
        assert_eq!(c.status, STATUS_CONNECTED);
        assert_eq!(c.access_token_ct, Some(vec![7, 0, 255]));
    }
    #[tokio::test]
    async fn cancelled_reservation_cannot_be_replayed_and_transaction_rollback_is_atomic() {
        let p = pool().await;
        let mut tx = WriteTx::begin(&p).await.unwrap();
        let pending = begin_authorization_in(&mut tx, authorization(principal(), "rollback"))
            .await
            .unwrap()
            .unwrap();
        drop(tx);
        assert!(
            get_connection(&p, &principal(), "gmail")
                .await
                .unwrap()
                .is_none()
        );
        assert!(take_pending(&p, "rollback").await.unwrap().is_none());
        assert!(complete(&p, &pending, 1).await.is_none());
        let pending = begin_authorization(&p, authorization(principal(), "cancel"))
            .await
            .unwrap()
            .unwrap();
        take_pending(&p, "cancel").await.unwrap().unwrap();
        assert!(cancel_authorization(&p, &pending).await.unwrap());
        assert!(!cancel_authorization(&p, &pending).await.unwrap());
        assert!(complete(&p, &pending, 1).await.is_none());
    }
    #[tokio::test]
    async fn due_refresh_and_catalog_delete_cover_all_owners_and_policy_cleanup() {
        let p = pool().await;
        for owner in [user(), principal()] {
            upsert_connection(&p, credential(owner, 1))
                .await
                .unwrap()
                .unwrap();
        }
        assert!(
            connections_due_for_refresh(
                &p,
                timestamp("2029-01-01T00:00:00Z"),
                timestamp("2000-01-01T00:00:00Z")
            )
            .await
            .unwrap()
            .is_empty()
        );
        let due = connections_due_for_refresh(
            &p,
            timestamp("2031-01-01T00:00:00Z"),
            timestamp("2000-01-01T00:00:00Z"),
        )
        .await
        .unwrap();
        assert_eq!(due.len(), 2);
        let saved = list_connections(&p, &user()).await.unwrap();
        assert_eq!(saved.len(), 1);
        assert!(
            mark_error(
                &p,
                &saved[0].identity(),
                "fixture-catalog-generation",
                "provider rejected credential",
                true
            )
            .await
            .unwrap()
        );
        let saved = get_connection(&p, &user(), "gmail").await.unwrap().unwrap();
        assert!(saved.is_errored());
        assert!(saved.needs_reauth());
        assert_eq!(
            connections_due_for_refresh(
                &p,
                timestamp("2031-01-01T00:00:00Z"),
                timestamp("2000-01-01T00:00:00Z")
            )
            .await
            .unwrap()
            .len(),
            1
        );
        crate::server::db::user_mcp::set_tool_mode(
            &p,
            "same",
            "gmail",
            "write",
            crate::server::db::user_mcp::ToolMode::Off,
        )
        .await
        .unwrap();
        begin_authorization(&p, authorization(principal(), "cleanup"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(delete_all_for_connector(&p, "gmail").await.unwrap(), 2);
        assert!(take_pending(&p, "cleanup").await.unwrap().is_none());
        assert!(
            crate::server::db::user_mcp::tool_modes(&p, "same", "gmail")
                .await
                .unwrap()
                .is_empty()
        );
    }
    #[tokio::test]
    async fn concurrent_pending_consumers_return_exactly_one_full_authorization() {
        let p = pool().await;
        let expected = begin_authorization(&p, authorization(principal(), "one-shot"))
            .await
            .unwrap()
            .unwrap();
        let mut tasks = tokio::task::JoinSet::new();
        for _ in 0..8 {
            let pool = p.clone();
            tasks.spawn(async move { take_pending(&pool, "one-shot").await.unwrap() });
        }
        let mut returned = Vec::new();
        while let Some(result) = tasks.join_next().await {
            if let Some(value) = result.unwrap() {
                returned.push(value);
            }
        }
        assert_eq!(returned.len(), 1);
        let actual = &returned[0];
        assert_eq!(actual.owner, expected.owner);
        assert_eq!(actual.expires_at, expected.expires_at);
        assert_eq!(
            actual.authorization_generation,
            expected.authorization_generation
        );
        assert_eq!(actual.pkce_verifier, expected.pkce_verifier);
        assert_eq!(actual.dcr_client_secret_ct, expected.dcr_client_secret_ct);
    }
    #[tokio::test]
    async fn replacement_revokes_pending_and_initial_reservation_has_no_tools() {
        let p = pool().await;
        let old = begin_authorization(&p, authorization(principal(), "replace"))
            .await
            .unwrap()
            .unwrap();
        assert!(connected_keys(&p, &principal()).await.unwrap().is_empty());
        let replacement = upsert_connection(&p, credential(principal(), 7))
            .await
            .unwrap()
            .unwrap();
        assert!(take_pending(&p, "replace").await.unwrap().is_none());
        assert!(!cancel_authorization(&p, &old).await.unwrap());
        assert!(complete(&p, &old, 9).await.is_none());
        assert_eq!(
            get_connection(&p, &principal(), "gmail")
                .await
                .unwrap()
                .unwrap()
                .generation,
            replacement.generation
        );
    }
    #[tokio::test]
    async fn expiration_sweep_does_not_cancel_a_newer_deadline() {
        let p = pool().await;
        let old = begin_authorization_at(
            &p,
            authorization(user(), "old"),
            timestamp("2030-01-01T00:00:00Z"),
        )
        .await
        .unwrap()
        .unwrap();
        let new = begin_authorization_at(
            &p,
            authorization(user(), "new"),
            timestamp("2030-01-01T00:01:00Z"),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(
            sweep_expired_pending_at(&p, old.expires_at).await.unwrap(),
            0
        );
        let actual = get_connection(&p, &user(), "gmail").await.unwrap().unwrap();
        assert_eq!(
            actual.authorization_generation,
            Some(new.authorization_generation)
        );
        assert_eq!(actual.authorization_expires_at, Some(new.expires_at));
    }

    #[test]
    fn serialized_authority_rejects_unknown_nested_fields() {
        assert!(
            serde_json::from_value::<ConnectionOwner>(
                serde_json::json!({"kind":"user","id":"same","unexpected":true})
            )
            .is_err()
        );
        let identity = ConnectionIdentity {
            owner: user(),
            connector_key: "gmail".into(),
            connection_id: "row".into(),
            generation: "generation".into(),
        };
        let valid = serde_json::to_value(&identity).unwrap();
        assert_eq!(
            serde_json::from_value::<ConnectionIdentity>(valid.clone()).unwrap(),
            identity
        );
        let mut invalid = valid;
        invalid["unexpected"] = serde_json::json!(true);
        assert!(serde_json::from_value::<ConnectionIdentity>(invalid).is_err());
    }

    #[tokio::test]
    async fn authorization_snapshot_survives_consumption_but_not_replacement() {
        let pool = pool().await;
        let pending = begin_authorization(&pool, authorization(principal(), "admission"))
            .await
            .unwrap()
            .unwrap();
        take_pending(&pool, "admission").await.unwrap().unwrap();
        let mut tx = WriteTx::begin(&pool).await.unwrap();
        assert!(
            authorization_is_current_in(&mut tx, &pending)
                .await
                .unwrap()
        );
        tx.commit().await.unwrap();
        begin_authorization(&pool, authorization(principal(), "new-admission"))
            .await
            .unwrap()
            .unwrap();
        let mut tx = WriteTx::begin(&pool).await.unwrap();
        assert!(
            !authorization_is_current_in(&mut tx, &pending)
                .await
                .unwrap()
        );
        tx.commit().await.unwrap();
    }
    #[tokio::test]
    async fn connection_scoped_disconnect_uses_exact_credential_generation() {
        let pool = pool().await;
        let old = upsert_connection(&pool, credential(principal(), 1))
            .await
            .unwrap()
            .unwrap();
        let current = upsert_connection(&pool, credential(principal(), 7))
            .await
            .unwrap()
            .unwrap();
        let pending = begin_authorization(&pool, authorization(principal(), "disconnect-cas"))
            .await
            .unwrap()
            .unwrap();
        let mut tx = WriteTx::begin(&pool).await.unwrap();
        assert!(!delete_connection_in(&mut tx, &old).await.unwrap());
        assert!(
            get_connection_in(&mut tx, &principal(), "gmail")
                .await
                .unwrap()
                .is_some()
        );
        assert!(delete_connection_in(&mut tx, &current).await.unwrap());
        tx.commit().await.unwrap();
        assert!(
            get_connection(&pool, &principal(), "gmail")
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            take_pending(&pool, "disconnect-cas")
                .await
                .unwrap()
                .is_none()
        );
        assert!(complete(&pool, &pending, 9).await.is_none());
    }

    async fn provenance_catalog(p: &Pool) -> String {
        super::super::mcp_catalog::get(p, "gmail")
            .await
            .unwrap()
            .unwrap()
            .dispatch_generation
    }

    fn proven_credential(owner: ConnectionOwner, generation: &str, value: u8) -> NewConnection {
        let mut new = credential(owner, value);
        new.catalog_generation = generation.into();
        new
    }
    fn proven_authorization(
        owner: ConnectionOwner,
        generation: &str,
        state: &str,
    ) -> NewAuthorization {
        let mut new = authorization(owner, state);
        new.catalog_generation = generation.into();
        new
    }

    #[tokio::test]
    async fn catalog_generation_stale_save_and_reservation_leave_credentials_unchanged() {
        let p = pool().await;
        let generation = provenance_catalog(&p).await;
        let identity = upsert_connection(&p, proven_credential(user(), &generation, 1))
            .await
            .unwrap()
            .unwrap();
        let old = get_connection(&p, &user(), "gmail").await.unwrap().unwrap();
        assert_eq!(old.catalog_generation.as_deref(), Some(generation.as_str()));
        assert!(
            upsert_connection(&p, proven_credential(user(), "stale", 9))
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            begin_authorization(&p, proven_authorization(user(), "stale", "stale-state"))
                .await
                .unwrap()
                .is_none()
        );
        let after = get_connection(&p, &user(), "gmail").await.unwrap().unwrap();
        assert_eq!(after.identity(), identity);
        assert_eq!(after.access_token_ct, old.access_token_ct);
        assert!(after.authorization_generation.is_none());
        assert!(take_pending(&p, "stale-state").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn catalog_generation_reauthorization_preserves_old_provenance_until_completion() {
        let p = pool().await;
        let old_generation = provenance_catalog(&p).await;
        upsert_connection(&p, proven_credential(user(), &old_generation, 1))
            .await
            .unwrap()
            .unwrap();
        sqlx::query(
            "UPDATE mcp_catalog_connectors SET dispatch_generation='replacement' WHERE key='gmail'",
        )
        .execute(&p)
        .await
        .unwrap();
        let pending =
            begin_authorization(&p, proven_authorization(user(), "replacement", "new-state"))
                .await
                .unwrap()
                .unwrap();
        assert_eq!(pending.catalog_generation.as_deref(), Some("replacement"));
        let before = get_connection(&p, &user(), "gmail").await.unwrap().unwrap();
        assert_eq!(
            before.catalog_generation.as_deref(),
            Some(old_generation.as_str())
        );
        let mut tx = WriteTx::begin(&p).await.unwrap();
        assert!(
            complete_authorization_in(
                &mut tx,
                &pending,
                proven_credential(user(), "replacement", 7)
            )
            .await
            .unwrap()
            .is_some()
        );
        tx.commit().await.unwrap();
        assert_eq!(
            get_connection(&p, &user(), "gmail")
                .await
                .unwrap()
                .unwrap()
                .catalog_generation
                .as_deref(),
            Some("replacement")
        );
        let cancelled = begin_authorization(
            &p,
            proven_authorization(user(), "replacement", "cancel-state"),
        )
        .await
        .unwrap()
        .unwrap();
        sqlx::query(
            "UPDATE mcp_catalog_connectors SET dispatch_generation='retired' WHERE key='gmail'",
        )
        .execute(&p)
        .await
        .unwrap();
        let mut tx = WriteTx::begin(&p).await.unwrap();
        assert!(
            !authorization_is_current_in(&mut tx, &cancelled)
                .await
                .unwrap()
        );
        assert!(cancel_authorization_in(&mut tx, &cancelled).await.unwrap());
        tx.commit().await.unwrap();
        assert_eq!(
            get_connection(&p, &user(), "gmail")
                .await
                .unwrap()
                .unwrap()
                .catalog_generation
                .as_deref(),
            Some("replacement")
        );
        assert!(connected_keys(&p, &user()).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn catalog_generation_stale_refresh_and_error_cannot_mutate_current_credentials() {
        let p = pool().await;
        let generation = provenance_catalog(&p).await;
        let identity = upsert_connection(&p, proven_credential(user(), &generation, 1))
            .await
            .unwrap()
            .unwrap();
        let refreshed = RefreshTokens {
            access_token_ct: &[7],
            access_token_nonce: &[8],
            refresh_token_ct: None,
            refresh_token_nonce: None,
            token_expires_at: None,
        };
        assert!(
            update_tokens(&p, &identity, &generation, refreshed)
                .await
                .unwrap()
        );
        let before = get_connection(&p, &user(), "gmail").await.unwrap().unwrap();
        assert_eq!(before.refresh_token_ct, Some(vec![3, 0, 254]));
        assert_eq!(before.identity(), identity);
        sqlx::query(
            "UPDATE mcp_catalog_connectors SET dispatch_generation='replacement' WHERE key='gmail'",
        )
        .execute(&p)
        .await
        .unwrap();
        let stale = RefreshTokens {
            access_token_ct: &[9],
            access_token_nonce: &[9],
            refresh_token_ct: None,
            refresh_token_nonce: None,
            token_expires_at: None,
        };
        assert!(
            !update_tokens(&p, &identity, &generation, stale)
                .await
                .unwrap()
        );
        assert!(
            !mark_error(&p, &identity, &generation, "stale provider error", true)
                .await
                .unwrap()
        );
        let after = get_connection(&p, &user(), "gmail").await.unwrap().unwrap();
        assert_eq!(after.status, STATUS_CONNECTED);
        assert_eq!(after.access_token_ct, before.access_token_ct);
        assert_eq!(after.catalog_generation, before.catalog_generation);
    }

    #[tokio::test]
    async fn static_bearer_unreadable_credential_is_marked_reauth_under_current_catalog() {
        let p = pool().await;
        sqlx::query("UPDATE mcp_catalog_connectors SET auth='static_bearer',dispatch_generation='static-current' WHERE key='gmail'")
            .execute(&p).await.unwrap();
        let old = upsert_connection(&p, proven_credential(user(), "static-current", 1))
            .await
            .unwrap()
            .unwrap();
        let current = upsert_connection(&p, proven_credential(user(), "static-current", 9))
            .await
            .unwrap()
            .unwrap();
        assert!(
            !mark_error(
                &p,
                &old,
                "static-current",
                "credential could not be decrypted",
                true
            )
            .await
            .unwrap()
        );
        assert!(
            mark_error(
                &p,
                &current,
                "static-current",
                "credential could not be decrypted",
                true
            )
            .await
            .unwrap()
        );
        let failed = get_connection(&p, &user(), "gmail").await.unwrap().unwrap();
        assert_eq!(failed.status, STATUS_REAUTH);
        assert_eq!(failed.identity(), current);
        assert_eq!(failed.access_token_ct, Some(vec![9, 0, 255]));
        assert!(connected_keys(&p, &user()).await.unwrap().is_empty());
        let replacement = upsert_connection(&p, proven_credential(user(), "static-current", 7))
            .await
            .unwrap()
            .unwrap();
        sqlx::query("UPDATE mcp_catalog_connectors SET dispatch_generation='static-retired' WHERE key='gmail'")
            .execute(&p).await.unwrap();
        assert!(
            !mark_error(
                &p,
                &replacement,
                "static-current",
                "stale decryption failure",
                true
            )
            .await
            .unwrap()
        );
        assert_eq!(
            get_connection(&p, &user(), "gmail")
                .await
                .unwrap()
                .unwrap()
                .status,
            STATUS_CONNECTED
        );
    }
}
