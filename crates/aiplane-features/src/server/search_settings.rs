// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Operator-configurable web-search backend for the `search_web` tool.
//!
//! `search_web` used to read `SEARCH_PROVIDER`, `SEARXNG_URL` and
//! `BRAVE_SEARCH_API_KEY` straight from the process environment. That made it
//! the last piece of gateway configuration living outside the database, and —
//! worse — it kept a third-party API key in plaintext in the environment,
//! where it shows up in `/proc/<pid>/environ`, in unit files, and in
//! `podman inspect`. Every other at-rest secret (backend API keys, connector
//! client secrets, the VAPID private key) is sealed in the DB under
//! `AIPLANE_ENCRYPTION_KEY`.
//!
//! So: the provider and the SearXNG URL are plain [`app_settings`] rows (not
//! secrets), and the Brave key is sealed with [`Crypto`] in the same
//! `nonce.ciphertext` shape `server::push` uses for VAPID.
//!
//! **Migration.** [`import_env_once`] runs at boot and copies whatever the
//! environment still carries into empty settings, logging what it took over.
//! It only ever fills gaps — once a value is in the DB the environment is
//! ignored, and the operator is told so. There is deliberately no permanent
//! env fallback: two sources of truth for one setting is how deployments end
//! up with a gateway nobody can explain.
//!
//! [`app_settings`]: aiplane_core::server::db::app_settings

use aiplane_core::server::crypto::Crypto;
use aiplane_core::server::db::{DbError, Pool, app_settings};

/// `app_settings` key for the selected provider.
pub const PROVIDER_KEY: &str = "search.provider";
/// `app_settings` key for the SearXNG base URL.
pub const SEARXNG_URL_KEY: &str = "search.searxng_url";
/// `app_settings` key for the sealed Brave API key (`nonce.ciphertext`,
/// base64url, matching `server::push`'s stored form).
pub const BRAVE_KEY_KEY: &str = "search.brave_api_key";
pub const TAVILY_KEY_KEY: &str = "search.tavily_api_key";
pub const TAVILY_ENABLED_KEY: &str = "search.tavily_enabled";

/// Which search backend answers `search_web`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchProvider {
    /// Self-hosted federated search. No key, no per-query cost.
    #[default]
    Searxng,
    /// Brave Search API. Needs a subscription token.
    Brave,
    Tavily,
}

impl SearchProvider {
    /// Parse the stored / posted wire name. Unknown values return `None` so
    /// callers can reject rather than silently pick a backend.
    pub fn from_wire(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "searxng" => Some(Self::Searxng),
            "brave" => Some(Self::Brave),
            "tavily" => Some(Self::Tavily),
            _ => None,
        }
    }

    /// Stable wire name (persisted value + admin form field).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Searxng => "searxng",
            Self::Brave => "brave",
            Self::Tavily => "tavily",
        }
    }
}

/// The resolved search configuration, as `search_web` needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchSettings {
    pub provider: SearchProvider,
    /// SearXNG base URL, trailing slash already trimmed. `None` when unset.
    pub searxng_url: Option<String>,
    /// Decrypted Brave API key. `None` when unset — or when the stored
    /// ciphertext could not be opened (the at-rest key changed), which is
    /// logged and treated as "not configured" so the tool reports a fixable
    /// state instead of a decryption error.
    pub brave_api_key: Option<String>,
    pub tavily_api_key: Option<String>,
    pub tavily_enabled: bool,
}

/// What the admin page shows. Never carries the Brave key itself — a secret
/// that has been written is write-only from then on, same as backend keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchSettingsView {
    pub provider: SearchProvider,
    pub searxng_url: Option<String>,
    pub brave_key_set: bool,
    pub tavily_key_set: bool,
    pub tavily_enabled: bool,
    pub tavily_active: bool,
}

/// Load the effective settings.
pub async fn load(pool: &Pool, crypto: &Crypto) -> Result<SearchSettings, DbError> {
    let provider = app_settings::get(pool, PROVIDER_KEY)
        .await?
        .and_then(|v| SearchProvider::from_wire(&v))
        .unwrap_or_default();
    Ok(SearchSettings {
        provider,
        searxng_url: normalized_url(app_settings::get(pool, SEARXNG_URL_KEY).await?),
        brave_api_key: open_key(pool, crypto, BRAVE_KEY_KEY, "Brave").await?,
        tavily_api_key: open_key(pool, crypto, TAVILY_KEY_KEY, "Tavily").await?,
        tavily_enabled: tavily_enabled(pool).await?,
    })
}

/// Load the admin-page view (no secret material).
pub async fn view(pool: &Pool, crypto: &Crypto) -> Result<SearchSettingsView, DbError> {
    let provider = app_settings::get(pool, PROVIDER_KEY)
        .await?
        .and_then(|v| SearchProvider::from_wire(&v))
        .unwrap_or_default();
    let tavily_key_set = app_settings::get(pool, TAVILY_KEY_KEY)
        .await?
        .is_some_and(|v| !v.trim().is_empty());
    let tavily_enabled = tavily_enabled(pool).await?;
    Ok(SearchSettingsView {
        provider,
        searxng_url: normalized_url(app_settings::get(pool, SEARXNG_URL_KEY).await?),
        brave_key_set: app_settings::get(pool, BRAVE_KEY_KEY)
            .await?
            .is_some_and(|v| !v.trim().is_empty()),
        tavily_key_set,
        tavily_enabled,
        tavily_active: tavily_enabled
            && open_key(pool, crypto, TAVILY_KEY_KEY, "Tavily")
                .await?
                .is_some(),
    })
}

pub async fn set_provider(pool: &Pool, provider: SearchProvider) -> Result<(), DbError> {
    app_settings::set(pool, PROVIDER_KEY, provider.as_str()).await
}

/// Set or clear the SearXNG base URL. An empty string clears it.
pub async fn set_searxng_url(pool: &Pool, url: &str) -> Result<(), DbError> {
    let url = url.trim();
    if url.is_empty() {
        return app_settings::delete(pool, SEARXNG_URL_KEY).await;
    }
    app_settings::set(pool, SEARXNG_URL_KEY, url).await
}

/// Seal and store the Brave API key. An empty string clears it.
pub async fn set_brave_key(pool: &Pool, crypto: &Crypto, key: &str) -> Result<(), DbError> {
    set_key(pool, crypto, BRAVE_KEY_KEY, key, "Brave").await
}

pub async fn set_tavily_key(pool: &Pool, crypto: &Crypto, key: &str) -> Result<(), DbError> {
    set_key(pool, crypto, TAVILY_KEY_KEY, key, "Tavily").await
}

pub async fn set_tavily_enabled(pool: &Pool, enabled: bool) -> Result<(), DbError> {
    app_settings::set(
        pool,
        TAVILY_ENABLED_KEY,
        if enabled { "true" } else { "false" },
    )
    .await
}

async fn tavily_enabled(pool: &Pool) -> Result<bool, DbError> {
    Ok(app_settings::get(pool, TAVILY_ENABLED_KEY)
        .await?
        .is_some_and(|value| value == "true"))
}

async fn set_key(
    pool: &Pool,
    crypto: &Crypto,
    setting: &str,
    key: &str,
    provider: &str,
) -> Result<(), DbError> {
    let key = key.trim();
    if key.is_empty() {
        return app_settings::delete(pool, setting).await;
    }
    let Ok(stored) = crypto.seal_to_string(key) else {
        tracing::error!(
            provider,
            "could not seal the search API key — at-rest encryption is unavailable"
        );
        return Ok(());
    };
    app_settings::set(pool, setting, &stored).await
}

/// The legacy environment variables, read once.
///
/// Read at the edge and passed in rather than pulled from `std::env` deep
/// inside [`import_once`]: env vars are process-global, so a test that had to
/// set them would race every other test in the binary. (The tests in
/// `tools::search_web` carry a comment wishing for exactly this shape.)
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct EnvSearchConfig {
    pub provider: Option<String>,
    pub searxng_url: Option<String>,
    pub brave_api_key: Option<String>,
}

impl EnvSearchConfig {
    pub fn from_env() -> Self {
        Self {
            provider: std::env::var("SEARCH_PROVIDER").ok(),
            searxng_url: std::env::var("SEARXNG_URL").ok(),
            brave_api_key: std::env::var("BRAVE_SEARCH_API_KEY").ok(),
        }
    }
}

/// Copy still-present environment variables into empty settings, once, at
/// boot. Returns the names it took over, for logging by the caller.
pub async fn import_env_once(pool: &Pool, crypto: &Crypto) -> Result<Vec<&'static str>, DbError> {
    import_once(pool, crypto, EnvSearchConfig::from_env()).await
}

/// Gap-filling import: a setting already in the DB wins, and its environment
/// counterpart is reported as ignored so a confused operator gets a hint
/// instead of silence.
async fn import_once(
    pool: &Pool,
    crypto: &Crypto,
    env: EnvSearchConfig,
) -> Result<Vec<&'static str>, DbError> {
    let mut imported = Vec::new();

    if let Some(raw) = env.provider.as_deref() {
        match (
            SearchProvider::from_wire(raw),
            app_settings::get(pool, PROVIDER_KEY).await?,
        ) {
            (Some(p), None) => {
                set_provider(pool, p).await?;
                imported.push("SEARCH_PROVIDER");
            }
            (Some(_), Some(_)) => warn_ignored("SEARCH_PROVIDER"),
            (None, _) => tracing::warn!(
                value = %raw,
                "SEARCH_PROVIDER is not `searxng`, `brave`, or `tavily` — ignoring it"
            ),
        }
    }

    if let Some(url) = env.searxng_url.as_deref() {
        if app_settings::get(pool, SEARXNG_URL_KEY).await?.is_none() {
            set_searxng_url(pool, url).await?;
            imported.push("SEARXNG_URL");
        } else {
            warn_ignored("SEARXNG_URL");
        }
    }

    if let Some(key) = env.brave_api_key.as_deref() {
        if app_settings::get(pool, BRAVE_KEY_KEY).await?.is_none() {
            set_brave_key(pool, crypto, key).await?;
            imported.push("BRAVE_SEARCH_API_KEY");
        } else {
            warn_ignored("BRAVE_SEARCH_API_KEY");
        }
    }

    Ok(imported)
}

fn warn_ignored(var: &str) {
    tracing::warn!(
        env_var = %var,
        "web-search settings now live in the database (configure them at \
         /admin/settings?tab=web-search); the environment variable is ignored"
    );
}

/// Trim a stored URL and drop it if it's blank.
fn normalized_url(stored: Option<String>) -> Option<String> {
    stored
        .map(|u| u.trim().trim_end_matches('/').to_string())
        .filter(|u| !u.is_empty())
}

async fn open_key(
    pool: &Pool,
    crypto: &Crypto,
    setting: &str,
    provider: &str,
) -> Result<Option<String>, DbError> {
    let Some(stored) = app_settings::get(pool, setting).await? else {
        return Ok(None);
    };
    let opened = crypto.open_from_string(&stored);
    if opened.is_none() {
        tracing::warn!(
            provider,
            "stored search API key could not be decrypted (at-rest key changed?); re-enter it in the admin UI"
        );
    }
    Ok(opened)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aiplane_core::server::db::open;
    use std::path::Path;

    async fn fresh() -> (Pool, Crypto) {
        (
            open(Path::new(":memory:")).await.unwrap(),
            Crypto::ephemeral(),
        )
    }

    #[test]
    fn provider_wire_names_round_trip() {
        for p in [
            SearchProvider::Searxng,
            SearchProvider::Brave,
            SearchProvider::Tavily,
        ] {
            assert_eq!(SearchProvider::from_wire(p.as_str()), Some(p));
        }
        // Tolerant of case and padding, strict about the value.
        assert_eq!(
            SearchProvider::from_wire(" BRAVE "),
            Some(SearchProvider::Brave)
        );
        assert_eq!(SearchProvider::from_wire("google"), None);
    }

    #[tokio::test]
    async fn defaults_to_searxng_with_nothing_configured() {
        let (pool, crypto) = fresh().await;
        let s = load(&pool, &crypto).await.unwrap();
        assert_eq!(s.provider, SearchProvider::Searxng);
        assert!(s.searxng_url.is_none());
        assert!(s.brave_api_key.is_none());
    }

    #[tokio::test]
    async fn provider_and_url_round_trip() {
        let (pool, crypto) = fresh().await;
        set_provider(&pool, SearchProvider::Brave).await.unwrap();
        set_searxng_url(&pool, "https://searx.example.com/")
            .await
            .unwrap();
        let s = load(&pool, &crypto).await.unwrap();
        assert_eq!(s.provider, SearchProvider::Brave);
        // Trailing slash normalised away so callers can append `/search`.
        assert_eq!(s.searxng_url.as_deref(), Some("https://searx.example.com"));
    }

    #[tokio::test]
    async fn empty_url_clears_the_setting() {
        let (pool, crypto) = fresh().await;
        set_searxng_url(&pool, "https://x.example").await.unwrap();
        set_searxng_url(&pool, "   ").await.unwrap();
        assert!(load(&pool, &crypto).await.unwrap().searxng_url.is_none());
    }

    #[tokio::test]
    async fn brave_key_is_sealed_at_rest_and_opens_again() {
        let (pool, crypto) = fresh().await;
        set_brave_key(&pool, &crypto, "super-secret-token")
            .await
            .unwrap();

        // The raw row must not contain the plaintext.
        let raw = app_settings::get(&pool, BRAVE_KEY_KEY)
            .await
            .unwrap()
            .expect("stored");
        assert!(
            !raw.contains("super-secret-token"),
            "stored plaintext: {raw}"
        );
        assert!(raw.contains('.'), "expected nonce.ciphertext shape: {raw}");

        assert_eq!(
            load(&pool, &crypto).await.unwrap().brave_api_key.as_deref(),
            Some("super-secret-token")
        );
    }

    #[tokio::test]
    async fn brave_key_under_a_different_at_rest_key_reads_as_unset() {
        let (pool, crypto) = fresh().await;
        set_brave_key(&pool, &crypto, "token").await.unwrap();
        // A rotated/lost AIPLANE_ENCRYPTION_KEY must degrade to "not
        // configured", never to an error that breaks every search.
        let other = Crypto::ephemeral();
        assert!(load(&pool, &other).await.unwrap().brave_api_key.is_none());
    }

    #[tokio::test]
    async fn empty_brave_key_clears_the_setting() {
        let (pool, crypto) = fresh().await;
        set_brave_key(&pool, &crypto, "token").await.unwrap();
        set_brave_key(&pool, &crypto, "").await.unwrap();
        assert!(
            app_settings::get(&pool, BRAVE_KEY_KEY)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn tavily_key_is_sealed_and_can_be_cleared() {
        let (pool, crypto) = fresh().await;
        set_tavily_key(&pool, &crypto, "tvly-secret").await.unwrap();
        let raw = app_settings::get(&pool, TAVILY_KEY_KEY)
            .await
            .unwrap()
            .unwrap();
        assert!(!raw.contains("tvly-secret"));
        assert_eq!(
            load(&pool, &crypto)
                .await
                .unwrap()
                .tavily_api_key
                .as_deref(),
            Some("tvly-secret")
        );
        assert!(view(&pool, &crypto).await.unwrap().tavily_key_set);

        set_tavily_key(&pool, &crypto, "").await.unwrap();
        assert!(load(&pool, &crypto).await.unwrap().tavily_api_key.is_none());
        assert!(!view(&pool, &crypto).await.unwrap().tavily_key_set);
    }

    #[tokio::test]
    async fn tavily_is_inactive_until_enabled_with_a_key() {
        let (pool, crypto) = fresh().await;
        assert!(!load(&pool, &crypto).await.unwrap().tavily_enabled);
        assert!(!view(&pool, &crypto).await.unwrap().tavily_active);

        set_tavily_enabled(&pool, true).await.unwrap();
        assert!(load(&pool, &crypto).await.unwrap().tavily_enabled);
        assert!(!view(&pool, &crypto).await.unwrap().tavily_active);

        set_tavily_key(&pool, &crypto, "tvly-test").await.unwrap();
        assert!(view(&pool, &crypto).await.unwrap().tavily_active);
        assert!(
            !view(&pool, &Crypto::ephemeral())
                .await
                .unwrap()
                .tavily_active
        );

        set_tavily_enabled(&pool, false).await.unwrap();
        assert!(!view(&pool, &crypto).await.unwrap().tavily_active);
    }

    #[tokio::test]
    async fn view_never_exposes_the_key_but_reports_whether_it_is_set() {
        let (pool, crypto) = fresh().await;
        assert!(!view(&pool, &crypto).await.unwrap().brave_key_set);
        set_brave_key(&pool, &crypto, "token").await.unwrap();
        let v = view(&pool, &crypto).await.unwrap();
        assert!(v.brave_key_set);
        // Nothing on the view type can carry the secret — checked structurally
        // by the absence of a field, and here by the debug output.
        assert!(!format!("{v:?}").contains("token"));
    }

    fn env(provider: Option<&str>, url: Option<&str>, key: Option<&str>) -> EnvSearchConfig {
        EnvSearchConfig {
            provider: provider.map(str::to_owned),
            searxng_url: url.map(str::to_owned),
            brave_api_key: key.map(str::to_owned),
        }
    }

    #[tokio::test]
    async fn env_import_fills_only_empty_settings() {
        let (pool, crypto) = fresh().await;
        // Pre-set the provider; the env value for it must be ignored while
        // the two unset ones are taken over.
        set_provider(&pool, SearchProvider::Searxng).await.unwrap();

        let imported = import_once(
            &pool,
            &crypto,
            env(
                Some("brave"),
                Some("https://from-env.example"),
                Some("env-token"),
            ),
        )
        .await
        .unwrap();
        assert_eq!(imported, vec!["SEARXNG_URL", "BRAVE_SEARCH_API_KEY"]);

        let s = load(&pool, &crypto).await.unwrap();
        // DB wins for the pre-set key.
        assert_eq!(s.provider, SearchProvider::Searxng);
        assert_eq!(s.searxng_url.as_deref(), Some("https://from-env.example"));
        assert_eq!(s.brave_api_key.as_deref(), Some("env-token"));
    }

    #[tokio::test]
    async fn env_import_is_idempotent() {
        let (pool, crypto) = fresh().await;
        let from_env = env(None, Some("https://first.example"), None);
        assert_eq!(
            import_once(&pool, &crypto, from_env.clone()).await.unwrap(),
            vec!["SEARXNG_URL"]
        );
        // Operator then changes it in the UI; a restart must not undo that.
        set_searxng_url(&pool, "https://changed-in-ui.example")
            .await
            .unwrap();
        let second = import_once(&pool, &crypto, from_env).await.unwrap();
        assert!(second.is_empty(), "{second:?}");
        assert_eq!(
            load(&pool, &crypto).await.unwrap().searxng_url.as_deref(),
            Some("https://changed-in-ui.example")
        );
    }

    #[tokio::test]
    async fn env_import_ignores_a_bogus_provider() {
        let (pool, crypto) = fresh().await;
        let imported = import_once(&pool, &crypto, env(Some("altavista"), None, None))
            .await
            .unwrap();
        assert!(imported.is_empty(), "{imported:?}");
        assert_eq!(
            load(&pool, &crypto).await.unwrap().provider,
            SearchProvider::Searxng
        );
    }

    #[tokio::test]
    async fn env_import_with_nothing_set_is_a_no_op() {
        let (pool, crypto) = fresh().await;
        assert!(
            import_once(&pool, &crypto, EnvSearchConfig::default())
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            app_settings::get(&pool, PROVIDER_KEY)
                .await
                .unwrap()
                .is_none()
        );
    }
}
