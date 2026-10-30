// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Where a pool's backends run, derived from their base URLs: the fact the
//! set-up assistant prefills a pool's level from.

use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::time::Duration;

use aiplane_core::server::net_guard::classify;
use aiplane_core::server::upstreams::Pool;
use rama::http::Uri;

/// All lookups run concurrently, so this bounds the whole answer.
const LOOKUP_BOUND: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hosting {
    OwnNetwork,
    Cloud,
    Unknown,
}

impl Hosting {
    pub fn as_str(self) -> &'static str {
        match self {
            Hosting::OwnNetwork => "own_network",
            Hosting::Cloud => "cloud",
            Hosting::Unknown => "unknown",
        }
    }
}

/// A pool is in the cloud when any backend has a public address, in the own
/// network when every backend resolved and none is public, else unknown.
/// `None` is a backend whose host did not resolve.
pub fn hosting_of(backends: &[Option<Vec<IpAddr>>]) -> Hosting {
    let public = backends
        .iter()
        .flatten()
        .flatten()
        .any(|ip| classify(*ip).is_public());
    if public {
        Hosting::Cloud
    } else if backends.is_empty()
        || backends
            .iter()
            .any(|b| b.as_ref().is_none_or(Vec::is_empty))
    {
        Hosting::Unknown
    } else {
        Hosting::OwnNetwork
    }
}

/// The host and port a base URL points at.
fn host_port(base_url: &str) -> Option<(String, u16)> {
    let uri: Uri = base_url.trim().parse().ok()?;
    let host = uri.host()?.trim_start_matches('[').trim_end_matches(']');
    let port = uri
        .port_u16()
        .unwrap_or(if uri.scheme_str() == Some("http") {
            80
        } else {
            443
        });
    Some((host.to_owned(), port))
}

async fn addresses(host: String, port: u16) -> Option<Vec<IpAddr>> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Some(vec![ip]);
    }
    let found = tokio::time::timeout(LOOKUP_BOUND, tokio::net::lookup_host((host.as_str(), port)))
        .await
        .ok()?
        .ok()?;
    Some(found.map(|a| a.ip()).collect())
}

/// Hosting and backend hosts for every pool, by pool name.
pub async fn of_pools(pools: &[std::sync::Arc<Pool>]) -> HashMap<String, (Hosting, Vec<String>)> {
    let hosts: HashSet<(String, u16)> = pools
        .iter()
        .flat_map(|p| p.backends.iter())
        .filter_map(|b| host_port(&b.base_url))
        .collect();
    let lookups: Vec<_> = hosts
        .into_iter()
        .map(|(host, port)| {
            tokio::spawn(async move {
                let found = addresses(host.clone(), port).await;
                ((host, port), found)
            })
        })
        .collect();
    let mut resolved = HashMap::new();
    for lookup in lookups {
        if let Ok((key, found)) = lookup.await {
            resolved.insert(key, found);
        }
    }
    pools
        .iter()
        .map(|pool| {
            let mut names = Vec::new();
            let per_backend: Vec<_> = pool
                .backends
                .iter()
                .map(|b| {
                    let key = host_port(&b.base_url)?;
                    if !names.contains(&key.0) {
                        names.push(key.0.clone());
                    }
                    resolved.get(&key).cloned().flatten()
                })
                .collect();
            (pool.name.clone(), (hosting_of(&per_backend), names))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ips(list: &[&str]) -> Option<Vec<IpAddr>> {
        Some(list.iter().map(|s| s.parse().unwrap()).collect())
    }

    #[test]
    fn private_and_loopback_backends_are_the_own_network() {
        assert_eq!(
            hosting_of(&[ips(&["10.0.0.5"]), ips(&["127.0.0.1", "fd00::1"])]),
            Hosting::OwnNetwork
        );
    }

    #[test]
    fn one_public_address_makes_a_pool_cloud() {
        assert_eq!(
            hosting_of(&[ips(&["10.0.0.5"]), ips(&["10.0.0.6", "93.184.216.34"])]),
            Hosting::Cloud
        );
    }

    #[test]
    fn a_backend_that_does_not_resolve_leaves_the_pool_unknown() {
        assert_eq!(hosting_of(&[ips(&["10.0.0.5"]), None]), Hosting::Unknown);
        assert_eq!(hosting_of(&[]), Hosting::Unknown);
    }

    #[test]
    fn a_public_address_wins_over_an_unresolved_backend() {
        assert_eq!(hosting_of(&[None, ips(&["8.8.8.8"])]), Hosting::Cloud);
    }

    #[test]
    fn host_and_port_come_from_the_base_url() {
        assert_eq!(
            host_port("https://api.z.ai/v1"),
            Some(("api.z.ai".into(), 443))
        );
        assert_eq!(
            host_port("http://[::1]:8000/v1"),
            Some(("::1".into(), 8000))
        );
        assert_eq!(
            host_port("http://llm01.int:9000"),
            Some(("llm01.int".into(), 9000))
        );
        assert_eq!(host_port("not a url"), None);
    }
}
