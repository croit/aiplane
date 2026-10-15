// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Web Push: notify a user that an assistant turn they started finished while
//! they were away from the app.
//!
//! Two RFCs meet here:
//!   - **VAPID** (RFC 8292) authenticates the gateway to the push service. We
//!     hold one P-256 keypair; its public half is the browser's
//!     `applicationServerKey` at subscribe time, and every send carries a
//!     short-lived ES256 JWT signed with the private half. The keypair is
//!     generated once and persisted (private half sealed under the gateway's
//!     at-rest key, like every other stored secret) in `app_settings`.
//!   - **Message Encryption** (RFC 8291, see [`encrypt`]) encrypts the payload
//!     end-to-end for the subscription so the push service can't read it.
//!
//! [`PushSender`] owns the keypair and does one thing: [`PushSender::send`] a
//! [`PushMessage`] to one subscription. [`send_to_user`] is the one fan-out
//! over a user's subscriptions, pruning any the service reports gone — used
//! by the turn-finalize hook, the agent inbox and the `notify_user` tool.

pub mod encrypt;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use rand::TryRng;

use aiplane_core::server::crypto::Crypto;
use aiplane_core::server::db::push_subscriptions::{self, PushSubscription};
use aiplane_core::server::db::{self, DbError, Pool};
use aiplane_core::server::net_guard::{IpClass, classify_host};
use aiplane_core::server::outbound_guard::{self, Pinned, Policy};
use session_core::i18n::Lang;

/// `app_settings` key holding the sealed VAPID private scalar.
const VAPID_PRIVATE_KEY_SETTING: &str = "push.vapid.private";

/// VAPID JWTs live 12h — well inside the 24h ceiling push services enforce,
/// long enough that we don't re-sign on every message within a burst.
const JWT_TTL_SECONDS: i64 = 12 * 60 * 60;

/// How long the push service should retain an undelivered message (seconds).
/// A day: a turn-done ping is stale well before then, but this tolerates a
/// phone that's briefly offline.
const PUSH_TTL_SECONDS: u32 = 24 * 60 * 60;

/// The JSON delivered to the service worker's `push` handler. Kept small and
/// stable — `sw.js` reads exactly these fields.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PushMessage {
    /// Notification title, e.g. the conversation title.
    pub title: String,
    /// Body line under the title.
    pub body: String,
    /// Where `notificationclick` should navigate (a same-origin path).
    pub url: String,
    /// Coalescing tag so repeated pings for one conversation replace rather
    /// than stack — the session id.
    pub tag: String,
}

/// The longest notification title, in characters. The whole payload rides
/// in one aes128gcm record with a ~4 KB budget (and FCM caps the body at
/// 4 KB too), and a phone cuts long text anyway.
pub const MAX_TITLE_CHARS: usize = 80;

/// The longest notification body, in characters. See [`MAX_TITLE_CHARS`].
pub const MAX_BODY_CHARS: usize = 300;

/// What one [`send_to_user`] reached.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FanOut {
    /// The user's subscribed browsers when it started.
    pub subscriptions: usize,
    pub delivered: usize,
    /// Subscriptions the push service reported gone, now deleted.
    pub pruned: usize,
}

/// Send one notification to every browser `user_id` subscribed: the message
/// `compose` writes in each subscription's language (English when it has
/// none), title and body cut to [`MAX_TITLE_CHARS`] / [`MAX_BODY_CHARS`]. A
/// subscription the push service reports gone is deleted; one that fails
/// otherwise is kept for next time. The one fan-out for every caller: a
/// finished turn, the inbox, `notify_user`.
pub async fn send_to_user(
    sender: &PushSender,
    db: &Pool,
    user_id: &str,
    compose: impl Fn(Lang) -> PushMessage,
) -> Result<FanOut, DbError> {
    fan_out(db, user_id, compose, sender).await
}

/// What a fan-out sends through: [`PushSender`], or a stand-in in a test.
/// A seam because every real push service is a public https host
/// `outbound_guard` lets through, and a test can stand none of them up.
trait Deliver {
    fn deliver(
        &self,
        sub: &PushSubscription,
        message: &PushMessage,
    ) -> impl Future<Output = SendOutcome> + Send;
}

impl Deliver for PushSender {
    fn deliver(
        &self,
        sub: &PushSubscription,
        message: &PushMessage,
    ) -> impl Future<Output = SendOutcome> + Send {
        self.send(sub, message)
    }
}

async fn fan_out(
    db: &Pool,
    user_id: &str,
    compose: impl Fn(Lang) -> PushMessage,
    send: &impl Deliver,
) -> Result<FanOut, DbError> {
    let subs = push_subscriptions::list_for_user(db, user_id).await?;
    let mut out = FanOut {
        subscriptions: subs.len(),
        ..FanOut::default()
    };
    for sub in &subs {
        let lang = sub
            .lang
            .as_deref()
            .and_then(Lang::from_code)
            .unwrap_or(Lang::En);
        let message = fitted(compose(lang));
        match send.deliver(sub, &message).await {
            SendOutcome::Delivered => out.delivered += 1,
            SendOutcome::Gone => {
                out.pruned += 1;
                if let Err(err) = push_subscriptions::delete(db, &sub.id).await {
                    tracing::warn!(error = %err, "push: pruning a gone subscription");
                }
            }
            SendOutcome::Failed => {}
        }
    }
    Ok(out)
}

fn fitted(message: PushMessage) -> PushMessage {
    PushMessage {
        title: session_core::text::truncate_chars(&message.title, MAX_TITLE_CHARS),
        body: session_core::text::truncate_chars(&message.body, MAX_BODY_CHARS),
        ..message
    }
}

/// What happened when we posted to a push endpoint.
#[derive(Debug, PartialEq, Eq)]
pub enum SendOutcome {
    /// The push service accepted the message (2xx).
    Delivered,
    /// The subscription is gone (404/410) — the caller should prune it.
    Gone,
    /// A transient or unexpected failure; left in place to retry next time.
    Failed,
}

/// A loaded VAPID keypair (RFC 8292).
struct Vapid {
    signing: SigningKey,
    /// The public key as base64url — the client's `applicationServerKey` and
    /// the `k=` parameter of the `Authorization` header.
    public_b64: String,
}

impl Vapid {
    fn from_private_bytes(priv32: &[u8]) -> anyhow::Result<Self> {
        let signing = SigningKey::from_slice(priv32)
            .map_err(|_| anyhow::anyhow!("VAPID private key is not a valid P-256 scalar"))?;
        let point = signing.verifying_key().to_sec1_point(false);
        let public_b64 = URL_SAFE_NO_PAD.encode(point.as_bytes());
        Ok(Self {
            signing,
            public_b64,
        })
    }

    /// A fresh keypair. Returns the raw private scalar (to seal + persist)
    /// alongside the loaded key.
    fn generate() -> anyhow::Result<([u8; 32], Self)> {
        for _ in 0..8 {
            let mut bytes = [0u8; 32];
            rand::rngs::SysRng
                .try_fill_bytes(&mut bytes)
                .map_err(|e| anyhow::anyhow!("RNG failure generating VAPID key: {e}"))?;
            if let Ok(v) = Self::from_private_bytes(&bytes) {
                return Ok((bytes, v));
            }
        }
        anyhow::bail!("could not generate a valid VAPID scalar")
    }

    /// The `vapid t=<jwt>, k=<pubkey>` Authorization header value for one
    /// endpoint. `aud` is the endpoint's origin; `now_secs` is the current
    /// Unix time (injected so the JWT-building logic is unit-testable).
    fn auth_header(&self, audience: &str, contact: &str, now_secs: i64) -> String {
        let header = URL_SAFE_NO_PAD.encode(br#"{"typ":"JWT","alg":"ES256"}"#);
        let claims = serde_json::json!({
            "aud": audience,
            "exp": now_secs + JWT_TTL_SECONDS,
            "sub": contact,
        });
        let claims_b64 = URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&claims).expect("serializing fixed-shape JWT claims never fails"),
        );
        let signing_input = format!("{header}.{claims_b64}");
        let sig: Signature = self.signing.sign(signing_input.as_bytes());
        let sig_b64 = URL_SAFE_NO_PAD.encode(sig.to_bytes());
        format!("vapid t={signing_input}.{sig_b64}, k={}", self.public_b64)
    }
}

/// Sends encrypted, VAPID-signed push messages. Built once at startup and
/// shared on `AppState`.
pub struct PushSender {
    vapid: Vapid,
    /// The VAPID `sub` claim — a `mailto:` or `https:` contact for the push
    /// service to reach the operator. From `push.contact`.
    contact: String,
}

/// How long one push may take, so one stalled push service can't wedge a
/// user's fan-out or leak the detached notify task.
const PUSH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// A client for one push to `endpoint`, through `outbound_guard`: the host
/// resolved and every address checked public at send time, the client pinned
/// to those addresses (a DNS answer that changed since the browser
/// subscribed cannot point the VAPID-signed POST into the gateway's own
/// network), no proxy and no redirect.
async fn pinned_endpoint(endpoint: &str) -> Result<Pinned, String> {
    outbound_guard::pin(endpoint, Policy::public_https(), PUSH_TIMEOUT).await
}

impl PushSender {
    /// Load (or first-time generate + persist) the VAPID keypair and build a
    /// sender. The keypair's public half is stable across restarts so browsers
    /// keep working without re-subscribing.
    ///
    /// Each push connects through its own pinned client
    /// ([`pinned_endpoint`]), not the shared upstream one.
    pub async fn new(pool: &Pool, crypto: &Crypto, contact: String) -> anyhow::Result<Self> {
        let vapid = load_or_create_vapid(pool, crypto).await?;
        Ok(Self { vapid, contact })
    }

    /// The base64url VAPID public key — served to the client as its
    /// `applicationServerKey`.
    pub fn public_key(&self) -> &str {
        &self.vapid.public_b64
    }

    /// Encrypt `message` for `sub` and POST it to the push service.
    pub async fn send(&self, sub: &PushSubscription, message: &PushMessage) -> SendOutcome {
        let payload = match serde_json::to_vec(message) {
            Ok(p) => p,
            Err(err) => {
                tracing::warn!(error = %err, "serializing push message");
                return SendOutcome::Failed;
            }
        };
        self.send_bytes(sub, &payload).await
    }

    async fn send_bytes(&self, sub: &PushSubscription, payload: &[u8]) -> SendOutcome {
        let ua_public = match b64url_decode(&sub.p256dh) {
            Some(k) => k,
            None => {
                tracing::warn!(endpoint = %sub.endpoint, "subscription p256dh is not base64url");
                return SendOutcome::Failed;
            }
        };
        let auth = match b64url_decode(&sub.auth) {
            Some(a) => a,
            None => {
                tracing::warn!(endpoint = %sub.endpoint, "subscription auth is not base64url");
                return SendOutcome::Failed;
            }
        };
        let body = match encrypt::encrypt(&ua_public, &auth, payload) {
            Ok(b) => b,
            Err(err) => {
                tracing::warn!(error = %err, endpoint = %sub.endpoint, "encrypting push payload");
                return SendOutcome::Failed;
            }
        };

        let Some(audience) = endpoint_origin(&sub.endpoint) else {
            tracing::warn!(endpoint = %sub.endpoint, "push endpoint has no parseable origin");
            return SendOutcome::Failed;
        };
        let now = jiff::Timestamp::now().as_second();
        let auth_header = self.vapid.auth_header(&audience, &self.contact, now);

        let pinned = match pinned_endpoint(&sub.endpoint).await {
            Ok(pinned) => pinned,
            Err(why) => {
                tracing::warn!(endpoint = %sub.endpoint, reason = %why, "push endpoint refused");
                return SendOutcome::Failed;
            }
        };
        let resp = pinned
            .client
            .post(pinned.url)
            .header(reqwest::header::AUTHORIZATION, auth_header)
            .header(reqwest::header::CONTENT_ENCODING, "aes128gcm")
            .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
            .header("TTL", PUSH_TTL_SECONDS.to_string())
            .header("Urgency", "normal")
            .body(body)
            .send()
            .await;

        match resp {
            Ok(r) if r.status().is_success() => SendOutcome::Delivered,
            Ok(r) if matches!(r.status().as_u16(), 404 | 410) => SendOutcome::Gone,
            Ok(r) => {
                tracing::warn!(status = %r.status(), endpoint = %sub.endpoint, "push service rejected message");
                SendOutcome::Failed
            }
            Err(err) => {
                tracing::warn!(error = %err, endpoint = %sub.endpoint, "posting to push service");
                SendOutcome::Failed
            }
        }
    }
}

/// Load the persisted VAPID keypair, generating + storing one on first use (or
/// if the stored value can't be decrypted, e.g. the at-rest key rotated).
async fn load_or_create_vapid(pool: &Pool, crypto: &Crypto) -> anyhow::Result<Vapid> {
    if let Some(stored) = db::app_settings::get(pool, VAPID_PRIVATE_KEY_SETTING).await?
        && let Some(vapid) = open_sealed(&stored, crypto)
    {
        return Ok(vapid);
    }
    if db::app_settings::get(pool, VAPID_PRIVATE_KEY_SETTING)
        .await?
        .is_some()
    {
        tracing::warn!(
            "stored VAPID key could not be decrypted (at-rest key changed?); regenerating — \
             existing push subscriptions will stop delivering until re-subscribed"
        );
    }
    let (priv_bytes, vapid) = Vapid::generate()?;
    let stored = crypto.seal_bytes_to_string(&priv_bytes)?;
    db::app_settings::set(pool, VAPID_PRIVATE_KEY_SETTING, &stored).await?;
    tracing::info!("generated a new VAPID keypair for Web Push");
    Ok(vapid)
}

/// Parse the `nonce.ciphertext` stored form, decrypt, and load the key.
/// `None` on any malformation so the caller regenerates rather than failing.
fn open_sealed(stored: &str, crypto: &Crypto) -> Option<Vapid> {
    let priv_bytes = crypto.open_bytes_from_string(stored)?;
    Vapid::from_private_bytes(&priv_bytes).ok()
}

/// The origin (`scheme://host[:port]`) of a push endpoint — the VAPID `aud`.
fn endpoint_origin(endpoint: &str) -> Option<String> {
    let url = url::Url::parse(endpoint).ok()?;
    let origin = url.origin();
    if origin.is_tuple() {
        Some(origin.ascii_serialization())
    } else {
        None
    }
}

/// Decode base64url, tolerating optional padding (browsers omit it).
fn b64url_decode(s: &str) -> Option<Vec<u8>> {
    URL_SAFE_NO_PAD.decode(s.trim_end_matches('=')).ok()
}

/// Validate a subscription the client is trying to register, BEFORE it's
/// stored — the gateway later POSTs (VAPID-signed) to `endpoint` whenever the
/// owner's turn finishes, so an unchecked endpoint is a blind-SSRF vector.
///
/// Requires:
/// - `endpoint` is an `https` URL whose host is a name other than
///   `localhost` or an address `net_guard` classifies as public (blocks the
///   cloud metadata IP, loopback, RFC 1918 / ULA, CGNAT, multicast, and their
///   IPv4-mapped IPv6 spellings), and
/// - `p256dh` decodes to a 65-byte uncompressed P-256 point and `auth` to a
///   16-byte secret (so we never persist junk that can only ever fail to
///   encrypt).
///
/// A public hostname that resolves to an internal IP (DNS rebinding) is
/// caught at send time instead: every push resolves, checks and pins the
/// endpoint again ([`pinned_endpoint`]).
pub fn validate_subscription(endpoint: &str, p256dh: &str, auth: &str) -> Result<(), String> {
    let url = url::Url::parse(endpoint).map_err(|_| "endpoint is not a valid URL".to_string())?;
    if url.scheme() != "https" {
        return Err("push endpoint must be https".to_string());
    }
    let public = match url.host() {
        Some(url::Host::Domain(d)) => !d.eq_ignore_ascii_case("localhost"),
        Some(host) => classify_host(&host).is_some_and(IpClass::is_public),
        None => false,
    };
    if !public {
        return Err("push endpoint host is not allowed".to_string());
    }
    match b64url_decode(p256dh) {
        Some(k) if k.len() == 65 && k[0] == 0x04 => {}
        _ => return Err("keys.p256dh must be a base64url 65-byte P-256 point".to_string()),
    }
    match b64url_decode(auth) {
        Some(a) if a.len() == 16 => {}
        _ => return Err("keys.auth must be a base64url 16-byte secret".to_string()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vapid() -> Vapid {
        Vapid::from_private_bytes(&[5u8; 32]).unwrap()
    }

    #[test]
    fn public_key_is_a_65_byte_uncompressed_point() {
        let bytes = URL_SAFE_NO_PAD.decode(vapid().public_b64).unwrap();
        assert_eq!(bytes.len(), 65);
        assert_eq!(bytes[0], 0x04, "uncompressed SEC1 point prefix");
    }

    #[test]
    fn auth_header_is_a_verifiable_es256_jwt() {
        use p256::ecdsa::signature::Verifier;

        let v = vapid();
        let header = v.auth_header(
            "https://fcm.googleapis.com",
            "mailto:ops@example.com",
            1_700_000_000,
        );
        // Shape: "vapid t=<jwt>, k=<pubkey>".
        let rest = header.strip_prefix("vapid t=").expect("vapid scheme");
        let (jwt, k) = rest.split_once(", k=").expect("t= and k= params");
        assert_eq!(k, v.public_b64, "k= carries our public key");

        let parts: Vec<&str> = jwt.split('.').collect();
        assert_eq!(parts.len(), 3, "header.claims.signature");

        // Header + claims decode and carry the expected fields.
        let claims: serde_json::Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
        assert_eq!(claims["aud"], "https://fcm.googleapis.com");
        assert_eq!(claims["sub"], "mailto:ops@example.com");
        assert_eq!(claims["exp"], 1_700_000_000 + JWT_TTL_SECONDS);

        // The signature verifies against the VAPID public key over
        // "header.claims" — i.e. it's a real ES256 JWT.
        let signing_input = format!("{}.{}", parts[0], parts[1]);
        let sig = Signature::from_slice(&URL_SAFE_NO_PAD.decode(parts[2]).unwrap()).unwrap();
        v.signing
            .verifying_key()
            .verify(signing_input.as_bytes(), &sig)
            .expect("VAPID JWT signature verifies");
    }

    /// A push service answering by the endpoint's last segment: `gone`,
    /// `flaky` (a failure), anything else delivered. Records what it got.
    #[derive(Default)]
    struct PushService {
        sent: std::sync::Mutex<Vec<(String, PushMessage)>>,
    }

    impl Deliver for PushService {
        async fn deliver(&self, sub: &PushSubscription, message: &PushMessage) -> SendOutcome {
            self.sent
                .lock()
                .unwrap()
                .push((sub.endpoint.clone(), message.clone()));
            match sub.endpoint.rsplit('/').next() {
                Some("gone") => SendOutcome::Gone,
                Some("flaky") => SendOutcome::Failed,
                _ => SendOutcome::Delivered,
            }
        }
    }

    async fn subscribed(pool: &Pool, user: &str, endpoint: &str, lang: Option<&str>) {
        sqlx::query(
            "INSERT INTO users (id, email, created_at, updated_at)
             VALUES (?, ?, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')
             ON CONFLICT(id) DO NOTHING",
        )
        .bind(user)
        .bind(format!("{user}@example.com"))
        .execute(pool)
        .await
        .unwrap();
        push_subscriptions::upsert(pool, user, endpoint, P256DH, AUTH, lang, None)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn a_fan_out_speaks_each_browsers_language_cuts_the_text_and_prunes_the_gone() {
        let pool = aiplane_core::server::db::open(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        subscribed(&pool, "u1", "https://push.example/de", Some("de")).await;
        subscribed(&pool, "u1", "https://push.example/gone", None).await;
        subscribed(&pool, "u1", "https://push.example/flaky", Some("fr")).await;
        subscribed(&pool, "u2", "https://push.example/other", None).await;
        let service = PushService::default();

        let out = fan_out(
            &pool,
            "u1",
            |lang| PushMessage {
                title: format!("{}{}", lang.code(), "t".repeat(200)),
                body: "b".repeat(1000),
                url: "/chat/s1".into(),
                tag: "s1".into(),
            },
            &service,
        )
        .await
        .unwrap();

        assert_eq!(
            out,
            FanOut {
                subscriptions: 3,
                delivered: 1,
                pruned: 1
            }
        );
        let sent = service.sent.into_inner().unwrap();
        let mut langs: Vec<(String, String)> = sent
            .iter()
            .map(|(endpoint, m)| (endpoint.clone(), m.title[..2].to_string()))
            .collect();
        langs.sort();
        assert_eq!(
            langs,
            [
                ("https://push.example/de".to_string(), "de".to_string()),
                ("https://push.example/flaky".to_string(), "fr".to_string()),
                ("https://push.example/gone".to_string(), "en".to_string()),
            ]
        );
        for (_, m) in &sent {
            assert_eq!(m.title.chars().count(), MAX_TITLE_CHARS + 1, "{}", m.title);
            assert!(m.title.ends_with('…'), "cut, and marked as cut");
            assert_eq!(m.body.chars().count(), MAX_BODY_CHARS + 1);
        }
        let left: Vec<String> = push_subscriptions::list_for_user(&pool, "u1")
            .await
            .unwrap()
            .into_iter()
            .map(|s| s.endpoint)
            .collect();
        assert_eq!(left.len(), 2, "only the gone one went: {left:?}");
        assert!(!left.iter().any(|e| e.ends_with("/gone")));
    }

    #[tokio::test]
    async fn a_user_without_subscriptions_reaches_nobody() {
        let pool = aiplane_core::server::db::open(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        let out = fan_out(
            &pool,
            "nobody",
            |_| unreachable!("nothing to compose for"),
            &PushService::default(),
        )
        .await
        .unwrap();
        assert_eq!(out, FanOut::default());
    }

    #[test]
    fn endpoint_origin_strips_the_path() {
        assert_eq!(
            endpoint_origin("https://fcm.googleapis.com/fcm/send/abc123").as_deref(),
            Some("https://fcm.googleapis.com"),
        );
        assert_eq!(
            endpoint_origin("https://updates.push.services.mozilla.com:443/wpush/v2/xyz")
                .as_deref(),
            Some("https://updates.push.services.mozilla.com"),
        );
        assert_eq!(endpoint_origin("not a url"), None);
    }

    #[test]
    fn b64url_decode_tolerates_padding() {
        assert_eq!(b64url_decode("YWJj"), Some(b"abc".to_vec()));
        assert_eq!(b64url_decode("YWJj=="), Some(b"abc".to_vec()));
    }

    // Valid RFC 8291 §5 key material for the validation tests.
    const P256DH: &str =
        "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4";
    const AUTH: &str = "BTBZMqHH6r4Tts7J_aSIgg";

    #[test]
    fn validate_subscription_accepts_a_public_https_endpoint() {
        assert!(
            validate_subscription("https://fcm.googleapis.com/fcm/send/abc", P256DH, AUTH).is_ok()
        );
    }

    #[test]
    fn validate_subscription_rejects_ssrf_and_non_https_targets() {
        // Cloud metadata IP, loopback, private range, link-local, CGNAT,
        // IPv4-mapped spellings of those, multicast, localhost, http.
        for bad in [
            "https://169.254.169.254/latest/meta-data/",
            "https://127.0.0.1/x",
            "https://10.0.0.5/x",
            "https://192.168.1.1/x",
            "https://[::1]/x",
            "https://[fd00::1]/x",
            "https://[fe80::1]/x",
            "https://100.64.0.1/x",
            "https://[::ffff:127.0.0.1]/x",
            "https://[::ffff:169.254.169.254]/x",
            "https://[::ffff:10.0.0.5]/x",
            "https://224.0.0.1/x",
            "https://localhost/x",
            "http://fcm.googleapis.com/x",
            "ftp://fcm.googleapis.com/x",
            "not a url",
        ] {
            assert!(
                validate_subscription(bad, P256DH, AUTH).is_err(),
                "should reject endpoint: {bad}"
            );
        }
    }

    /// The send-time guard, which also covers a name the subscribe-time
    /// check let through: one that resolves into the gateway's own network
    /// (`localhost.` with its trailing dot passes `validate_subscription`,
    /// as any public-looking name whose DNS answer later changes would).
    #[tokio::test]
    async fn a_push_to_an_endpoint_resolving_to_a_private_address_is_refused() {
        assert!(validate_subscription("https://localhost./x", P256DH, AUTH).is_ok());
        for endpoint in [
            "https://localhost./x",
            "https://localhost/x",
            "https://127.0.0.1/x",
            "https://10.0.0.5/x",
            "https://169.254.169.254/latest/meta-data/",
            "http://93.184.216.34/x",
        ] {
            assert!(
                pinned_endpoint(endpoint).await.is_err(),
                "should refuse {endpoint}"
            );
        }
        let pinned = pinned_endpoint("https://93.184.216.34/push/abc")
            .await
            .unwrap();
        assert_eq!(pinned.url.as_str(), "https://93.184.216.34/push/abc");
    }

    #[test]
    fn validate_subscription_rejects_bad_key_material() {
        // Wrong p256dh length / prefix, wrong auth length.
        assert!(validate_subscription("https://fcm.googleapis.com/x", "AAAA", AUTH).is_err());
        assert!(validate_subscription("https://fcm.googleapis.com/x", P256DH, "AAAA").is_err());
        assert!(
            validate_subscription("https://fcm.googleapis.com/x", "not!base64!", AUTH).is_err()
        );
    }
}
