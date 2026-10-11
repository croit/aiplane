// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use sqlx::sqlite::SqliteRow;

use super::{DbError, Pool, WriteTx};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct User {
    pub id: String,
    pub email: String,
    pub name: Option<String>,
    pub roles: Vec<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    /// IANA timezone (e.g. `Europe/Berlin`). Captured from the browser
    /// on first authed page load — see `set_timezone`. None for users
    /// who only hit the gateway through external API calls,
    /// or who haven't navigated the UI since the column was added.
    /// Tools that care about wall-clock time fall back to UTC when
    /// this is null.
    pub timezone: Option<String>,
    /// TTS voice the user picked for spoken replies (a bare upstream voice id
    /// like `onyx`). `None` — the common case — means the speech pool's
    /// language → voice map decides. Never trusted on read: the speech path
    /// re-checks it against the voices the pools currently advertise, so an
    /// operator who drops a voice from the config retires it everywhere.
    pub speech_voice: Option<String>,
}

fn map_row(row: &SqliteRow) -> Result<User, DbError> {
    let id: String = row.try_get("id")?;
    let email: String = row.try_get("email")?;
    let name: Option<String> = row.try_get("name")?;
    let roles_json: String = row.try_get("roles_json")?;
    let created_at: String = row.try_get("created_at")?;
    let updated_at: String = row.try_get("updated_at")?;
    let timezone: Option<String> = row.try_get("timezone")?;
    let speech_voice: Option<String> = row.try_get("speech_voice")?;

    let roles: Vec<String> = serde_json::from_str(&roles_json).map_err(|e| DbError::Decode {
        column: "roles_json",
        source: e.into(),
    })?;
    let created_at: Timestamp = created_at
        .parse()
        .map_err(|e: jiff::Error| DbError::Decode {
            column: "created_at",
            source: e.into(),
        })?;
    let updated_at: Timestamp = updated_at
        .parse()
        .map_err(|e: jiff::Error| DbError::Decode {
            column: "updated_at",
            source: e.into(),
        })?;

    Ok(User {
        id,
        email,
        name,
        roles,
        created_at,
        updated_at,
        timezone,
        speech_voice,
    })
}

/// Inserts a new user or, if `id` already exists, updates the mutable fields
/// (email, name, roles) and bumps `updated_at`. `created_at` is preserved.
pub async fn upsert(pool: &Pool, user: &User) -> Result<(), DbError> {
    let roles_json = serde_json::to_string(&user.roles).map_err(|e| DbError::Decode {
        column: "roles_json",
        source: e.into(),
    })?;
    sqlx::query(
        r#"INSERT INTO users (id, email, name, roles_json, created_at, updated_at)
           VALUES (?, ?, ?, ?, ?, ?)
           ON CONFLICT(id) DO UPDATE SET
               email      = excluded.email,
               name       = excluded.name,
               roles_json = excluded.roles_json,
               updated_at = excluded.updated_at"#,
    )
    .bind(&user.id)
    .bind(&user.email)
    .bind(&user.name)
    .bind(&roles_json)
    .bind(user.created_at.to_string())
    .bind(user.updated_at.to_string())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn find_by_id(pool: &Pool, id: &str) -> Result<Option<User>, DbError> {
    let row = sqlx::query("SELECT * FROM users WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    row.as_ref().map(map_row).transpose()
}

pub async fn find_by_id_in(tx: &mut super::WriteTx, id: &str) -> Result<Option<User>, DbError> {
    let row = sqlx::query("SELECT * FROM users WHERE id = ?")
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?;
    row.as_ref().map(map_row).transpose()
}

/// Every registered user, newest first. Powers the admin `/admin/users`
/// roster. Users are created lazily on first OIDC login (see
/// `oidc_handlers::callback`), so this is "everyone who has ever signed
/// in", not a pre-provisioned directory.
pub async fn list_all(pool: &Pool) -> Result<Vec<User>, DbError> {
    let rows = sqlx::query("SELECT * FROM users ORDER BY created_at DESC, id")
        .fetch_all(pool)
        .await?;
    rows.iter().map(map_row).collect()
}

/// At most `limit` users whose name or email contains `query`, ignoring
/// case; one whose email is exactly `query` first, then by name. Picks a
/// person to share with without handing anyone the roster: the caller decides
/// how short a query it serves and what of each match it shows.
pub async fn search(pool: &Pool, query: &str, limit: i64) -> Result<Vec<User>, DbError> {
    search_with(pool, query, limit, 0).await
}

pub struct UserSearchPage {
    pub users: Vec<User>,
    pub next_offset: Option<u64>,
}

pub async fn search_page_in(
    tx: &mut WriteTx,
    query: &str,
    limit: u32,
    offset: u64,
) -> Result<UserSearchPage, DbError> {
    let limit = limit.min(50);
    if limit == 0 {
        return Ok(UserSearchPage {
            users: Vec::new(),
            next_offset: None,
        });
    }
    let sql_offset = i64::try_from(offset).map_err(|source| DbError::Decode {
        column: "search_offset",
        source: source.into(),
    })?;
    let mut users = search_with(&mut **tx, query, i64::from(limit) + 1, sql_offset).await?;
    let next_offset = (users.len() > limit as usize).then_some(offset + u64::from(limit));
    users.truncate(limit as usize);
    Ok(UserSearchPage { users, next_offset })
}

async fn search_with<'e, E>(
    executor: E,
    query: &str,
    limit: i64,
    offset: i64,
) -> Result<Vec<User>, DbError>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let pattern = format!(
        "%{}%",
        query
            .to_lowercase()
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    );
    let rows = sqlx::query(
        r#"SELECT * FROM users
           WHERE lower(coalesce(name, '')) LIKE ?1 ESCAPE '\' OR lower(email) LIKE ?1 ESCAPE '\'
           ORDER BY lower(email) = lower(?2) DESC, lower(coalesce(name, email)), id
           LIMIT ?3 OFFSET ?4"#,
    )
    .bind(&pattern)
    .bind(query)
    .bind(limit)
    .bind(offset)
    .fetch_all(executor)
    .await?;
    rows.iter().map(map_row).collect()
}

/// Updates just the `timezone` column for an existing user. Bumps
/// `updated_at`. Called from the `POST /api/v0/me/timezone` handler
/// after the browser's `Intl.DateTimeFormat().resolvedOptions().
/// timeZone` posts up. Silently no-ops if the user doesn't exist
/// (shouldn't happen — caller is always authed).
pub async fn set_timezone(pool: &Pool, user_id: &str, timezone: &str) -> Result<(), DbError> {
    let now = Timestamp::now().to_string();
    sqlx::query("UPDATE users SET timezone = ?, updated_at = ? WHERE id = ?")
        .bind(timezone)
        .bind(now)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Store (or clear, with `None`) the caller's preferred TTS voice. Bumps
/// `updated_at`. Called from `POST /api/v0/me/speech_voice`, which validates
/// the id against the voices the speech pools advertise before it gets here —
/// this function stores whatever it is given.
pub async fn set_speech_voice(
    pool: &Pool,
    user_id: &str,
    voice: Option<&str>,
) -> Result<(), DbError> {
    let now = Timestamp::now().to_string();
    sqlx::query("UPDATE users SET speech_voice = ?, updated_at = ? WHERE id = ?")
        .bind(voice)
        .bind(now)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// A browser-shared geolocation read back off the `users` row. Distinct
/// from the IP-derived `geoip::GeoLocation`: this is the precise position
/// the user explicitly granted via `navigator.geolocation`, stamped with
/// when we recorded it so callers can judge staleness.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StoredLocation {
    pub lat: f64,
    pub lon: f64,
    /// Reported accuracy radius in metres, when the browser supplied one.
    pub accuracy: Option<f64>,
    pub updated_at: Timestamp,
}

impl StoredLocation {
    /// Whether the fix is at most `max_age_secs` old. A precise GPS fix
    /// goes stale as the user moves, so the `get_user_location` tool only
    /// trusts a recent one and otherwise falls back to coarse GeoIP.
    pub fn is_fresh(&self, max_age_secs: i64) -> bool {
        Timestamp::now().as_second() - self.updated_at.as_second() <= max_age_secs
    }
}

/// Store the caller's browser-reported position on their user row,
/// bumping `loc_updated_at` + `updated_at`. Written by
/// `POST /api/v0/me/location` (the `/tools` button + the chat
/// feedback-loop prompt). Last shared position wins.
pub async fn set_location(
    pool: &Pool,
    user_id: &str,
    lat: f64,
    lon: f64,
    accuracy: Option<f64>,
) -> Result<(), DbError> {
    let now = Timestamp::now().to_string();
    sqlx::query(
        "UPDATE users SET loc_lat = ?, loc_lon = ?, loc_accuracy = ?, \
         loc_updated_at = ?, updated_at = ? WHERE id = ?",
    )
    .bind(lat)
    .bind(lon)
    .bind(accuracy)
    .bind(&now)
    .bind(&now)
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Read the caller's stored browser position, if any. Returns `None`
/// when the user never shared one (all-null columns) or doesn't exist.
pub async fn find_location(pool: &Pool, user_id: &str) -> Result<Option<StoredLocation>, DbError> {
    let row = sqlx::query(
        "SELECT loc_lat, loc_lon, loc_accuracy, loc_updated_at FROM users WHERE id = ?",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    let Some(row) = row else { return Ok(None) };
    let lat: Option<f64> = row.try_get("loc_lat")?;
    let lon: Option<f64> = row.try_get("loc_lon")?;
    let accuracy: Option<f64> = row.try_get("loc_accuracy")?;
    let updated_at: Option<String> = row.try_get("loc_updated_at")?;
    match (lat, lon, updated_at) {
        (Some(lat), Some(lon), Some(updated_at)) => {
            let updated_at: Timestamp =
                updated_at
                    .parse()
                    .map_err(|e: jiff::Error| DbError::Decode {
                        column: "loc_updated_at",
                        source: e.into(),
                    })?;
            Ok(Some(StoredLocation {
                lat,
                lon,
                accuracy,
                updated_at,
            }))
        }
        _ => Ok(None),
    }
}

/// Forget the caller's stored position (the "stop sharing" affordance on
/// `/tools`). No-op if none was stored.
pub async fn clear_location(pool: &Pool, user_id: &str) -> Result<(), DbError> {
    let now = Timestamp::now().to_string();
    sqlx::query(
        "UPDATE users SET loc_lat = NULL, loc_lon = NULL, loc_accuracy = NULL, \
         loc_updated_at = NULL, updated_at = ? WHERE id = ?",
    )
    .bind(&now)
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(id: &str) -> User {
        let now = Timestamp::now();
        User {
            id: id.into(),
            email: format!("{id}@example.com"),
            name: Some("Test User".into()),
            roles: vec!["engineering".into()],
            created_at: now,
            updated_at: now,
            timezone: None,
            speech_voice: None,
        }
    }

    async fn pool() -> Pool {
        super::super::open(std::path::Path::new(":memory:"))
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn search_matches_name_or_email_capped_with_an_exact_email_first() {
        let pool = pool().await;
        for (id, name, email) in [
            ("u1", Some("Ada Lovelace"), "ada@example.com"),
            ("u2", Some("Adam Smith"), "adam@corp.test"),
            ("u3", None, "zed_ada@example.com"),
            ("u4", Some("Bob"), "bob@example.com"),
        ] {
            let mut u = fixture(id);
            u.name = name.map(str::to_string);
            u.email = email.into();
            upsert(&pool, &u).await.unwrap();
        }
        let ids = |users: Vec<User>| users.into_iter().map(|u| u.id).collect::<Vec<_>>();
        assert_eq!(
            ids(search(&pool, "ADA", 10).await.unwrap()),
            ["u1", "u2", "u3"]
        );
        assert_eq!(ids(search(&pool, "ada", 2).await.unwrap()).len(), 2);
        assert_eq!(
            ids(search(&pool, "adam@corp.test", 10).await.unwrap()),
            ["u2"]
        );
        assert_eq!(
            ids(search(&pool, "zed_ada@example.com", 10).await.unwrap())[0],
            "u3",
            "an exact address comes first"
        );
        assert!(
            search(&pool, "%", 10).await.unwrap().is_empty(),
            "wildcards are literal"
        );
        assert_eq!(ids(search(&pool, "d_a", 10).await.unwrap()), ["u3"]);
    }

    #[tokio::test]
    async fn transactional_search_pages_are_capped_ordered_and_finite() {
        let pool = pool().await;
        for index in 0..53 {
            let id = format!("candidate-{index:03}");
            let mut user = fixture(&id);
            user.name = Some("Candidate".into());
            upsert(&pool, &user).await.unwrap();
        }
        let expected = search(&pool, "Candidate", 100).await.unwrap();
        let mut tx = super::super::WriteTx::begin(&pool).await.unwrap();
        let first = search_page_in(&mut tx, "Candidate", 500, 0).await.unwrap();
        assert_eq!(first.users.len(), 50);
        assert_eq!(first.next_offset, Some(50));
        assert_eq!(first.users, expected[..50]);
        let second = search_page_in(&mut tx, "Candidate", 50, first.next_offset.unwrap())
            .await
            .unwrap();
        assert_eq!(second.users, expected[50..]);
        assert_eq!(second.next_offset, None);
        assert!(
            search_page_in(&mut tx, "Candidate", 0, 0)
                .await
                .unwrap()
                .users
                .is_empty()
        );
        assert!(
            search_page_in(&mut tx, "Candidate", 50, 100)
                .await
                .unwrap()
                .users
                .is_empty()
        );
        assert!(
            search_page_in(&mut tx, "%", 50, 0)
                .await
                .unwrap()
                .users
                .is_empty()
        );
        tx.commit().await.unwrap();
    }

    #[tokio::test]
    async fn transactional_search_pages_use_current_rows_and_exact_address_ranking() {
        let pool = pool().await;
        for id in ["first", "last"] {
            upsert(&pool, &fixture(id)).await.unwrap();
        }
        let mut tx = super::super::WriteTx::begin(&pool).await.unwrap();
        sqlx::query(r#"UPDATE users SET name='last@example.com',roles_json='["current-role"]' WHERE id='first'"#)
            .execute(&mut *tx).await.unwrap();
        let page = search_page_in(&mut tx, "last@example.com", 1, 0)
            .await
            .unwrap();
        assert_eq!(page.users[0].id, "last");
        assert_eq!(page.next_offset, Some(1));
        let next = search_page_in(&mut tx, "last@example.com", 1, 1)
            .await
            .unwrap();
        assert_eq!(next.users[0].id, "first");
        assert_eq!(next.users[0].roles, ["current-role"]);
        assert_eq!(next.next_offset, None);
        drop(tx);
        assert_eq!(
            find_by_id(&pool, "first").await.unwrap().unwrap().roles,
            ["engineering"]
        );
    }

    #[tokio::test]
    async fn upsert_inserts_then_updates() {
        let pool = pool().await;
        let mut u = fixture("subject-abc");
        upsert(&pool, &u).await.unwrap();

        u.email = "new@example.com".into();
        u.roles = vec!["finance".into(), "admin".into()];
        upsert(&pool, &u).await.unwrap();

        let got = find_by_id(&pool, "subject-abc").await.unwrap().unwrap();
        assert_eq!(got.email, "new@example.com");
        assert_eq!(got.roles, vec!["finance", "admin"]);
    }

    #[tokio::test]
    async fn find_by_id_returns_none_for_unknown() {
        let pool = pool().await;
        assert!(find_by_id(&pool, "nope").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn list_all_returns_every_user() {
        let pool = pool().await;
        assert!(list_all(&pool).await.unwrap().is_empty());
        upsert(&pool, &fixture("u1")).await.unwrap();
        upsert(&pool, &fixture("u2")).await.unwrap();
        let all = list_all(&pool).await.unwrap();
        assert_eq!(all.len(), 2);
        let ids: Vec<&str> = all.iter().map(|u| u.id.as_str()).collect();
        assert!(ids.contains(&"u1") && ids.contains(&"u2"));
    }

    #[tokio::test]
    async fn location_round_trips_and_clears() {
        let pool = pool().await;
        let u = fixture("subject-loc");
        upsert(&pool, &u).await.unwrap();
        assert!(find_location(&pool, &u.id).await.unwrap().is_none());

        set_location(&pool, &u.id, 52.52, 13.405, Some(25.0))
            .await
            .unwrap();
        let got = find_location(&pool, &u.id).await.unwrap().unwrap();
        assert_eq!(got.lat, 52.52);
        assert_eq!(got.lon, 13.405);
        assert_eq!(got.accuracy, Some(25.0));
        // Just-written fix is fresh under any sane threshold.
        assert!(got.is_fresh(60));
        // A zero-second window makes even a fresh fix count as stale.
        assert!(!got.is_fresh(-1));

        clear_location(&pool, &u.id).await.unwrap();
        assert!(find_location(&pool, &u.id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn set_location_accepts_null_accuracy() {
        let pool = pool().await;
        let u = fixture("subject-loc2");
        upsert(&pool, &u).await.unwrap();
        set_location(&pool, &u.id, 1.0, 2.0, None).await.unwrap();
        let got = find_location(&pool, &u.id).await.unwrap().unwrap();
        assert_eq!(got.accuracy, None);
    }

    #[tokio::test]
    async fn set_timezone_persists_and_bumps_updated_at() {
        let pool = pool().await;
        let u = fixture("subject-tz");
        upsert(&pool, &u).await.unwrap();
        let before = find_by_id(&pool, &u.id).await.unwrap().unwrap();
        assert!(before.timezone.is_none());

        set_timezone(&pool, &u.id, "Europe/Berlin").await.unwrap();

        let after = find_by_id(&pool, &u.id).await.unwrap().unwrap();
        assert_eq!(after.timezone.as_deref(), Some("Europe/Berlin"));
        assert!(after.updated_at >= before.updated_at);
    }

    #[tokio::test]
    async fn connection_scoped_user_read_observes_actual_roles() {
        let pool = pool().await;
        let mut user = fixture("current");
        user.roles = vec!["engineering".into()];
        upsert(&pool, &user).await.unwrap();
        let mut tx = crate::server::db::WriteTx::begin(&pool).await.unwrap();
        let actual = find_by_id_in(&mut tx, "current").await.unwrap().unwrap();
        assert_eq!(actual.roles, user.roles);
        assert!(find_by_id_in(&mut tx, "missing").await.unwrap().is_none());
        tx.commit().await.unwrap();
    }
}
