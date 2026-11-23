// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The one way to connect to a destination the operator does not configure
//! himself: `fetch_url`, `load_image_url`, `tls_cert`, `dns_lookup` and
//! `whois_lookup` for the model, an A2A route's card, endpoint and OAuth
//! token URL and a `host_jwt` verifier's JWKS URL for an agent's owner, a
//! Web Push endpoint a browser registered, and the MCP OAuth flow's
//! discovery, registration and token endpoints. Each caller names its
//! [`Policy`].
//!
//! Before every connection the host is resolved here, every address it
//! resolves to is checked, and the HTTP client is pinned to exactly those
//! addresses, so a second lookup (DNS rebinding) cannot swap a private
//! address in between the check and the request. The client never follows a
//! redirect itself: [`get`] follows them one hop at a time and puts every
//! hop through the same check, so a public page cannot bounce the gateway
//! into its own network.
//!
//! Always refused: unspecified, link-local (the cloud metadata address
//! `169.254.169.254` among them), broadcast and multicast addresses. Refused
//! unless the operator sets `$AIPLANE_ALLOW_PRIVATE_NETWORKS`
//! ([`PRIVATE_NETWORKS_VAR`]): loopback, private (RFC 1918, IPv6 unique
//! local) and carrier-grade NAT addresses.
//!
//! What an address is comes from `net_guard::classify`, shared with the
//! other outbound guards; the policies are this module's own. The MCP OAuth
//! one ([`Policy::mcp_oauth`]) allows private ranges on purpose — an admin
//! curates the MCP catalog — while still refusing link-local and the other
//! never-reached ranges. What comes back is read through `capped_read`, so
//! the peer does not decide how much is buffered.
//!
//! A host's addresses and the client pinned to them are reused for
//! [`CACHE_TTL`], so a tool reading many pages of one site neither resolves
//! it nor sets up a client per request. The addresses are checked against
//! the caller's policy on every use, cached or not, so a cached answer is
//! never let through where a fresh one would be refused; caching only
//! defers noticing that the host's DNS changed.

use std::collections::HashMap;
use std::hash::Hash;
use std::net::{IpAddr, SocketAddr};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use reqwest::Url;

use crate::server::config::PRIVATE_NETWORKS_VAR;
use crate::server::net_guard::{IpClass, classify, is_loopback_host};

/// How many redirects [`get`] follows before giving up.
pub const MAX_REDIRECTS: usize = 5;

/// How long a host's resolved addresses, and a client pinned to them, are
/// reused.
pub const CACHE_TTL: Duration = Duration::from_secs(30);

/// The longest a host's lookup may take. The caller's timeout covers only
/// the request, and a resolver that never answers would otherwise hold the
/// tool call for as long as the system resolver retries.
pub const DNS_BOUND: Duration = Duration::from_secs(5);

/// Entries either cache holds before the stale ones are dropped.
const CACHE_MAX: usize = 1024;

/// Values that expire [`CACHE_TTL`] after they were stored.
struct Expiring<K, V> {
    entries: Mutex<HashMap<K, (Instant, V)>>,
}

impl<K: Eq + Hash, V: Clone> Expiring<K, V> {
    fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }

    fn get(&self, key: &K, now: Instant) -> Option<V> {
        let entries = self.entries.lock().unwrap_or_else(|p| p.into_inner());
        entries
            .get(key)
            .filter(|(at, _)| now.saturating_duration_since(*at) < CACHE_TTL)
            .map(|(_, v)| v.clone())
    }

    fn put(&self, key: K, value: V, now: Instant) {
        let mut entries = self.entries.lock().unwrap_or_else(|p| p.into_inner());
        if entries.len() >= CACHE_MAX {
            entries.retain(|_, (at, _)| now.saturating_duration_since(*at) < CACHE_TTL);
            if entries.len() >= CACHE_MAX {
                entries.clear();
            }
        }
        entries.insert(key, (now, value));
    }
}

/// What a host and port resolved to, unchecked: each use checks it.
static RESOLVED: LazyLock<Expiring<(String, u16), Vec<SocketAddr>>> = LazyLock::new(Expiring::new);

/// Clients pinned to a host's checked addresses, by host, addresses and
/// timeout.
type ClientKey = (String, Vec<SocketAddr>, Duration);
static CLIENTS: LazyLock<Expiring<ClientKey, reqwest::Client>> = LazyLock::new(Expiring::new);

/// Which schemes a destination may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Schemes {
    /// `https`; plain `http` only where private networks are allowed. For
    /// a peer that receives credentials or bound values.
    HttpsUnlessPrivate,
    /// `http` or `https`. For reading public web content.
    HttpOrHttps,
    /// `https`; plain `http` only to a loopback host (`localhost` or a
    /// loopback address), for a self-hosted server in development.
    HttpsUnlessLoopback,
}

/// What a destination may be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Policy {
    pub allow_private: bool,
    pub schemes: Schemes,
}

impl Policy {
    /// A URL an agent's owner names (A2A routes, JWKS).
    pub fn agent(allow_private: bool) -> Self {
        Self {
            allow_private,
            schemes: Schemes::HttpsUnlessPrivate,
        }
    }

    /// A URL a model asks the gateway to read.
    pub fn web(allow_private: bool) -> Self {
        Self {
            allow_private,
            schemes: Schemes::HttpOrHttps,
        }
    }

    /// A public host over https and nothing else, whatever the operator
    /// allows elsewhere: a Web Push endpoint a browser registered, a
    /// registry an RDAP lookup is sent on to.
    pub fn public_https() -> Self {
        Self {
            allow_private: false,
            schemes: Schemes::HttpsUnlessPrivate,
        }
    }

    /// The MCP OAuth flow's discovery, registration and token endpoints:
    /// the admin-curated catalog may sit in a private network, so private
    /// and loopback addresses are allowed, plain http to loopback only.
    pub fn mcp_oauth() -> Self {
        Self {
            allow_private: true,
            schemes: Schemes::HttpsUnlessLoopback,
        }
    }
}

/// A host that passed the guard, and the client pinned to its addresses.
pub struct Pinned {
    pub url: Url,
    pub client: reqwest::Client,
}

/// One redirect a [`get_traced`] followed: the status that sent it on, and the
/// address it pointed to.
#[derive(Debug, Clone)]
pub struct Hop {
    pub status: u16,
    pub url: Url,
}

/// [`get_traced`]'s answer: the last hop's response, the address that was
/// asked for, and every redirect between the two.
pub struct Fetched {
    pub response: reqwest::Response,
    /// What the caller asked for, before any redirect.
    pub requested: Url,
    /// The redirects followed, in order; the last one's `url` is where the
    /// answer came from. Empty when the address answered directly.
    pub hops: Vec<Hop>,
}

/// Whether `ip` may be connected to, and why not.
pub fn check_ip(ip: IpAddr, allow_private: bool) -> Result<(), String> {
    let class = classify(ip);
    match class {
        IpClass::Public => Ok(()),
        IpClass::Unspecified | IpClass::LinkLocal | IpClass::Broadcast | IpClass::Multicast => Err(
            format!("{ip} is {}, which is never reached", class.describe()),
        ),
        IpClass::Loopback | IpClass::Private | IpClass::Cgnat if allow_private => Ok(()),
        IpClass::Loopback | IpClass::Private | IpClass::Cgnat => Err(format!(
            "{ip} is {}; a URL chosen by a user, a model or an agent's owner reaches public \
             hosts only, unless the operator sets `${PRIVATE_NETWORKS_VAR}=true`",
            class.describe()
        )),
    }
}

fn check_scheme(url: &Url, policy: Policy) -> Result<(), String> {
    match (url.scheme(), policy.schemes) {
        ("https", _) => Ok(()),
        ("http", Schemes::HttpOrHttps) => Ok(()),
        ("http", Schemes::HttpsUnlessPrivate) if policy.allow_private => Ok(()),
        ("http", Schemes::HttpsUnlessPrivate) => Err(format!(
            "{url} is plain http; this destination is reached over https only (plain http \
             only where the operator sets `${PRIVATE_NETWORKS_VAR}=true`)"
        )),
        ("http", Schemes::HttpsUnlessLoopback)
            if url.host().is_some_and(|h| is_loopback_host(&h)) =>
        {
            Ok(())
        }
        ("http", Schemes::HttpsUnlessLoopback) => Err(format!(
            "{url} is plain http; this destination is reached over https only (plain http only \
             to localhost)"
        )),
        (other, _) => Err(format!("{url} uses `{other}`; only http(s) is allowed")),
    }
}

/// Every address `host` resolves to, each one checked. A literal address is
/// checked as it is.
pub async fn resolve(
    host: &str,
    port: u16,
    allow_private: bool,
) -> Result<Vec<SocketAddr>, String> {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let addrs: Vec<SocketAddr> = match host.parse::<IpAddr>() {
        Ok(ip) => vec![SocketAddr::new(ip, port)],
        Err(_) => lookup(host, port).await?,
    };
    if addrs.is_empty() {
        return Err(format!("`{host}` resolves to no address"));
    }
    for addr in &addrs {
        check_ip(addr.ip(), allow_private).map_err(|why| format!("`{host}` resolves to {why}"))?;
    }
    Ok(addrs)
}

/// `host`'s addresses, from [`RESOLVED`] or a lookup of at most
/// [`DNS_BOUND`].
async fn lookup(host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
    let key = (host.to_ascii_lowercase(), port);
    if let Some(addrs) = RESOLVED.get(&key, Instant::now()) {
        return Ok(addrs);
    }
    let addrs: Vec<SocketAddr> =
        tokio::time::timeout(DNS_BOUND, tokio::net::lookup_host((host, port)))
            .await
            .map_err(|_| {
                format!(
                    "resolving `{host}` took longer than {} s; try again later",
                    DNS_BOUND.as_secs()
                )
            })?
            .map_err(|e| format!("cannot resolve `{host}` ({e})"))?
            .collect();
    if !addrs.is_empty() {
        RESOLVED.put(key, addrs.clone(), Instant::now());
    }
    Ok(addrs)
}

/// A client pinned to `addrs` for `host` that uses no proxy and follows no
/// redirect, reused from [`CLIENTS`] when one was built for the same.
fn pinned_client(
    host: &str,
    addrs: &[SocketAddr],
    timeout: Duration,
) -> Result<reqwest::Client, String> {
    let mut sorted = addrs.to_vec();
    sorted.sort_unstable();
    let key = (host.to_ascii_lowercase(), sorted, timeout);
    if let Some(client) = CLIENTS.get(&key, Instant::now()) {
        return Ok(client);
    }
    // The guarded client: pinned to the addresses checked by the caller. A
    // proxy would resolve the host again on its own, so none is ever used.
    #[allow(clippy::disallowed_methods)]
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(timeout)
        .resolve_to_addrs(host, addrs)
        .build()
        .map_err(|e| format!("building the HTTP client failed: {e}"))?;
    CLIENTS.put(key, client.clone(), Instant::now());
    Ok(client)
}

/// What can be told about `raw` under `policy` without resolving it: the
/// scheme, a host, no credentials in the URL, and a literal address (or
/// `localhost`) the policy reaches. A configuration is checked with it when
/// it is saved, so that a URL [`pin`] would refuse at run time is refused
/// then already, with the same reason; a host name is resolved only then.
pub fn check_url(raw: &str, policy: Policy) -> Result<Url, String> {
    let url = Url::parse(raw.trim()).map_err(|e| format!("`{raw}` is not a URL ({e})"))?;
    check_scheme(&url, policy)?;
    let host = url.host().ok_or_else(|| format!("`{url}` names no host"))?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(format!(
            "`{}` carries a user name or password; a URL here must not",
            url.host_str().unwrap_or_default()
        ));
    }
    let literal = match host {
        url::Host::Ipv4(ip) => Some(IpAddr::V4(ip)),
        url::Host::Ipv6(ip) => Some(IpAddr::V6(ip)),
        url::Host::Domain(_) if is_loopback_host(&host) => {
            Some(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST))
        }
        url::Host::Domain(_) => None,
    };
    if let Some(ip) = literal {
        check_ip(ip, policy.allow_private).map_err(|why| format!("`{url}`: {why}"))?;
    }
    Ok(url)
}

/// Resolve `raw`, check every address, and build a client pinned to them
/// that follows no redirect.
pub async fn pin(raw: &str, policy: Policy, timeout: Duration) -> Result<Pinned, String> {
    let url = Url::parse(raw).map_err(|e| format!("`{raw}` is not a URL ({e})"))?;
    pin_url(url, policy, timeout).await
}

async fn pin_url(url: Url, policy: Policy, timeout: Duration) -> Result<Pinned, String> {
    check_scheme(&url, policy)?;
    let host = url
        .host_str()
        .ok_or_else(|| format!("`{url}` names no host"))?
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_string();
    let port = url
        .port_or_known_default()
        .ok_or_else(|| format!("`{url}` has no port"))?;
    let addrs = resolve(&host, port, policy.allow_private).await?;
    let client = pinned_client(&host, &addrs, timeout)?;
    Ok(Pinned { url, client })
}

/// `GET raw`, following up to [`MAX_REDIRECTS`] redirects, each hop resolved,
/// checked and pinned afresh. The answer is the last hop's, unread; its
/// `url()` is where it came from.
pub async fn get(
    raw: &str,
    policy: Policy,
    timeout: Duration,
    user_agent: &str,
) -> Result<reqwest::Response, String> {
    get_with(
        raw,
        policy,
        timeout,
        &[(reqwest::header::USER_AGENT.as_str(), user_agent)],
    )
    .await
}

/// [`get`] with these request headers on every hop.
pub async fn get_with(
    raw: &str,
    policy: Policy,
    timeout: Duration,
    headers: &[(&str, &str)],
) -> Result<reqwest::Response, String> {
    get_traced(raw, policy, timeout, headers)
        .await
        .map(|fetched| fetched.response)
}

/// [`get_with`], reporting how it got to the answer.
///
/// A redirect is not a detail the caller can reconstruct: the response only
/// carries where it ended up. A caller that shows the address to a model or a
/// person needs the one that was asked for and the status that moved it, or a
/// correct answer under a different URL reads as an answer to a different
/// question.
pub async fn get_traced(
    raw: &str,
    policy: Policy,
    timeout: Duration,
    headers: &[(&str, &str)],
) -> Result<Fetched, String> {
    let requested = Url::parse(raw).map_err(|e| format!("`{raw}` is not a URL ({e})"))?;
    let mut url = requested.clone();
    let mut hops = Vec::new();
    for _ in 0..=MAX_REDIRECTS {
        let pinned = pin_url(url.clone(), policy, timeout).await?;
        let mut request = pinned.client.get(pinned.url);
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let resp = request
            .send()
            .await
            .map_err(|e| format!("fetching {url} failed: {e}"))?;
        let location = resp
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok());
        let Some(location) = location.filter(|_| resp.status().is_redirection()) else {
            return Ok(Fetched {
                response: resp,
                requested,
                hops,
            });
        };
        url = url
            .join(location)
            .map_err(|e| format!("{url} redirects to `{location}`, which is not a URL ({e})"))?;
        hops.push(Hop {
            status: resp.status().as_u16(),
            url: url.clone(),
        });
    }
    Err(format!(
        "{raw} redirects more than {MAX_REDIRECTS} times; giving up"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    const T: Duration = Duration::from_secs(5);

    #[test]
    fn metadata_link_local_and_unspecified_are_never_reached() {
        for addr in [
            "169.254.169.254",
            "0.0.0.0",
            "255.255.255.255",
            "224.0.0.1",
            "fe80::1",
            "::",
            "ff02::1",
            "::ffff:169.254.169.254",
        ] {
            assert!(check_ip(ip(addr), true).is_err(), "{addr}");
            assert!(check_ip(ip(addr), false).is_err(), "{addr}");
        }
    }

    #[test]
    fn private_and_loopback_need_the_operator_switch() {
        for addr in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "100.64.0.1",
            "::1",
            "fd00::1",
            "::ffff:10.0.0.1",
            "::ffff:127.0.0.1",
        ] {
            let refused = check_ip(ip(addr), false).unwrap_err();
            assert!(refused.contains(PRIVATE_NETWORKS_VAR), "{refused}");
            assert!(check_ip(ip(addr), true).is_ok(), "{addr}");
        }
    }

    #[test]
    fn public_addresses_pass() {
        for addr in ["93.184.216.34", "2606:4700::1111", "100.128.0.1"] {
            assert!(check_ip(ip(addr), false).is_ok(), "{addr}");
        }
    }

    #[tokio::test]
    async fn the_agent_policy_wants_https_and_refuses_private_literals_before_connecting() {
        let http = pin("http://93.184.216.34/a2a", Policy::agent(false), T)
            .await
            .err()
            .unwrap();
        assert!(http.contains("https"), "{http}");
        let lo = pin("https://127.0.0.1:9/a2a", Policy::agent(false), T)
            .await
            .err()
            .unwrap();
        assert!(lo.contains("loopback"), "{lo}");
        let meta = pin("http://169.254.169.254/latest", Policy::agent(true), T)
            .await
            .err()
            .unwrap();
        assert!(meta.contains("link-local"), "{meta}");
        assert!(
            pin("http://127.0.0.1:9/a2a", Policy::agent(true), T)
                .await
                .is_ok()
        );
        assert!(
            pin("ftp://example.com/x", Policy::agent(true), T)
                .await
                .is_err()
        );
    }

    #[test]
    fn a_url_is_checked_before_saving_as_it_is_pinned_at_run_time() {
        let agent = Policy::agent(false);
        assert!(check_url("https://partner.example.com/card", agent).is_ok());
        for refused in [
            "http://partner.example.com/card",
            "https://127.0.0.1:9/card",
            "https://localhost/card",
            "https://10.0.0.1/card",
            "https://[::1]/card",
            "https://169.254.169.254/latest",
            "https://user:pw@partner.example.com/card",
            "ftp://partner.example.com/card",
            "not a url",
        ] {
            assert!(check_url(refused, agent).is_err(), "{refused}");
        }
        let private = Policy::agent(true);
        assert!(check_url("http://127.0.0.1:9/card", private).is_ok());
        assert!(check_url("http://localhost:9/card", private).is_ok());
        assert!(
            check_url("https://169.254.169.254/latest", private).is_err(),
            "never reached, whatever the operator allows"
        );
        let why = check_url("https://127.0.0.1/card", agent).unwrap_err();
        assert!(why.contains(PRIVATE_NETWORKS_VAR), "{why}");
        assert!(
            !check_url("https://u:hunter2@example.com/", agent)
                .unwrap_err()
                .contains("hunter2"),
            "the refusal does not repeat the password"
        );
    }

    #[tokio::test]
    async fn the_public_https_policy_takes_neither_http_nor_a_private_host() {
        let http = pin("http://93.184.216.34/", Policy::public_https(), T)
            .await
            .err()
            .unwrap();
        assert!(http.contains("https"), "{http}");
        for url in [
            "https://127.0.0.1:9/",
            "https://localhost:9/",
            "https://10.0.0.1/",
        ] {
            let why = pin(url, Policy::public_https(), T).await.err().unwrap();
            assert!(why.contains(PRIVATE_NETWORKS_VAR), "{url}: {why}");
        }
        assert!(
            pin("https://93.184.216.34/", Policy::public_https(), T)
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn the_mcp_oauth_policy_allows_private_hosts_but_plain_http_only_to_loopback() {
        for ok in [
            "https://10.1.2.3/token",
            "http://127.0.0.1:9000/token",
            "http://localhost:9000/token",
            "http://[::1]:9000/token",
        ] {
            assert!(pin(ok, Policy::mcp_oauth(), T).await.is_ok(), "{ok}");
        }
        for bad in [
            "http://93.184.216.34/token",
            "http://10.1.2.3/token",
            "https://169.254.169.254/latest/meta-data",
            "https://0.0.0.0/x",
            "https://[fe80::1]/x",
            "https://[::ffff:169.254.169.254]/x",
            "not a url",
        ] {
            assert!(pin(bad, Policy::mcp_oauth(), T).await.is_err(), "{bad}");
        }
    }

    #[tokio::test]
    async fn the_web_policy_takes_plain_http_to_a_public_host() {
        assert!(
            pin("http://93.184.216.34/", Policy::web(false), T)
                .await
                .is_ok()
        );
        assert!(
            pin("file:///etc/passwd", Policy::web(true), T)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn the_gateways_own_network_is_refused_before_connecting() {
        for url in [
            "http://127.0.0.1:9/",
            "http://localhost:9/",
            "http://10.0.0.1/",
            "http://[::ffff:127.0.0.1]:9/",
            "http://169.254.169.254/latest/meta-data/",
        ] {
            let why = get(url, Policy::web(false), T, "t").await.unwrap_err();
            assert!(
                why.contains(PRIVATE_NETWORKS_VAR) || why.contains("never reached"),
                "{url}: {why}"
            );
        }
    }

    // wiremock listens on loopback, so these run with private networks
    // allowed; the hop the redirect lands on is link-local, which no switch
    // allows, so a refusal proves the hop was checked again.
    #[tokio::test]
    async fn a_redirect_is_checked_again_at_every_hop() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/bounce"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("location", "http://169.254.169.254/latest/meta-data/"),
            )
            .mount(&server)
            .await;
        let why = get(
            &format!("{}/bounce", server.uri()),
            Policy::web(true),
            T,
            "t",
        )
        .await
        .unwrap_err();
        assert!(why.contains("link-local"), "{why}");
    }

    #[tokio::test]
    async fn a_redirect_into_a_private_network_is_refused_without_the_switch() {
        // The first hop must be public to get as far as the redirect, and no
        // test peer is; check the second hop through the same path `get`
        // takes for it.
        let next = Url::parse("http://93.184.216.34/")
            .unwrap()
            .join("http://10.0.0.1/admin")
            .unwrap();
        let why = pin_url(next, Policy::web(false), T).await.err().unwrap();
        assert!(why.contains(PRIVATE_NETWORKS_VAR), "{why}");
    }

    #[tokio::test]
    async fn an_allowed_redirect_is_followed_to_its_answer() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/old"))
            .respond_with(ResponseTemplate::new(301).insert_header("location", "/new"))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/new"))
            .respond_with(ResponseTemplate::new(200).set_body_string("here"))
            .mount(&server)
            .await;
        let resp = get(&format!("{}/old", server.uri()), Policy::web(true), T, "t")
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        assert_eq!(resp.url().path(), "/new");
    }

    /// The chain is the one thing a caller cannot recover from the response:
    /// it names only where the request ended up.
    #[tokio::test]
    async fn a_followed_redirect_is_reported_hop_by_hop() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/moved"))
            .respond_with(ResponseTemplate::new(301).insert_header("location", "/middle"))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/middle"))
            .respond_with(ResponseTemplate::new(308).insert_header("location", "/final"))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/final"))
            .respond_with(ResponseTemplate::new(200).set_body_string("here"))
            .mount(&server)
            .await;
        let fetched = get_traced(
            &format!("{}/moved", server.uri()),
            Policy::web(true),
            T,
            &[],
        )
        .await
        .unwrap();
        assert_eq!(fetched.response.status(), 200);
        assert_eq!(fetched.requested.path(), "/moved");
        let hops: Vec<(u16, &str)> = fetched
            .hops
            .iter()
            .map(|hop| (hop.status, hop.url.path()))
            .collect();
        assert_eq!(hops, [(301, "/middle"), (308, "/final")]);
        assert_eq!(fetched.response.url().path(), "/final");
    }

    #[tokio::test]
    async fn an_answer_without_a_redirect_reports_no_hops() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/direct"))
            .respond_with(ResponseTemplate::new(200).set_body_string("here"))
            .mount(&server)
            .await;
        let fetched = get_traced(
            &format!("{}/direct", server.uri()),
            Policy::web(true),
            T,
            &[],
        )
        .await
        .unwrap();
        assert!(fetched.hops.is_empty());
        assert_eq!(fetched.requested, *fetched.response.url());
    }

    /// A proxy resolves the host itself, after the guard checked it, so a
    /// guarded request never goes through one, whatever the environment says.
    #[tokio::test]
    async fn an_environment_proxy_is_never_used() {
        let proxy = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string("via proxy"))
            .mount(&proxy)
            .await;
        let target = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/page"))
            .respond_with(ResponseTemplate::new(200).set_body_string("direct"))
            .mount(&target)
            .await;
        const VARS: [&str; 4] = ["HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy"];
        // nextest runs every test in a process of its own; the variables are
        // read when the client is built and removed right after.
        unsafe {
            for var in VARS {
                std::env::set_var(var, proxy.uri());
            }
            std::env::remove_var("NO_PROXY");
            std::env::remove_var("no_proxy");
        }
        let pinned = pin(&format!("{}/page", target.uri()), Policy::web(true), T).await;
        unsafe {
            for var in VARS {
                std::env::remove_var(var);
            }
        }
        let pinned = pinned.unwrap();
        let resp = pinned.client.get(pinned.url).send().await.unwrap();
        let body = crate::server::capped_read::read_capped_text(resp, 1024)
            .await
            .unwrap();
        assert_eq!(body, "direct");
    }

    #[test]
    fn a_cached_value_expires_after_the_ttl() {
        let cache: Expiring<&str, u8> = Expiring::new();
        let t0 = Instant::now();
        cache.put("host", 1, t0);
        assert_eq!(cache.get(&"host", t0 + CACHE_TTL / 2), Some(1));
        assert_eq!(cache.get(&"host", t0 + CACHE_TTL), None);
    }

    #[tokio::test]
    async fn a_cached_address_is_checked_again_against_each_callers_policy() {
        let allowed = resolve("localhost", 9, true).await.unwrap();
        assert!(allowed.iter().all(|a| a.ip().is_loopback()), "{allowed:?}");
        assert!(
            RESOLVED
                .get(&("localhost".to_string(), 9), Instant::now())
                .is_some(),
            "the lookup is cached"
        );
        let refused = resolve("localhost", 9, false).await.unwrap_err();
        assert!(refused.contains(PRIVATE_NETWORKS_VAR), "{refused}");
        let pinned = pin("https://localhost:9/", Policy::public_https(), T)
            .await
            .err()
            .unwrap();
        assert!(pinned.contains(PRIVATE_NETWORKS_VAR), "{pinned}");
    }

    #[tokio::test]
    async fn a_client_is_reused_for_the_same_pin_only() {
        let a = pin("http://127.0.0.1:9/a", Policy::web(true), T)
            .await
            .unwrap();
        let b = pin("http://127.0.0.1:9/b", Policy::web(true), T)
            .await
            .unwrap();
        let key = |port: u16| {
            (
                "127.0.0.1".to_string(),
                vec![SocketAddr::new(ip("127.0.0.1"), port)],
                T,
            )
        };
        assert!(CLIENTS.get(&key(9), Instant::now()).is_some());
        assert!(CLIENTS.get(&key(10), Instant::now()).is_none());
        assert_eq!((a.url.path(), b.url.path()), ("/a", "/b"));
    }

    #[tokio::test]
    async fn a_redirect_loop_gives_up() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/loop"))
            .respond_with(ResponseTemplate::new(302).insert_header("location", "/loop"))
            .expect((MAX_REDIRECTS + 1) as u64)
            .mount(&server)
            .await;
        let why = get(&format!("{}/loop", server.uri()), Policy::web(true), T, "t")
            .await
            .unwrap_err();
        assert!(why.contains("redirects more than"), "{why}");
    }
}
