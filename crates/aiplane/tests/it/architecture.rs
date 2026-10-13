// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Architecture tests: the cross-cutting invariants a feature test does not
//! see, checked by scanning the workspace's production source.
//!
//! Each scan is a tripwire, not a proof: it looks for the call shape that
//! breaks the invariant and fails when a file outside the rule's allow-list
//! uses it. Every allow-list entry names the file and why it may; an entry
//! that no longer matches anything fails too, so the lists cannot rot into
//! blanket exemptions. How to add one: `docs/testing.md` → "Architecture
//! tests".
//!
//! Comments and `#[cfg(test)]` items are blanked before matching, and so are
//! string literal contents except where a rule matches a literal on purpose
//! (the spec keys), so prose about a forbidden call never trips a rule.
//! Test code — `tests/` directories, `tests.rs`, `#[cfg(test)]` items,
//! `examples/` — is out of scope: it talks to in-process mocks.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// One production `.rs` file, masked twice over the same byte offsets.
struct Source {
    /// Relative to `crates/`, with `/` separators.
    rel: String,
    /// Comments, test items and string literal contents blanked.
    code: String,
    /// Comments and test items blanked; string literals kept.
    text: String,
}

/// A rule's exception: a file (or a directory, with a trailing `/`) and why.
struct Allowed {
    path: &'static str,
    why: &'static str,
}

fn crates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate lives under crates/")
        .to_path_buf()
}

fn is_test_or_example(rel: &str) -> bool {
    rel.split('/')
        .any(|part| matches!(part, "tests" | "examples" | "benches" | "target"))
        || rel.ends_with("/tests.rs")
}

fn production_sources() -> &'static [Source] {
    static SOURCES: OnceLock<Vec<Source>> = OnceLock::new();
    SOURCES.get_or_init(|| {
        let root = crates_dir();
        let mut out = Vec::new();
        let mut stack = vec![root.clone()];
        while let Some(dir) = stack.pop() {
            for entry in fs::read_dir(&dir).expect("readable directory").flatten() {
                let path = entry.path();
                let rel = path
                    .strip_prefix(&root)
                    .expect("under crates/")
                    .to_string_lossy()
                    .replace('\\', "/");
                if is_test_or_example(&rel) {
                    continue;
                }
                if path.is_dir() {
                    stack.push(path);
                } else if rel.ends_with(".rs") {
                    let src = fs::read_to_string(&path).expect("readable source");
                    let (code, text) = mask(&src);
                    out.push(Source { rel, code, text });
                }
            }
        }
        out.sort_by(|a, b| a.rel.cmp(&b.rel));
        out
    })
}

fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn blank(buf: &mut [u8], range: std::ops::Range<usize>) {
    for b in &mut buf[range] {
        if *b != b'\n' {
            *b = b' ';
        }
    }
}

/// `(code, text)` for `src`, both the same length as `src` with newlines
/// kept, so an offset or line number in either is one in the file.
fn mask(src: &str) -> (String, String) {
    let bytes = src.as_bytes();
    let mut code = bytes.to_vec();
    let mut text = bytes.to_vec();
    let n = bytes.len();
    let mut i = 0;
    while i < n {
        let rest = &bytes[i..];
        if rest.starts_with(b"//") {
            let end = rest.iter().position(|&b| b == b'\n').map_or(n, |p| i + p);
            blank(&mut code, i..end);
            blank(&mut text, i..end);
            i = end;
        } else if rest.starts_with(b"/*") {
            let mut depth = 0;
            let mut j = i;
            while j < n {
                if bytes[j..].starts_with(b"/*") {
                    depth += 1;
                    j += 2;
                } else if bytes[j..].starts_with(b"*/") {
                    depth -= 1;
                    j += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    j += 1;
                }
            }
            blank(&mut code, i..j);
            blank(&mut text, i..j);
            i = j;
        } else if let Some((open, hashes)) = raw_string_start(bytes, i) {
            let mut close = vec![b'"'];
            close.extend(std::iter::repeat_n(b'#', hashes));
            let body = i + open;
            let end = bytes[body..]
                .windows(close.len())
                .position(|w| w == close.as_slice())
                .map_or(n, |p| body + p);
            blank(&mut code, body..end);
            i = (end + close.len()).min(n);
        } else if bytes[i] == b'"' {
            let mut j = i + 1;
            while j < n && bytes[j] != b'"' {
                j += if bytes[j] == b'\\' { 2 } else { 1 };
            }
            blank(&mut code, i + 1..j.min(n));
            i = j + 1;
        } else if bytes[i] == b'\'' {
            i = skip_char_literal(bytes, i, &mut code);
        } else {
            i += 1;
        }
    }
    for range in test_items(&code) {
        blank(&mut code, range.clone());
        blank(&mut text, range);
    }
    // Every blanked range covers whole characters, so both stay valid UTF-8.
    (
        String::from_utf8_lossy(&code).into_owned(),
        String::from_utf8_lossy(&text).into_owned(),
    )
}

/// `r"`, `r#"`, `br"`, `br#"` at `i` (not inside an identifier): the length
/// of the opener and the number of `#`.
fn raw_string_start(bytes: &[u8], i: usize) -> Option<(usize, usize)> {
    if i > 0 && is_ident(bytes[i - 1]) {
        return None;
    }
    let mut j = i;
    if bytes.get(j) == Some(&b'b') {
        j += 1;
    }
    if bytes.get(j) != Some(&b'r') {
        return None;
    }
    j += 1;
    let hashes = bytes[j..].iter().take_while(|&&b| b == b'#').count();
    j += hashes;
    (bytes.get(j) == Some(&b'"')).then_some((j + 1 - i, hashes))
}

/// Past a char literal at `i`, blanking it in `code`; past just the quote
/// when it opens a lifetime.
fn skip_char_literal(bytes: &[u8], i: usize, code: &mut [u8]) -> usize {
    if bytes.get(i + 1) == Some(&b'\\') {
        let end = bytes[i + 2..]
            .iter()
            .position(|&b| b == b'\'')
            .map_or(bytes.len(), |p| i + 2 + p);
        blank(code, i + 1..end);
        return end + 1;
    }
    let width = match bytes.get(i + 1) {
        Some(b) if *b < 0x80 => 1,
        Some(b) if *b >= 0xF0 => 4,
        Some(b) if *b >= 0xE0 => 3,
        Some(_) => 2,
        None => return i + 1,
    };
    if bytes.get(i + 1 + width) == Some(&b'\'') {
        blank(code, i + 1..i + 1 + width);
        i + 2 + width
    } else {
        i + 1
    }
}

/// The byte ranges of every `#[cfg(test)]` item in masked `code`.
fn test_items(bytes: &[u8]) -> Vec<std::ops::Range<usize>> {
    const ATTR: &[u8] = b"#[cfg(test)]";
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(p) = bytes[from..].windows(ATTR.len()).position(|w| w == ATTR) {
        let start = from + p;
        let mut j = start + ATTR.len();
        while j < bytes.len() && bytes[j] != b'{' && bytes[j] != b';' {
            j += 1;
        }
        let end = if j < bytes.len() && bytes[j] == b'{' {
            let mut depth = 0usize;
            let mut k = j;
            loop {
                match bytes.get(k) {
                    Some(b'{') => depth += 1,
                    Some(b'}') => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    None => break,
                    _ => {}
                }
                k += 1;
            }
            (k + 1).min(bytes.len())
        } else {
            (j + 1).min(bytes.len())
        };
        out.push(start..end);
        from = end;
    }
    out
}

fn line_of(s: &str, offset: usize) -> usize {
    s[..offset].bytes().filter(|&b| b == b'\n').count() + 1
}

/// Every offset in `hay` where `needle` starts.
fn offsets(hay: &str, needle: &str) -> Vec<usize> {
    hay.match_indices(needle).map(|(i, _)| i).collect()
}

fn allowed<'a>(list: &'a [Allowed], rel: &str) -> Option<&'a Allowed> {
    list.iter().find(|a| {
        if a.path.ends_with('/') {
            rel.starts_with(a.path)
        } else {
            rel == a.path
        }
    })
}

/// Fails with `rule` when `hits` has a file outside `list`, or `list` has an
/// entry no hit needs any more.
fn assert_within(rule: &str, list: &[Allowed], hits: &[(String, usize, String)]) {
    let violations: Vec<String> = hits
        .iter()
        .filter(|(rel, _, _)| allowed(list, rel).is_none())
        .map(|(rel, line, what)| format!("crates/{rel}:{line}: {what}"))
        .collect();
    let used: BTreeSet<&str> = hits
        .iter()
        .filter_map(|(rel, _, _)| allowed(list, rel).map(|a| a.path))
        .collect();
    let stale: Vec<String> = list
        .iter()
        .filter(|a| !used.contains(a.path))
        .map(|a| format!("{} ({})", a.path, a.why))
        .collect();
    assert!(
        violations.is_empty(),
        "{rule}\n\nviolations:\n  {}\n\nRoute the call through the shared mechanism, or — if this \
         file genuinely needs the exception — add it to the rule's allow-list in \
         crates/aiplane/tests/it/architecture.rs with the reason (docs/testing.md → \
         \"Architecture tests\").",
        violations.join("\n  ")
    );
    assert!(
        stale.is_empty(),
        "{rule}\n\nthese allow-list entries no longer match anything; remove them so the \
         exception cannot be reused silently:\n  {}",
        stale.join("\n  ")
    );
}

/// `(file, line, matched)` for every needle in the chosen view.
fn scan(needles: &[&str], view: fn(&Source) -> &str) -> Vec<(String, usize, String)> {
    let mut hits = Vec::new();
    for src in production_sources() {
        let hay = view(src);
        for needle in needles {
            for at in offsets(hay, needle) {
                hits.push((src.rel.clone(), line_of(hay, at), (*needle).to_string()));
            }
        }
    }
    hits
}

// ---------------------------------------------------------------------------
// Outbound HTTP clients.

/// Where a reqwest client may be built. Everything else takes one of these
/// (`AppState::http`, a provider context, a pinned client) as an argument.
/// Every destination the operator does not configure — a user's, a model's,
/// an agent owner's, a browser's push endpoint, the MCP OAuth endpoints —
/// goes through the one guarded site, `outbound_guard`, with a policy of its
/// own; no other guarded client exists. The MCP transport is not listed
/// because rmcp builds its own client inside the crate; its destinations are
/// the admin-curated connector catalog.
const OUTBOUND_CLIENTS: &[Allowed] = &[
    Allowed {
        path: "aiplane-core/src/server/outbound_guard.rs",
        why: "guarded: resolve-and-pin with net_guard, every redirect hop re-checked — the one \
              client for destinations a user, a model or an agent's owner chooses",
    },
    Allowed {
        path: "aiplane-runtime/src/server/state.rs",
        why: "operator-configured: AppState's shared client for the upstream pools, \
              embeddings, rerank and image generation backends",
    },
    Allowed {
        path: "aiplane-core/src/server/upstreams/health.rs",
        why: "operator-configured: health probes of the upstream pools",
    },
    Allowed {
        path: "aiplane-core/src/server/auth/oidc.rs",
        why: "operator-configured: the OIDC provider",
    },
    Allowed {
        path: "aiplane-runtime/src/server/tools/sandbox/mod.rs",
        why: "operator-configured: the sandbox runner",
    },
    Allowed {
        path: "aiplane-features/src/server/comfyui/client.rs",
        why: "operator-configured: the ComfyUI server",
    },
    Allowed {
        path: "aiplane-tools/src/search_web.rs",
        why: "operator-configured: the search provider (SearXNG URL or a fixed API host)",
    },
    Allowed {
        path: "aiplane-features/src/server/geoip/update.rs",
        why: "fixed host: the IP2Location download",
    },
    Allowed {
        path: "aiplane-tools/src/wikipedia.rs",
        why: "fixed host: `<lang>.wikipedia.org`, `lang` validated as a short alphanumeric code",
    },
    Allowed {
        path: "aiplane-tools/src/currency.rs",
        why: "fixed host: api.frankfurter.app",
    },
];

/// The names `code` can build a reqwest client by: the full paths, plus
/// whatever a `use reqwest::…` brings `Client` / `ClientBuilder` in as.
fn reqwest_client_names(code: &str) -> Vec<String> {
    let mut names = vec![
        "reqwest::Client".to_string(),
        "reqwest::ClientBuilder".to_string(),
    ];
    for at in offsets(code, "use reqwest::") {
        let stmt = &code[at + "use reqwest::".len()..];
        let stmt = &stmt[..stmt.find(';').unwrap_or(stmt.len())];
        for item in stmt.split([',', '{', '}']) {
            let mut words = item.split_whitespace();
            let (Some(name), alias) = (words.next(), words.nth(1)) else {
                continue;
            };
            if matches!(name, "Client" | "ClientBuilder") {
                names.push(alias.unwrap_or(name).to_string());
            }
        }
    }
    names
}

#[test]
fn outbound_http_clients_are_built_only_at_the_vetted_sites() {
    let mut hits = Vec::new();
    for src in production_sources() {
        let code = src.code.as_str();
        let mut needles = vec!["reqwest::get(".to_string()];
        for name in reqwest_client_names(code) {
            needles.push(format!("{name}::new("));
            needles.push(format!("{name}::builder("));
        }
        for needle in &needles {
            for at in offsets(code, needle) {
                // `SandboxClient::new(` or `comfyui::Client::new(` is another
                // type that happens to end in an imported name.
                let bare = !needle.starts_with("reqwest::");
                let longer_path = at > 0 && {
                    let before = code.as_bytes()[at - 1];
                    is_ident(before) || before == b':'
                };
                if bare && longer_path {
                    continue;
                }
                hits.push((src.rel.clone(), line_of(code, at), needle.clone()));
            }
        }
    }
    assert_within(
        "A reqwest client is built outside the vetted sites. A new client is a new place \
         the gateway connects from: a destination a user, a model or an agent's owner can \
         choose must go through the net_guard-checked, pinned client \
         (aiplane-core server/outbound_guard.rs), and one only the operator configures \
         should reuse AppState::http.",
        OUTBOUND_CLIENTS,
        &hits,
    );
}

// ---------------------------------------------------------------------------
// Unbounded request body reads.

/// How a file may read an inbound request body.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BodyRead {
    /// Defines the readers everything else calls: frame by frame, capped.
    Reader,
    /// A capped reader of its own, outside the crate stack.
    OwnCap,
    /// Drains a body whole (`read_body_to_bytes`, `read_json`), bounded by
    /// the `BodyLimitLayer` its route group is registered under.
    BehindLimit,
    /// Reads the fields of a multipart upload whole, behind `BodyLimitLayer`.
    Multipart,
}

/// Every file that reads a request body, once, with how. Each rule below
/// takes the kinds it allows from this one list.
const REQUEST_BODY_READS: &[(BodyRead, Allowed)] = &[
    (
        BodyRead::Reader,
        Allowed {
            path: "session-core/src/chrome.rs",
            why: "the readers: read_body_capped, read_body_prefix, and read_body_to_bytes (for \
                  routes behind BodyLimitLayer)",
        },
    ),
    (
        BodyRead::OwnCap,
        Allowed {
            path: "sandbox-runner/src/server.rs",
            why: "the runner's own capped /run reader (413 past MAX_RUN_REQUEST_BYTES): it \
                  stands outside the crate stack and cannot use session_core::chrome",
        },
    ),
    (
        BodyRead::OwnCap,
        Allowed {
            path: "sandbox-runner/src/server/workspace_tests.rs",
            why: "test helper draining an in-process response while asserting the \
                  MAX_RUN_REQUEST_BYTES cap itself — test code, not a production reader",
        },
    ),
    (
        BodyRead::BehindLimit,
        Allowed {
            path: "aiplane/src/rama_server/",
            why: "routed through BodyLimitLayer",
        },
    ),
    (
        BodyRead::BehindLimit,
        Allowed {
            path: "aiplane-api/src/",
            why: "routed through BodyLimitLayer",
        },
    ),
    (
        BodyRead::Multipart,
        Allowed {
            path: "aiplane-api/src/pages/chat/mod.rs",
            why: "the fields of an inbound chat upload, behind BodyLimitLayer — not a response",
        },
    ),
    (
        BodyRead::Multipart,
        Allowed {
            path: "aiplane-api/src/pages/json_skills.rs",
            why: "the field of an inbound skill-archive upload, behind BodyLimitLayer — not a \
                  response",
        },
    ),
    (
        BodyRead::Multipart,
        Allowed {
            path: "aiplane/src/rama_server/multipart.rs",
            why: "the fields of an inbound /v1 multipart upload, behind BodyLimitLayer — not a \
                  response",
        },
    ),
];

fn body_reads(kinds: &[BodyRead]) -> Vec<Allowed> {
    REQUEST_BODY_READS
        .iter()
        .filter(|(kind, _)| kinds.contains(kind))
        .map(|(_, a)| Allowed {
            path: a.path,
            why: a.why,
        })
        .collect()
}

const ROUTER: &str = "aiplane/src/rama_server/router.rs";

/// The handlers `router.rs` registers under
/// `endpoint(BodyLimitLayer::HANDLER_CAPPED)`, as the files that define them
/// — a directory for a `mod.rs`, whose submodules serve it. Read from the
/// router itself, so a route added to that group is checked without a list
/// to keep in step.
fn handler_capped_modules() -> BTreeSet<String> {
    let sources = production_sources();
    let router = sources
        .iter()
        .find(|s| s.rel == ROUTER)
        .expect("the router is a production source");
    let mut handlers = Vec::new();
    for group in router.text.split(".with_endpoint_layer(").skip(1) {
        if !group.starts_with("endpoint(BodyLimitLayer::HANDLER_CAPPED)") {
            continue;
        }
        for route in group.split(".with_").skip(1) {
            let Some((_, handler)) = route.split_once(',') else {
                continue;
            };
            let handler: String = handler
                .trim_start()
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == ':')
                .collect();
            handlers.push(handler);
        }
    }
    assert!(
        !handlers.is_empty(),
        "found no route under endpoint(BodyLimitLayer::HANDLER_CAPPED) in crates/{ROUTER}; \
         the scan below would check nothing"
    );
    let mut modules = BTreeSet::new();
    for handler in handlers {
        let (path, name) = handler.rsplit_once("::").unwrap_or(("", &handler));
        let under = format!("aiplane-api/src/{}", path.replace("::", "/"));
        let defines = format!("fn {name}(");
        let found: Vec<&Source> = sources
            .iter()
            .filter(|s| s.rel.starts_with(&under) && s.code.contains(&defines))
            .collect();
        assert!(
            !found.is_empty(),
            "cannot find where `{handler}`, a handler-capped route's handler in \
             crates/{ROUTER}, is defined (looked for `{defines}` under crates/{under})"
        );
        for s in found {
            modules.insert(match s.rel.strip_suffix("mod.rs") {
                Some(dir) => dir.to_string(),
                None => s.rel.clone(),
            });
        }
    }
    modules
}

#[test]
fn request_bodies_are_read_only_through_the_capped_readers() {
    let hits = scan(
        &[
            "BodyExt",
            "http_body_util",
            ".try_into_json(",
            ".try_into_string(",
            ".into_data_stream(",
        ],
        |s| s.code.as_str(),
    );
    assert_within(
        "A body is read outside session_core::chrome. Reading frames or collecting a body \
         by hand bypasses the cap; call read_body_capped (or read_body_prefix), or \
         read_body_to_bytes in a handler behind BodyLimitLayer.",
        &body_reads(&[BodyRead::Reader, BodyRead::OwnCap]),
        &hits,
    );

    let drains = scan(&["read_body_to_bytes(", "read_json("], |s| s.code.as_str());
    assert_within(
        "read_body_to_bytes / read_json drains a body without a cap of its own, which is \
         only bounded behind a route group's BodyLimitLayer.",
        &body_reads(&[BodyRead::Reader, BodyRead::BehindLimit]),
        &drains,
    );

    let modules = handler_capped_modules();
    let uncapped: Vec<String> = drains
        .iter()
        .filter(|(rel, _, _)| {
            modules.iter().any(|m| {
                if m.ends_with('/') {
                    rel.starts_with(m.as_str())
                } else {
                    rel == m
                }
            })
        })
        .map(|(rel, line, what)| format!("crates/{rel}:{line}: {what}"))
        .collect();
    assert!(
        uncapped.is_empty(),
        "these handlers are registered under BodyLimitLayer::HANDLER_CAPPED, which passes \
         their body through uncapped, so their whole-body read has no bound at all; read \
         through read_body_capped / read_json_capped / read_body_prefix:\n  {}",
        uncapped.join("\n  ")
    );
}

// ---------------------------------------------------------------------------
// Unbounded response body reads.

/// Where a body may be read whole with `.bytes()` / `.text()` / `.json()`:
/// only an inbound multipart upload's fields (`REQUEST_BODY_READS`).
/// Outbound responses never are: they go through `capped_read`.
fn whole_body_reads() -> Vec<Allowed> {
    body_reads(&[BodyRead::Multipart])
}

/// Offsets of `.bytes()`, `.text()`, `.json()` and `.json::<…>()` calls that
/// are awaited: reqwest's whole-body reads (and multipart's). `str::bytes()`
/// and friends are never awaited, so they do not match.
fn awaited_whole_body_reads(code: &str) -> Vec<(usize, &'static str)> {
    let mut out = Vec::new();
    for (needle, name) in [
        (".bytes()", ".bytes()"),
        (".text()", ".text()"),
        (".json()", ".json()"),
        (".json::<", ".json::<…>()"),
    ] {
        for at in offsets(code, needle) {
            let mut end = at + needle.len();
            if needle.ends_with('<') {
                let mut depth = 1;
                let bytes = code.as_bytes();
                while end < bytes.len() && depth > 0 {
                    match bytes[end] {
                        b'<' => depth += 1,
                        b'>' => depth -= 1,
                        _ => {}
                    }
                    end += 1;
                }
                if !code[end..].starts_with("()") {
                    continue;
                }
                end += 2;
            }
            if code[end..].trim_start().starts_with(".await") {
                out.push((at, name));
            }
        }
    }
    out
}

#[test]
fn response_bodies_are_read_only_through_the_capped_reader() {
    let mut hits = Vec::new();
    for src in production_sources() {
        for (at, name) in awaited_whole_body_reads(&src.code) {
            hits.push((src.rel.clone(), line_of(&src.code, at), name.to_string()));
        }
    }
    assert_within(
        "A response body is read whole. `.bytes()` / `.text()` / `.json()` buffer whatever \
         the peer sends; read through aiplane_core::server::capped_read (read_capped, \
         read_capped_for, read_capped_json, read_capped_text, read_error_text) with a cap that fits the use.",
        &whole_body_reads(),
        &hits,
    );
}

#[test]
fn the_whole_body_scan_matches_awaited_reads_only() {
    let code = "let a = resp.bytes().await?; let b = s.bytes().count(); \
                let c = r.json::<Vec<u8>>()\n    .await; let d = r.text().await;";
    let names: Vec<&str> = awaited_whole_body_reads(code)
        .into_iter()
        .map(|(_, n)| n)
        .collect();
    assert_eq!(names, [".bytes()", ".text()", ".json::<…>()"]);
}

// ---------------------------------------------------------------------------
// Agent spec JSON.

/// Where an agent spec's JSON may be read by key: the validator walk, and
/// the few places that must keep JSON (docs/agent-spec.md → "The typed spec").
const SPEC_JSON_READERS: &[Allowed] = &[
    Allowed {
        path: "aiplane-runtime/src/agents/spec.rs",
        why: "the validator: the path-reporting walk over the JSON before it is typed",
    },
    Allowed {
        path: "aiplane-runtime/src/agents/spec/",
        why: "the validator's per-kind walks, and `secrets`, which seals a credential in the \
              stored JSON before it is saved (a typed round trip would re-serialize the spec)",
    },
    Allowed {
        path: "aiplane-runtime/src/agents/state.rs",
        why: "StateSchema compiles `state` from the same JSON the walk checks",
    },
    Allowed {
        path: "aiplane-runtime/src/agents/a2a_client/mod.rs",
        why: "the A2A task answer has a `state` of its own, not a spec's",
    },
    Allowed {
        path: "aiplane-runtime/src/agents/assist/",
        why: "the prompt assistant builds spec fragments onto a draft that need not be valid \
              yet, and hands every result to the validator; it never runs a spec",
    },
    Allowed {
        path: "aiplane-api/src/pages/architect/dot.rs",
        why: "the personal proposal builder writes reviewed permissions into draft JSON before \
              AgentSpec validation, like assist/; its task, tools, defaults and bindings are \
              read through typed AgentSpec fields, and it never executes a spec",
    },
    Allowed {
        path: "aiplane-agents/src/db/agent_analytics.rs",
        why: "an audit row's `routes` detail, not a spec",
    },
];

/// The spec's top-level keys and the keys of its parts whose names give a
/// raw read away.
const SPEC_KEYS: &[&str] = &[
    "profile",
    "main",
    "state",
    "verifiers",
    "router",
    "routes",
    "finish",
    "publish",
    "tool_resources",
    "bind",
    "when",
];

#[test]
fn agent_spec_json_is_read_only_by_the_validator() {
    let mut needles = Vec::new();
    for key in SPEC_KEYS {
        for accessor in [".get(", ".get_mut(", ".remove("] {
            needles.push(format!("{accessor}\"{key}\")"));
        }
        for accessor in [".pointer(", ".pointer_mut("] {
            needles.push(format!("{accessor}\"/{key}\""));
            needles.push(format!("{accessor}\"/{key}/"));
        }
        needles.push(format!("[\"{key}\"]"));
    }
    let needles: Vec<&str> = needles.iter().map(String::as_str).collect();
    let hits = scan(&needles, |s| s.text.as_str());
    assert_within(
        "An agent spec is read as raw JSON outside the validator. Runtime code reads the \
         typed AgentSpec (agents::spec::model) that the validator produced, from \
         CompiledSpec::agent(); a default belongs in the type's accessor, not at the read.",
        SPEC_JSON_READERS,
        &hits,
    );
}

// ---------------------------------------------------------------------------
// Model tool dispatch.

/// Who may hand a round's calls to the runner, and the grant-narrowed
/// source each one must name so the calls resolve through it.
const DISPATCHERS: &[(Allowed, Option<&str>)] = &[
    (
        Allowed {
            path: "aiplane-runtime/src/server/tools/runner.rs",
            why: "the runner itself: run_with_tools hands its rounds to the guarded executor",
        },
        None,
    ),
    (
        Allowed {
            path: "aiplane-runtime/src/openai_driver.rs",
            why: "the chat and agent driver: GrantedToolSource, with an agent run's \
                  RunToolSource layered over it",
        },
        Some("GrantedToolSource"),
    ),
    (
        Allowed {
            path: "aiplane-runtime/src/openai_driver/resume.rs",
            why: "resumes a call with the source the driver built (GrantedToolSource)",
        },
        None,
    ),
    (
        Allowed {
            path: "aiplane/src/rama_server/proxy.rs",
            why: "the /v1 loops: DiscoverableToolSource over the token's RBAC-bounded layer",
        },
        Some("DiscoverableToolSource"),
    ),
];

/// Who may call `Tool::run` directly rather than through the runner.
const DIRECT_TOOL_RUNS: &[Allowed] = &[
    Allowed {
        path: "aiplane-runtime/src/agents/profile.rs",
        why: "the organization wrapper revalidates the initially granted tool's exact private turn, current principal grants and selected account before inner dispatch",
    },
    Allowed {
        path: "aiplane-runtime/src/agents/approval.rs",
        why: "the shared permission gate classifies a granted invocation before allowing, denying or suspending its original call",
    },
    Allowed {
        path: "aiplane-runtime/src/persona/live.rs",
        why: "a personal Dot wrapper reloads owner grants, active selection and approval rules before dispatch through GrantedToolSource",
    },
    Allowed {
        path: "aiplane-runtime/src/server/tools/runner.rs",
        why: "the runner, after resolving the call through the round's source",
    },
    Allowed {
        path: "aiplane-runtime/src/agents/bind.rs",
        why: "a bound tool wraps a granted tool and only narrows its arguments",
    },
    Allowed {
        path: "aiplane-runtime/src/server/tools/ask_first.rs",
        why: "AskFirst wraps a granted tool and runs it once the user approved",
    },
    Allowed {
        path: "aiplane-runtime/src/server/tools/mcp/manager.rs",
        why: "the MCP overlay's wrappers around a connector's tools",
    },
    Allowed {
        path: "aiplane-runtime/src/agents/verifier/lookup.rs",
        why: "a lookup verifier runs its tool only after checking the principal's grant",
    },
    Allowed {
        path: "aiplane-runtime/src/agents/verifier/mod.rs",
        why: "a connector verifier resolves through the principal's grant-bounded MCP layer",
    },
];

#[test]
fn action_rules_classify_and_validate_before_using_shared_approvals() {
    let sources = production_sources();
    for (path, approval) in [
        ("aiplane-runtime/src/agents/approval.rs", "AskFirst::wrap"),
        (
            "aiplane-runtime/src/server/tools/ask_first.rs",
            "match approval(",
        ),
    ] {
        let source = sources
            .iter()
            .find(|source| source.rel == path)
            .expect("the shared permission and approval implementations must exist");
        let run = source
            .code
            .split_once("fn run")
            .expect("the policy implements Tool::run")
            .1;
        let classifier = run
            .find(".classify_actions(")
            .expect("permission and approval gates must use the tool's classifier");
        let validation = run
            .find("validate_actions(")
            .expect("permission and approval gates must share action-fact validation");
        let approval = run
            .find(approval)
            .expect("action rules must use the existing approval mechanism");
        assert!(
            classifier < validation && validation < approval,
            "{path} must classify and validate an invocation before using its approval"
        );
    }
}

#[test]
fn agent_resume_claims_use_one_request_aware_transaction() {
    let source = production_sources()
        .iter()
        .find(|source| source.rel == "aiplane-runtime/src/agents/resume.rs")
        .expect("the shared agent resume implementation must exist");
    assert!(source.code.contains("WriteTx::begin("));
    assert!(source.code.contains("suspend::claim_chain_in("));
    assert!(source.code.contains(".commit()"));
    assert!(!source.code.contains("claim_for_resume("));
    assert!(!source.code.contains("chat::claim_suspension("));
    let shared = production_sources()
        .iter()
        .find(|source| source.rel == "aiplane-runtime/src/suspend.rs")
        .expect("the shared suspension claims must exist");
    let chain = shared
        .code
        .split("async fn claim_chain_in(")
        .nth(1)
        .expect("the shared chain claim must exist");
    let chain = chain.split("\npub ").next().unwrap();
    assert!(chain.contains("claim_for_resume_in("));
    assert!(chain.contains("claim_suspension_in("));
    assert!(!chain.contains("WriteTx::begin("));
    assert!(!chain.contains(".commit()"));
}

#[test]
fn personal_dot_calls_revalidate_access_before_execution() {
    let source = production_sources()
        .iter()
        .find(|source| source.rel == "aiplane-runtime/src/persona/live.rs")
        .expect("the personal Dot dispatch wrapper must exist");
    for mechanism in [
        "users::find_by_id",
        "agents::get_personal_dot",
        "dot.active",
        "spec.main.tools",
        "GrantedToolSource::new",
        "gate_dot",
    ] {
        assert!(
            source.code.contains(mechanism),
            "personal Dot dispatch must keep {mechanism} before executing a tool"
        );
    }
    let wrapping = scan(&["LivePersonalDotTool {"], |source| source.code.as_str());
    assert_within(
        "Only the grant-filtered personal Dot source may wrap a tool for live dispatch.",
        &[
            Allowed {
                path: "aiplane-runtime/src/persona.rs",
                why: "the persona's selected source wraps an initially granted tool",
            },
            Allowed {
                path: "aiplane-runtime/src/persona/live.rs",
                why: "the wrapper type and its Tool implementation",
            },
        ],
        &wrapping,
    );
}

#[test]
fn organization_dot_calls_revalidate_private_authority_before_execution() {
    let source = production_sources()
        .iter()
        .find(|source| source.rel == "aiplane-runtime/src/agents/profile.rs")
        .expect("the organization Dot dispatch wrapper must exist");
    for mechanism in [
        "validate_organization_turn_in",
        "validate_organization_identity_in",
        "Arc::ptr_eq",
        "pending_execution_facts",
        "dependency_tool_ids",
    ] {
        assert!(
            source.code.contains(mechanism),
            "organization Dot dispatch must keep {mechanism}"
        );
    }
    let implementation = source
        .code
        .split_once("impl Tool for OrganizationTool")
        .expect("the admitted tool implements Tool")
        .1;
    let execute = implementation
        .split_once("fn run")
        .expect("the wrapper executes the tool")
        .1;
    assert!(
        execute.find("self.admit(&ctx)").unwrap() < execute.find("self.inner.run(ctx").unwrap()
    );
    let wrapping = scan(&["OrganizationTool {"], |source| source.code.as_str());
    assert_within(
        "Only the grant-filtered run source may wrap an organization tool for live dispatch.",
        &[Allowed {
            path: "aiplane-runtime/src/agents/profile.rs",
            why: "the run source wraps an initially granted tool and verifies its live private authority",
        }],
        &wrapping,
    );
}

#[test]
fn organization_memory_uses_shared_private_authority_for_tools_and_model_context() {
    let sources = production_sources();
    let memory = sources
        .iter()
        .find(|source| source.rel == "aiplane-runtime/src/server/tools/memory.rs")
        .expect("the organization memory admission mechanism must exist");
    let compact = memory.code.split_whitespace().collect::<String>();
    for mechanism in [
        "validate_organization_turn_in",
        "ctx.principal",
        "ctx.session_id",
        "ctx.assistant_turn_id",
        "ctx.granted_tools",
        "GrantKind::Tool",
        "surface.offered",
    ] {
        assert!(
            compact.contains(mechanism),
            "organization memory must retain {mechanism}"
        );
    }
    let scope_callers = scan(&["organization_scope_in("], |source| source.code.as_str());
    assert_within(
        "Organization memory tools must use the shared private-turn admission mechanism.",
        &[
            Allowed {
                path: "aiplane-runtime/src/server/tools/memory.rs",
                why: "the canonical same-transaction memory admission mechanism",
            },
            Allowed {
                path: "aiplane-tools/src/memory.rs",
                why: "the four actual memory tools admit their scoped transaction before CRUD",
            },
        ],
        &scope_callers,
    );
    let context_callers = scan(&["organization_standing_context("], |source| {
        source.code.as_str()
    });
    assert_within(
        "Organization standing memory must enter the existing agent system-message path.",
        &[
            Allowed {
                path: "aiplane-runtime/src/server/tools/memory.rs",
                why: "the bounded reader uses the same current private-turn admission",
            },
            Allowed {
                path: "aiplane-runtime/src/openai_driver/turn_policy.rs",
                why: "the existing agent leading system message consumes bounded organization notes",
            },
        ],
        &context_callers,
    );
}

#[test]
fn organization_published_admission_uses_the_existing_private_opener_and_source_proof() {
    let sources = production_sources();
    let profile = sources
        .iter()
        .find(|source| source.rel == "aiplane-runtime/src/agents/profile.rs")
        .expect("the organization source proof must exist");
    for mechanism in [
        "ProfilePurpose::Private",
        "OrganizationSpecSource",
        "validate_source_in",
        "agents_db::version_in",
        "authority.principal_id",
        "authority.session_id",
        "authority.assistant_turn_id",
    ] {
        assert!(
            profile.code.contains(mechanism),
            "private published admission must retain {mechanism}"
        );
    }
    let callers = scan(&["load_private("], |source| source.code.as_str());
    assert_within(
        "Only the authenticated private opener may load an organization published profile directly.",
        &[
            Allowed {
                path: "aiplane-runtime/src/agents/profile.rs",
                why: "the common loader defines its explicit private admission purpose",
            },
            Allowed {
                path: "aiplane-runtime/src/agents/run/draft.rs",
                why: "the existing private opener verifies the authenticated member in the opening transaction",
            },
        ],
        &callers,
    );
}

#[test]
fn organization_schedules_and_private_stop_use_shared_admission() {
    assert_within(
        "Only the scheduled worker may admit an organization schedule through its exact recorded source.",
        &[
            Allowed {
                path: "aiplane-runtime/src/agents/profile.rs",
                why: "the shared loader defines exact scheduled-source admission",
            },
            Allowed {
                path: "aiplane-runtime/src/server/scheduled/worker.rs",
                why: "the existing worker opens and attaches the private principal turn in one transaction",
            },
        ],
        &scan(&["load_scheduled("], |source| source.code.as_str()),
    );
    assert_within(
        "Organization webhook admission must bind its exact recorded run in the trigger's transaction.",
        &[
            Allowed {
                path: "aiplane-runtime/src/agents/profile.rs",
                why: "the shared principal loader validates the recorded webhook source",
            },
            Allowed {
                path: "aiplane-api/src/pages/webhooks.rs",
                why: "public firing and human replay share atomic admission and private turn attachment",
            },
        ],
        &scan(&["load_webhook_in("], |source| source.code.as_str()),
    );
    assert_within(
        "Private Stop must use the background structural proof in its mutation transaction.",
        &[
            Allowed {
                path: "aiplane-runtime/src/server/background.rs",
                why: "the existing background matcher owns original run and session proof",
            },
            Allowed {
                path: "aiplane-runtime/src/agents/resume.rs",
                why: "the private cancellation gate validates the binding before turn mutation",
            },
        ],
        &scan(&["validate_turn_in("], |source| source.code.as_str()),
    );
}

/// `.run(<context>, …)`: a `Tool::run` call, told from other `run` methods
/// by its first argument being a tool context.
fn tool_run_calls(src: &Source) -> Vec<usize> {
    let code = src.code.as_str();
    offsets(code, ".run(")
        .into_iter()
        .filter(|&at| {
            let arg = code[at + ".run(".len()..].trim_start();
            let first: String = arg
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            first.ends_with("ctx") || first == "ToolContext"
        })
        .collect()
}

#[test]
fn model_tool_calls_dispatch_only_through_the_grant() {
    let hits = scan(
        &[
            "execute_tool_calls(",
            "execute_tool_calls_guarded(",
            "run_with_tools(",
        ],
        |s| s.code.as_str(),
    );
    let allow: Vec<Allowed> = DISPATCHERS
        .iter()
        .map(|(a, _)| Allowed {
            path: a.path,
            why: a.why,
        })
        .collect();
    assert_within(
        "A round's tool calls are dispatched outside the grant-narrowed loops. A model can \
         name any tool, so every loop resolves calls through GrantedToolSource (chat, agent \
         runs) or DiscoverableToolSource (/v1); see docs/tools-rbac.md.",
        &allow,
        &hits,
    );
    for (a, gate) in DISPATCHERS {
        let Some(gate) = gate else { continue };
        let src = production_sources()
            .iter()
            .find(|s| s.rel == a.path)
            .unwrap_or_else(|| panic!("{} is gone; update DISPATCHERS", a.path));
        assert!(
            src.code.contains(gate),
            "crates/{} dispatches tool calls but no longer builds {gate}, the source that \
             narrows them to the grant",
            a.path
        );
    }

    let mut runs = Vec::new();
    for src in production_sources() {
        for at in tool_run_calls(src) {
            runs.push((src.rel.clone(), line_of(&src.code, at), ".run(ctx…)".into()));
        }
    }
    assert_within(
        "A tool is run directly instead of through the runner. Resolve the call through the \
         round's grant-narrowed source; a wrapper that runs an inner tool belongs in the \
         allow-list with its reason.",
        DIRECT_TOOL_RUNS,
        &runs,
    );
}

// ---------------------------------------------------------------------------
// The scanner itself.

// ---------------------------------------------------------------------------
// The agent activity log.

/// Where SQL may change `agent_audit`: the log's own module, the only
/// writer of its hash chains.
const ACTIVITY_LOG_SQL: &[Allowed] = &[Allowed {
    path: "aiplane-agents/src/db/agent_audit.rs",
    why: "the activity log itself: `append` is the one INSERT, and the retention sweep deletes \
          whole chains",
}];

/// Who may call the log's writers. Each management module records its own
/// changes on their transaction; none of them redacts anything, because
/// `append` does, by event kind.
const ACTIVITY_LOG_WRITERS: &[Allowed] = &[
    Allowed {
        path: "aiplane-agents/src/db/agent_audit.rs",
        why: "the log itself: `append_now`, the anchor, the sweep's markers",
    },
    Allowed {
        path: "aiplane-agents/src/db/agent_state.rs",
        why: "a slot write records `state_written` on the write's transaction",
    },
    Allowed {
        path: "aiplane-agents/src/db/agents.rs",
        why: "agent create, draft, publish, live version, shares and delete",
    },
    Allowed {
        path: "aiplane-agents/src/db/organizations.rs",
        why: "organization metadata and group bindings are audited on their own management transaction",
    },
    Allowed {
        path: "aiplane-agents/src/db/system_principals.rs",
        why: "principal create and disable, grants, tokens",
    },
    Allowed {
        path: "aiplane-agents/src/db/embed_keys.rs",
        why: "embed key create and revoke",
    },
    Allowed {
        path: "aiplane-agents/src/db/agent_channels.rs",
        why: "notification channels created and deleted",
    },
    Allowed {
        path: "aiplane-agents/src/db/dot_sessions.rs",
        why: "private Dot session binding and its event share one write transaction",
    },
    Allowed {
        path: "aiplane-agents/src/db/dot_delegation.rs",
        why: "directed consent and its management audit event share one write transaction",
    },
    Allowed {
        path: "aiplane-runtime/src/agents/audit.rs",
        why: "the runtime's one door for run events: bounded, and it stops a run whose event \
              cannot be written",
    },
];

#[test]
fn activity_events_are_written_only_through_the_activity_log() {
    let sql = scan(
        &[
            "INSERT INTO agent_audit",
            "INSERT OR REPLACE INTO agent_audit",
            "REPLACE INTO agent_audit",
            "UPDATE agent_audit",
            "DELETE FROM agent_audit",
        ],
        |s| s.text.as_str(),
    );
    assert_within(
        "SQL changes agent_audit outside the activity log. Every event is appended by \
         agent_audit::append so it takes its place in a hash chain, and the log is never edited \
         in place; record an event through aiplane_runtime::agents::audit (or \
         agent_audit::append on a management change's transaction) instead.",
        ACTIVITY_LOG_SQL,
        &sql,
    );
    let writers = scan(
        &["agent_audit::append", "append_now", "agent_audit::anchor"],
        |s| s.code.as_str(),
    );
    assert_within(
        "An activity event is written around the runtime's door. A run event goes through \
         aiplane_runtime::agents::audit (record_event, record, ToolContext::audit), which bounds \
         the write and stops a run whose event is lost; only the agent DB modules write on a \
         change's own transaction.",
        ACTIVITY_LOG_WRITERS,
        &writers,
    );
}

#[test]
fn the_mask_blanks_comments_strings_and_test_items_but_keeps_lines() {
    let src = "let a = \"Client::new()\"; // Client::new()\n\
               /* Client::new() */ let c = '\"'; let l: &'static str = r#\"x\"#;\n\
               #[cfg(test)]\nmod tests { fn f() { Client::new(); } }\nClient::new();\n";
    let (code, text) = mask(src);
    assert_eq!(code.len(), src.len());
    assert_eq!(code.lines().count(), src.lines().count());
    assert_eq!(offsets(&code, "Client::new(").len(), 1, "{code}");
    assert_eq!(line_of(&code, offsets(&code, "Client::new(")[0]), 5);
    assert!(text.contains("\"Client::new()\""), "{text}");
    assert!(!text.contains("fn f()"), "{text}");
    assert_eq!(
        reqwest_client_names("use reqwest::{Client as Http, Url};"),
        ["reqwest::Client", "reqwest::ClientBuilder", "Http"]
    );
}

// ---------------------------------------------------------------------------
// Crate direction.

/// The crate stack of AGENTS.md, bottom (0) up. A crate may depend only on
/// crates on a lower level; siblings share one, so neither depends on the
/// other.
const STACK: &[(&str, u8)] = &[
    ("shared", 0),
    ("session-core", 1),
    ("aiplane-core", 2),
    ("aiplane-features", 3),
    ("aiplane-agents", 3),
    ("aiplane-runtime", 4),
    ("aiplane-tools", 5),
    ("aiplane-api", 5),
    ("aiplane", 6),
];

/// Members outside the stack, with the workspace crates each may use.
/// Nothing may depend on them.
const OUTSIDE_THE_STACK: &[(&str, &[&str])] = &[
    // A separate service binary; it shares only the wire types.
    ("sandbox-runner", &["shared"]),
];

/// The workspace members, and `(crate, dependency, kind)` for every
/// dependency between them, from `cargo metadata`, so renamed,
/// target-specific and workspace-inherited dependencies all count.
fn workspace_edges() -> (Vec<String>, Vec<(String, String, String)>) {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| env!("CARGO").to_string());
    let workspace = crates_dir()
        .parent()
        .expect("crates/ lives in the workspace root")
        .to_path_buf();
    let out = std::process::Command::new(cargo)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--offline",
        ])
        .current_dir(&workspace)
        .output()
        .expect("cargo metadata runs");
    assert!(
        out.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let meta: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("cargo metadata prints JSON");
    let packages = meta["packages"].as_array().expect("packages");
    let members: Vec<String> = packages
        .iter()
        .filter_map(|p| p["name"].as_str().map(str::to_string))
        .collect();
    let mut edges = Vec::new();
    for p in packages {
        let from = p["name"].as_str().expect("a package has a name");
        for d in p["dependencies"].as_array().into_iter().flatten() {
            let to = d["name"].as_str().expect("a dependency has a name");
            if !members.iter().any(|m| m == to) {
                continue;
            }
            let kind = d["kind"].as_str().unwrap_or("normal").to_string();
            edges.push((from.to_string(), to.to_string(), kind));
        }
    }
    (members, edges)
}

/// Why `from` may not depend on `to`, or `None` when it may.
fn edge_violation(from: &str, to: &str) -> Option<String> {
    let level = |name: &str| STACK.iter().find(|(n, _)| *n == name).map(|(_, l)| *l);
    if let Some((_, allowed)) = OUTSIDE_THE_STACK.iter().find(|(n, _)| *n == from) {
        return (!allowed.contains(&to))
            .then(|| format!("{from} stands outside the stack and may use only {allowed:?}"));
    }
    if OUTSIDE_THE_STACK.iter().any(|(n, _)| *n == to) {
        return Some(format!(
            "{to} stands outside the stack; nothing depends on it"
        ));
    }
    match (level(from), level(to)) {
        (Some(f), Some(t)) if t < f => None,
        (Some(f), Some(t)) if t == f => Some(format!(
            "{from} and {to} are siblings on level {f}; neither may depend on the other"
        )),
        (Some(f), Some(t)) => Some(format!(
            "{to} (level {t}) sits above {from} (level {f}); a crate may depend only on \
             crates beneath it"
        )),
        _ => Some(format!(
            "{from} -> {to} involves a crate this test does not place in the stack"
        )),
    }
}

#[test]
fn workspace_crates_depend_only_down_the_stack() {
    let (members, edges) = workspace_edges();
    assert!(
        edges
            .iter()
            .any(|(from, to, _)| from == "aiplane-runtime" && to == "aiplane-core"),
        "cargo metadata reported no aiplane-runtime -> aiplane-core edge; the test is not \
         reading the dependency graph it means to check: {edges:?}"
    );
    let unplaced: Vec<&String> = members
        .iter()
        .filter(|m| {
            !STACK.iter().any(|(n, _)| n == m) && !OUTSIDE_THE_STACK.iter().any(|(n, _)| n == m)
        })
        .collect();
    assert!(
        unplaced.is_empty(),
        "these workspace members have no place in the crate stack: {unplaced:?}. Decide \
         where they sit, document it in AGENTS.md → \"The gateway crate stack\" and \
         docs/architecture.md#crate-boundaries, and add them to STACK (or \
         OUTSIDE_THE_STACK) in crates/aiplane/tests/it/architecture.rs"
    );
    let wrong: Vec<String> = edges
        .iter()
        .filter_map(|(from, to, kind)| {
            edge_violation(from, to).map(|why| format!("{from} -> {to} ({kind}): {why}"))
        })
        .collect();
    assert!(
        wrong.is_empty(),
        "dependencies against the crate stack:\n  {}\n\nAn upward or sideways edge collapses \
         a layer, and every build of the lower crate pays for the upper one. Move the code \
         the lower crate needs down, or the caller up; see AGENTS.md → \"The gateway crate \
         stack\" and docs/architecture.md#crate-boundaries.",
        wrong.join("\n  ")
    );
}

#[test]
fn the_stack_rule_refuses_upward_and_sideways_edges() {
    assert_eq!(edge_violation("aiplane-runtime", "aiplane-core"), None);
    assert!(edge_violation("aiplane-core", "aiplane-agents").is_some());
    assert!(edge_violation("aiplane-features", "aiplane-agents").is_some());
    assert!(edge_violation("aiplane-api", "aiplane-tools").is_some());
    assert!(edge_violation("session-core", "aiplane-core").is_some());
    assert!(edge_violation("aiplane", "sandbox-runner").is_some());
    assert_eq!(edge_violation("sandbox-runner", "shared"), None);
    assert!(edge_violation("sandbox-runner", "session-core").is_some());
}
