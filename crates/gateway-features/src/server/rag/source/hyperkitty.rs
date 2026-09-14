// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Mailing-list archives published by HyperKitty, as a RAG source.
//!
//! A list is configured the way a git repository is — paste one URL, add as
//! many as you like — and every *thread* in it becomes one document. The
//! parsing, quote-stripping and threading all live in [`super::mail`]; this
//! module is the I/O half: fetch the archive, turn threads into
//! [`RemoteEntry`]s, hand over rendered bytes.
//!
//! # Why a thread needs the whole archive, and how it is paid for once
//!
//! HyperKitty exports two things: the complete archive
//! (`export/<list>.mbox.gz`) and a date window of it (`?start=…&end=…`,
//! `start` inclusive and `end` exclusive — verified against a live server).
//! Indexing only the window is the obvious daily update and it is wrong on
//! its own, for two reasons:
//!
//!   * **A thread is the document, and threads cross windows.** A question
//!     asked on the 30th and answered on the 2nd would index as two half
//!     conversations, one of them the question with no answer — precisely the
//!     retrieval failure this source exists to avoid.
//!
//!   * **Enumeration must be complete every time.** The indexer reads "absent
//!     from the listing" as "deleted", so a window-only listing would delete
//!     the entire back catalogue on its first run.
//!
//! Both objections are about the *corpus*, not about the *download*. So the
//! corpus is always whole and the download is not: the archive is kept in
//! [`ProviderContext::cache_dir`], and each sync fetches only the days since
//! the last one and appends them. Gzip members concatenate, so "append" is
//! literally that — a new `.mbox.gz` file next to the others, decoded as one
//! stream. Messages are identified by `Message-ID`, which is what makes the
//! deliberate overlap between windows harmless
//! ([`mail::thread_messages`] drops the duplicates).
//!
//! Why bother: the full export is 57 MB of gzip that the server generates on
//! demand, per request, with no `ETag` and no `Last-Modified` to make a
//! conditional GET possible. Pulling that daily — forever, for every list —
//! is load somebody else pays for, and the kind of traffic that earns a block.
//! A day's window is a few kilobytes.
//!
//! The cache is refilled from scratch every [`FULL_REFRESH_AFTER_DAYS`] days.
//! That is not housekeeping: windows only ever *add* mail, so a message an
//! administrator deleted from the archive would otherwise live in our copy
//! forever. It also bounds the segment count, which is why no compaction step
//! exists.
//!
//! An operator who would rather not have the gateway keep the archive on disk
//! turns **Keep a local copy** off, and every sync downloads the whole export
//! as before. Nothing else changes; the corpus is identical either way.
//!
//! Costs, measured on twelve years of `ceph-users`: 57 MB of gzip carrying
//! 187 MB of mail become 7,469 threads in 4.6 s of a *debug* build. The gzip
//! is streamed through the parser, so the 187 MB inside it never exists whole,
//! and [`mail::Thread::version`] means an unchanged thread is re-embedded by
//! nobody — which is where the real money is.
//!
//! # What an operator types
//!
//! One URL: the list's HyperKitty page, e.g.
//! `https://lists.example.com/hyperkitty/list/users@example.com/`. The export
//! URL and the per-message permalinks are derived from it, so there is nothing
//! else to get wrong. Public archives need no credentials, which is why this
//! provider declares none.

use std::collections::BTreeMap;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use flate2::read::MultiGzDecoder;
use jiff::civil::Date;
use jiff::{Span, Timestamp};
use serde::{Deserialize, Serialize};
use tokio::sync::OnceCell;

use super::mail;
use super::{
    ConfigField, DirListing, DirRef, EntryKind, FieldKind, FileProvider, ProbeReport,
    ProviderCapabilities, ProviderConfig, ProviderContext, ProviderError, ProviderFactory,
    RemoteEntry,
};

const KIND: &str = "hyperkitty";

/// Caps on one archive fetch.
///
/// Both are errors rather than silent truncation, which is the whole point:
/// half an archive looks, to the sync planner, exactly like an archive whose
/// other half was deleted. Failing the sync leaves the previous index in
/// place; truncating it would quietly throw away years of mail.
///
/// The numbers are "an order of magnitude past the largest real list" —
/// `ceph-users` is 57 MB compressed and 187 MB expanded after twelve years.
const MAX_COMPRESSED_BYTES: u64 = 512 * 1024 * 1024;
const MAX_DECOMPRESSED_BYTES: u64 = 4 * 1024 * 1024 * 1024;

/// Refill the cache from the full export when it is older than this.
///
/// Windows only add mail. Without a periodic full refill, a message deleted
/// from the archive — a moderator removing a posted password, a GDPR erasure —
/// would stay in our copy and in the index forever. Thirty days also bounds
/// the number of segments, which is why there is no compaction step.
const FULL_REFRESH_AFTER_DAYS: i64 = 30;

/// How far back a window reaches past what is already covered.
///
/// HyperKitty buckets a message by a calendar date in *its* timezone, not
/// ours, and mail can arrive out of order; three days is cheap insurance in a
/// scheme where a duplicate costs nothing. Anything missed would otherwise be
/// invisible until the next full refill.
const WINDOW_OVERLAP_DAYS: i64 = 3;

/// Refill from the full export once the cache has this many segments.
///
/// One sync appends one segment, and the 30-day refill would normally keep
/// that near 30. This is the backstop for the operator who clicks Re-index
/// fifty times in an afternoon: cheap to check, and a refill is the same
/// download those fifty windows were avoiding.
const MAX_SEGMENTS: usize = 64;

/// Where the cache records what it has. Sits next to the `.mbox.gz` segments.
const STATE_FILE: &str = "state.json";

/// How much of the archive a "Test connection" reads. Enough to prove the
/// export exists, is gzip, and parses as mail; small enough that clicking the
/// button is not a 40 MB download.
const PROBE_BYTES: u64 = 1024 * 1024;

static FIELDS: &[ConfigField] = &[
    ConfigField {
        key: "list_url",
        label: "List URL",
        help: "The list's page in HyperKitty, e.g. \
           https://lists.example.com/hyperkitty/list/users@example.com/ — copy it from the \
           browser's address bar. The archive download and the per-message links are derived \
           from it.",
        kind: FieldKind::Url,
        required: true,
        default: None,
    },
    ConfigField {
        key: "cache_archive",
        label: "Keep a local copy",
        help: "Keep the downloaded archive on disk and fetch only new mail on later syncs, instead \
           of downloading the whole export every time. Strongly recommended: the full export is \
           tens of megabytes that the list server generates on demand, and re-pulling it daily \
           is load someone else pays for. Turn it off to trade that for disk.",
        kind: FieldKind::Bool,
        required: false,
        default: Some("true"),
    },
];

pub struct HyperkittyFactory;

impl ProviderFactory for HyperkittyFactory {
    fn kind(&self) -> &'static str {
        KIND
    }

    fn label(&self) -> &'static str {
        "Mailing list (HyperKitty)"
    }

    fn description(&self) -> &'static str {
        "Indexes a public mailing-list archive one conversation at a time: quoted reply chains, \
         signatures and list footers are stripped, and every message keeps its author, date and \
         permalink so an answer can be attributed and followed back to the archive."
    }

    fn config_fields(&self) -> &'static [ConfigField] {
        FIELDS
    }

    fn build(
        &self,
        cfg: &ProviderConfig,
        ctx: &ProviderContext,
    ) -> Result<Arc<dyn FileProvider>, ProviderError> {
        Ok(Arc::new(Hyperkitty::from_config(cfg, ctx)?) as Arc<dyn FileProvider>)
    }
}

/// One rendered thread, ready to hand to the indexer.
struct Doc {
    rel_path: String,
    version: String,
    text: String,
    /// Date of the newest message, so "modified" means what it says.
    modified_at: Option<Timestamp>,
}

/// Every thread in the archive, keyed by thread id (the root `Message-ID`).
type Archive = BTreeMap<String, Doc>;

pub struct Hyperkitty {
    /// List page with no trailing slash — the base for permalinks.
    list_url: String,
    /// Posting address, e.g. `users@example.com`. Also the export's filename.
    list_name: String,
    /// `{list_url}/export/{list_name}.mbox.gz`.
    export_url: String,
    http: reqwest::Client,
    /// Where the archive is kept between syncs. `None` when the operator
    /// turned caching off, or when the host offered nowhere to put it (the
    /// admin form's dry-run build, "Test connection"): the provider then
    /// downloads the full export, which is always correct and never cheap.
    cache_dir: Option<PathBuf>,
    /// Parsed once per provider lifetime, which is once per sync: the walk
    /// asks for the listing and then fetches hundreds of documents out of it,
    /// and re-reading a 187 MB archive per document is not a thing to leave
    /// available by accident.
    archive: OnceCell<Arc<Archive>>,
}

impl Hyperkitty {
    pub fn from_config(cfg: &ProviderConfig, ctx: &ProviderContext) -> Result<Self, ProviderError> {
        let cfg = &cfg.with_defaults(FIELDS);
        let list_url = cfg.require("list_url")?.trim_end_matches('/').to_string();
        if !list_url.starts_with("http://") && !list_url.starts_with("https://") {
            return Err(ProviderError::Config(format!(
                "List URL must start with http:// or https:// (got `{list_url}`)"
            )));
        }
        // HyperKitty's list page ends in the posting address, and its export
        // is named after it. Rejecting anything else here is what turns "you
        // pasted the archive's front page" into a message at save time rather
        // than a 404 in the middle of the night.
        let list_name = list_url.rsplit('/').next().unwrap_or_default().to_string();
        if !list_name.contains('@') {
            return Err(ProviderError::Config(format!(
                "`{list_url}` does not look like a HyperKitty list page — it should end with the \
                 list's posting address, e.g. \
                 https://lists.example.com/hyperkitty/list/users@example.com/"
            )));
        }
        Ok(Self {
            export_url: format!("{list_url}/export/{list_name}.mbox.gz"),
            list_url,
            list_name,
            http: ctx.http.clone(),
            cache_dir: cfg
                .bool("cache_archive", true)
                .then(|| ctx.cache_dir.clone())
                .flatten(),
            archive: OnceCell::new(),
        })
    }

    async fn archive(&self) -> Result<&Arc<Archive>, ProviderError> {
        self.archive.get_or_try_init(|| self.load()).await
    }

    /// Produce the corpus, cheaply where possible.
    ///
    /// A cache failure is never a sync failure: anything unreadable, stale or
    /// simply absent falls back to the full export, which always works. That
    /// is the whole contract — the cache changes what is downloaded, never
    /// what is indexed.
    async fn load(&self) -> Result<Arc<Archive>, ProviderError> {
        let Some(dir) = self.cache_dir.clone() else {
            return self.load_direct().await;
        };
        match self.load_cached(&dir).await {
            Ok(archive) => Ok(archive),
            Err(err) => {
                tracing::warn!(
                    list = %self.list_url,
                    error = %err,
                    "rag: the cached mailing-list archive could not be used, downloading it whole"
                );
                wipe_cache(&dir);
                self.load_direct().await
            }
        }
    }

    /// No cache: download the whole export into memory and parse it.
    async fn load_direct(&self) -> Result<Arc<Archive>, ProviderError> {
        let resp = self.get(&self.export_url, None, None).await?;
        let gz = super::read_capped(KIND, &self.export_url, resp, MAX_COMPRESSED_BYTES).await?;
        let list_url = self.list_url.clone();
        self.parse_blocking(move || build_archive(&gz[..], &list_url))
            .await
    }

    /// Bring the on-disk copy up to date, then parse it.
    ///
    /// Two shapes: refill from the full export (first sync, a different list,
    /// an aged-out copy, too many segments), or fetch the days since the last
    /// sync and append them. A refill needs no window afterwards — the export
    /// it just wrote is current — so exactly one request leaves here per sync.
    async fn load_cached(&self, dir: &Path) -> Result<Arc<Archive>, ProviderError> {
        let today = today_utc();
        let reusable = CacheState::read(dir)
            .filter(|s| s.is_usable_for(&self.list_url, today))
            .filter(|_| {
                segments(dir)
                    .map(|s| s.len() < MAX_SEGMENTS)
                    .unwrap_or(false)
            });
        match reusable {
            None => {
                wipe_cache(dir);
                self.fetch_full(dir, today).await?;
            }
            Some(mut state) => {
                // Unconditionally, not only once a day: someone who clicks
                // Re-index is asking whether anything new arrived, and a
                // window that reaches back a few days is a few kilobytes.
                // `end` is exclusive, so tomorrow's date is how today is
                // asked for, and `start` reaches back past what is already
                // covered because a duplicate message costs nothing (the
                // `Message-ID` de-duplicates it) while a missed one costs a
                // month.
                let from = state
                    .covered_through
                    .checked_sub(Span::new().days(WINDOW_OVERLAP_DAYS))
                    .unwrap_or(state.covered_through);
                let until = today
                    .checked_add(Span::new().days(1))
                    .unwrap_or(state.covered_through);
                self.fetch_window(dir, from, until).await?;
                state.covered_through = today;
                state.write(dir)?;
            }
        }

        let segments = segments(dir)?;
        if segments.is_empty() {
            return Err(ProviderError::Malformed(
                "the cache holds no archive segments".into(),
            ));
        }
        let list_url = self.list_url.clone();
        self.parse_blocking(move || {
            let mut reader: Box<dyn Read> = Box::new(std::io::empty());
            for path in &segments {
                let file = std::fs::File::open(path).map_err(|e| {
                    ProviderError::Malformed(format!("reading `{}`: {e}", path.display()))
                })?;
                // Gzip members concatenate: the segments decode as one stream.
                reader = Box::new(reader.chain(file));
            }
            build_archive(reader, &list_url)
        })
        .await
    }

    /// Download the whole export into the cache, replacing whatever was there.
    async fn fetch_full(&self, dir: &Path, today: Date) -> Result<CacheState, ProviderError> {
        let resp = self.get(&self.export_url, None, None).await?;
        write_segment(dir, "full", resp, MAX_COMPRESSED_BYTES).await?;
        let state = CacheState {
            version: CACHE_VERSION,
            list_url: self.list_url.clone(),
            full_fetched_at: Timestamp::now(),
            covered_through: today,
        };
        state.write(dir)?;
        Ok(state)
    }

    /// Download `[from, until)` and append it as another segment.
    async fn fetch_window(&self, dir: &Path, from: Date, until: Date) -> Result<(), ProviderError> {
        let resp = self
            .get(&self.export_url, None, Some((from, until)))
            .await?;
        // A window is days of mail, not years of it; a response the size of a
        // full export means the server ignored the query and the cap is the
        // thing that notices.
        write_segment(dir, &from.to_string(), resp, MAX_COMPRESSED_BYTES).await?;
        Ok(())
    }

    /// Run the parse off the async runtime.
    ///
    /// Decompressing and parsing 187 MB is seconds of solid CPU, and on the
    /// runtime that is seconds of every other request stalling.
    async fn parse_blocking<F>(&self, f: F) -> Result<Arc<Archive>, ProviderError>
    where
        F: FnOnce() -> Result<Arc<Archive>, ProviderError> + Send + 'static,
    {
        tokio::task::spawn_blocking(f)
            .await
            .map_err(|e| ProviderError::Malformed(format!("the archive parser panicked: {e}")))?
    }

    async fn get(
        &self,
        url: &str,
        range: Option<&str>,
        window: Option<(Date, Date)>,
    ) -> Result<reqwest::Response, ProviderError> {
        let mut req = self.http.get(url);
        if let Some(range) = range {
            req = req.header(reqwest::header::RANGE, range);
        }
        if let Some((start, end)) = window {
            // Verified against a live HyperKitty: `start` is inclusive and
            // `end` exclusive, both bucketing by the message's calendar date.
            req = req.query(&[("start", start.to_string()), ("end", end.to_string())]);
        }
        let resp = req
            .send()
            .await
            .map_err(|source| ProviderError::Transport {
                provider: KIND,
                source,
            })?;
        let status = resp.status();
        if !status.is_success() {
            return Err(status_error(status.as_u16(), url, resp.text().await));
        }
        Ok(resp)
    }
}

fn status_error(status: u16, url: &str, body: Result<String, reqwest::Error>) -> ProviderError {
    match status {
        401 => ProviderError::Unauthorized {
            provider: KIND,
            status,
            hint: "This list's archive is not public. Only public archives can be indexed — \
                   there is nowhere to put a login.",
        },
        403 => ProviderError::Forbidden {
            provider: KIND,
            path: url.to_string(),
            status,
            hint: "The server refused the archive download. Private lists refuse it for \
                   everyone who is not signed in.",
        },
        404 => ProviderError::NotFound {
            provider: KIND,
            path: url.to_string(),
            hint: "Check the list URL. It should be the page the browser shows for the list \
                   itself, ending in the posting address.",
        },
        _ => ProviderError::Status {
            provider: KIND,
            status,
            body: body.unwrap_or_default().chars().take(400).collect(),
        },
    }
}

/// Bumped when the on-disk layout changes. A state file from a different
/// version is not migrated — it is thrown away and the archive re-downloaded,
/// which is the right trade for a cache.
const CACHE_VERSION: u32 = 1;

/// What the cache knows about itself. Everything else is derived from the
/// segment files next to it, so a half-written state file costs one refill
/// rather than a wrong corpus.
#[derive(Debug, Serialize, Deserialize)]
struct CacheState {
    version: u32,
    /// Which list these bytes are. An operator who repoints a collection at a
    /// different list must not inherit the previous one's mail.
    list_url: String,
    full_fetched_at: Timestamp,
    /// The last date whose mail we have asked for, inclusive.
    covered_through: Date,
}

impl CacheState {
    fn read(dir: &Path) -> Option<Self> {
        let raw = std::fs::read(dir.join(STATE_FILE)).ok()?;
        serde_json::from_slice(&raw).ok()
    }

    fn write(&self, dir: &Path) -> Result<(), ProviderError> {
        let json = serde_json::to_vec_pretty(self)
            .map_err(|e| ProviderError::Malformed(format!("encoding the cache state: {e}")))?;
        let tmp = dir.join(format!("{STATE_FILE}.part"));
        std::fs::write(&tmp, &json)
            .and_then(|()| std::fs::rename(&tmp, dir.join(STATE_FILE)))
            .map_err(|e| ProviderError::Malformed(format!("writing the cache state: {e}")))
    }

    /// Whether this cache may be extended rather than refilled.
    fn is_usable_for(&self, list_url: &str, today: Date) -> bool {
        if self.version != CACHE_VERSION || self.list_url != list_url {
            return false;
        }
        // A `covered_through` in the future means the clock moved backwards
        // (a VM restored from a snapshot, an NTP correction). Refill: the
        // alternative is a window that never fires again.
        if self.covered_through > today {
            return false;
        }
        let age = Timestamp::now()
            .as_second()
            .saturating_sub(self.full_fetched_at.as_second());
        age < FULL_REFRESH_AFTER_DAYS * 86_400
    }
}

fn today_utc() -> Date {
    Timestamp::now().to_zoned(jiff::tz::TimeZone::UTC).date()
}

/// The cache's archive segments, in the order they must be decoded.
///
/// Ordering is the filename's sequence prefix, so it survives a directory
/// listing that arrives in any order. `.part` files — a download that died
/// halfway — are not segments until they are renamed into place.
fn segments(dir: &Path) -> Result<Vec<PathBuf>, ProviderError> {
    let entries = std::fs::read_dir(dir)
        .map_err(|e| ProviderError::Malformed(format!("reading `{}`: {e}", dir.display())))?;
    let mut out: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(".mbox.gz"))
        })
        .collect();
    out.sort();
    Ok(out)
}

/// Stream one response into a new segment file.
///
/// Written to `.part` and renamed, so a crash or a capped download leaves
/// nothing that the next sync would read as a complete segment.
async fn write_segment(
    dir: &Path,
    label: &str,
    resp: reqwest::Response,
    max_bytes: u64,
) -> Result<PathBuf, ProviderError> {
    use rama::futures::StreamExt as _;

    let label: String = label
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    let seq = segments(dir)?.len();
    let name = format!("{seq:06}-{label}.mbox.gz");
    let target = dir.join(&name);
    let part = dir.join(format!("{name}.part"));

    let file = std::fs::File::create(&part)
        .map_err(|e| ProviderError::Malformed(format!("creating `{}`: {e}", part.display())))?;
    let mut out = std::io::BufWriter::new(file);
    let mut written = 0u64;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(c) => c,
            Err(source) => {
                let _ = std::fs::remove_file(&part);
                return Err(ProviderError::Transport {
                    provider: KIND,
                    source,
                });
            }
        };
        written += chunk.len() as u64;
        if written > max_bytes {
            let _ = std::fs::remove_file(&part);
            return Err(ProviderError::Config(format!(
                "the archive is larger than the {max_bytes}-byte download limit"
            )));
        }
        if let Err(e) = out.write_all(&chunk) {
            let _ = std::fs::remove_file(&part);
            return Err(ProviderError::Malformed(format!(
                "writing `{}`: {e}",
                part.display()
            )));
        }
    }
    out.flush()
        .and_then(|()| {
            drop(out);
            std::fs::rename(&part, &target)
        })
        .map_err(|e| ProviderError::Malformed(format!("finishing `{}`: {e}", target.display())))?;
    Ok(target)
}

/// Throw the cached archive away. Leaves the directory itself, which the host
/// created and owns.
fn wipe_cache(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let _ = std::fs::remove_file(entry.path());
    }
}

/// A `Read` that fails rather than ending early once `limit` bytes are through.
///
/// `Read::take` would do the length check, but it reports the cap as
/// end-of-stream, which is the one answer that must not be given here: the
/// parser would see a complete archive that happens to stop in 2021.
struct Limited<R> {
    inner: R,
    left: u64,
}

impl<R: Read> Read for Limited<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.left == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("the archive expands to more than {MAX_DECOMPRESSED_BYTES} bytes"),
            ));
        }
        let cap = buf.len().min(self.left as usize);
        let n = self.inner.read(&mut buf[..cap])?;
        self.left -= n as u64;
        Ok(n)
    }
}

/// Decompress, parse, thread and render — the whole archive, on a blocking
/// thread. `Err` means nothing is indexed, which is the safe side of the trade
/// (see [`MAX_COMPRESSED_BYTES`]).
fn build_archive<R: Read>(gz: R, list_url: &str) -> Result<Arc<Archive>, ProviderError> {
    let reader = BufReader::with_capacity(
        64 * 1024,
        Limited {
            inner: MultiGzDecoder::new(gz),
            left: MAX_DECOMPRESSED_BYTES,
        },
    );
    let mut messages = Vec::new();
    mail::parse_mbox_reader(reader, &mut messages).map_err(|e| {
        ProviderError::Malformed(format!(
            "the archive could not be read to the end ({e}) — {} message(s) had been parsed; \
             nothing was indexed rather than indexing a fraction of the list",
            messages.len()
        ))
    })?;
    if messages.is_empty() {
        return Err(ProviderError::Malformed(
            "the archive downloaded and decompressed but contained no messages".into(),
        ));
    }

    let mut docs = Archive::new();
    for thread in mail::thread_messages(messages) {
        let modified_at = thread.messages.iter().filter_map(|m| m.date).max();
        let doc = Doc {
            rel_path: rel_path(&thread, modified_at),
            version: thread.version(),
            text: mail::render_thread(&thread, list_url),
            modified_at,
        };
        docs.insert(thread.root_id, doc);
    }
    Ok(Arc::new(docs))
}

/// Where a thread appears to live: `2026/09/osd-flapping-after-upgrade-MFZW….md`.
///
/// The path is cosmetic — identity is the thread id — but it is what the user
/// sees as provenance and what include/exclude globs match on, so it is worth
/// being readable and dated. The hash suffix is what keeps two threads with
/// the same subject in the same month from colliding.
fn rel_path(thread: &mail::Thread, modified_at: Option<Timestamp>) -> String {
    let when = thread.messages.first().and_then(|m| m.date).or(modified_at);
    let (year, month) = match when {
        Some(ts) => {
            let z = ts.to_zoned(jiff::tz::TimeZone::UTC);
            (z.year().to_string(), format!("{:02}", z.month()))
        }
        None => ("undated".to_string(), "00".to_string()),
    };
    let hash = mail::message_hash(&thread.root_id);
    format!(
        "{year}/{month}/{}-{}.md",
        slug(&thread.subject),
        hash[..10].to_lowercase()
    )
}

/// Subject to filename: ASCII words joined by `-`, capped so a mail client
/// that wrapped the whole first paragraph into the subject cannot produce a
/// 900-character path.
fn slug(subject: &str) -> String {
    let mut out = String::with_capacity(SLUG_MAX);
    for word in subject
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
    {
        if !out.is_empty() && out.len() + 1 + word.len() > SLUG_MAX {
            break;
        }
        if !out.is_empty() {
            out.push('-');
        }
        out.push_str(&word.to_ascii_lowercase());
        // A single word longer than the cap is still worth keeping the head of.
        if out.len() >= SLUG_MAX {
            out.truncate(SLUG_MAX);
            break;
        }
    }
    if out.is_empty() {
        out.push_str("thread");
    }
    out
}

/// Cap on the slug, not on the path: the date folders and the hash suffix add
/// their own fixed length on top.
const SLUG_MAX: usize = 60;

#[async_trait::async_trait]
impl FileProvider for Hyperkitty {
    fn kind(&self) -> &'static str {
        KIND
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            // No directories, so nothing to prune; and no change feed. The
            // saving that would come from either is already had by versioning
            // each thread — see the module docs.
            subtree_pruning: false,
            delta: false,
            // A `Message-ID` is assigned by the sending mail client and never
            // changes. There is no more stable id in this domain.
            stable_ids: true,
        }
    }

    fn root(&self) -> DirRef {
        DirRef::root(self.export_url.clone())
    }

    /// The archive is one flat listing: a mailing list has no folders, and
    /// inventing some would only make the walker issue requests to learn what
    /// a single parse already knows.
    async fn list_dir(&self, dir: &DirRef) -> Result<DirListing, ProviderError> {
        if !dir.rel_path.is_empty() {
            return Err(ProviderError::NotFound {
                provider: KIND,
                path: dir.rel_path.clone(),
                hint: "A mailing-list archive has no subdirectories.",
            });
        }
        let archive = self.archive().await?;
        let entries = archive
            .iter()
            .map(|(id, doc)| RemoteEntry {
                id: id.clone(),
                locator: id.clone(),
                rel_path: doc.rel_path.clone(),
                kind: EntryKind::File,
                version: Some(doc.version.clone()),
                size_bytes: doc.text.len() as u64,
                mime: Some("text/markdown".into()),
                modified_at: doc.modified_at,
            })
            .collect();
        Ok(DirListing::Listed {
            entries,
            version: None,
        })
    }

    async fn fetch(&self, entry: &RemoteEntry, max_bytes: u64) -> Result<Vec<u8>, ProviderError> {
        let archive = self.archive().await?;
        let doc = archive
            .get(&entry.id)
            .ok_or_else(|| ProviderError::NotFound {
                provider: KIND,
                path: entry.rel_path.clone(),
                hint: "The thread was in the listing but not in the archive — re-run the sync.",
            })?;
        if doc.text.len() as u64 > max_bytes {
            return Err(ProviderError::Config(format!(
                "`{}` is larger than the {max_bytes}-byte limit for indexed files",
                entry.rel_path
            )));
        }
        Ok(doc.text.clone().into_bytes())
    }

    fn web_url(&self, entry: &RemoteEntry) -> Option<String> {
        Some(mail::permalink(&self.list_url, &entry.id))
    }

    /// Reads the first [`PROBE_BYTES`] of the export and reports what parsed.
    ///
    /// A truncated gzip is *expected* here and is not an error — the point is
    /// to prove the URL is right and the bytes are mail, not to download the
    /// archive. `root_entries` is therefore "messages visible in the first
    /// megabyte", which is what makes a wrong URL obvious (it is zero).
    async fn probe(&self) -> Result<ProbeReport, ProviderError> {
        let range = format!("bytes=0-{}", PROBE_BYTES - 1);
        let resp = self.get(&self.export_url, Some(&range), None).await?;
        let server = resp
            .headers()
            .get(reqwest::header::SERVER)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let gz = read_prefix(resp, PROBE_BYTES).await?;
        let mut messages = Vec::new();
        let _ =
            mail::parse_mbox_reader(BufReader::new(MultiGzDecoder::new(&gz[..])), &mut messages);
        if messages.is_empty() {
            return Err(ProviderError::Malformed(format!(
                "`{}` answered, but the first {PROBE_BYTES} bytes are not a gzipped mbox. Check \
                 that the list URL is a HyperKitty list page.",
                self.export_url
            )));
        }
        Ok(ProbeReport {
            account: Some(self.list_name.clone()),
            root_entries: messages.len(),
            server,
        })
    }
}

/// Read at most `max_bytes`, then stop — the deliberate-prefix counterpart to
/// [`super::read_capped`], which treats the same situation as an error.
async fn read_prefix(resp: reqwest::Response, max_bytes: u64) -> Result<Vec<u8>, ProviderError> {
    use rama::futures::StreamExt as _;

    let mut out: Vec<u8> = Vec::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|source| ProviderError::Transport {
            provider: KIND,
            source,
        })?;
        out.extend_from_slice(&chunk);
        if out.len() as u64 >= max_bytes {
            break;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn cfg(list_url: &str) -> ProviderConfig {
        ProviderConfig::new(
            BTreeMap::from([("list_url".to_string(), list_url.to_string())]),
            BTreeMap::new(),
        )
    }

    fn provider(list_url: &str) -> Hyperkitty {
        Hyperkitty::from_config(
            &cfg(list_url),
            &ProviderContext::new(reqwest::Client::new()),
        )
        .expect("a well-formed list URL builds")
    }

    #[test]
    fn the_export_and_permalink_urls_come_from_the_list_url() {
        let p = provider("https://lists.example.com/hyperkitty/list/users@example.com/");
        assert_eq!(
            p.export_url,
            "https://lists.example.com/hyperkitty/list/users@example.com/export/users@example.com.mbox.gz"
        );
        assert_eq!(p.list_name, "users@example.com");
        assert_eq!(
            p.web_url(&RemoteEntry {
                id: "root@example.com".into(),
                locator: "root@example.com".into(),
                rel_path: "2026/09/x.md".into(),
                kind: EntryKind::File,
                version: None,
                size_bytes: 0,
                mime: None,
                modified_at: None,
            })
            .as_deref(),
            Some(
                mail::permalink(
                    "https://lists.example.com/hyperkitty/list/users@example.com",
                    "root@example.com"
                )
                .as_str()
            )
        );
    }

    #[test]
    fn the_archive_front_page_is_rejected_at_save_time() {
        let err = Hyperkitty::from_config(
            &cfg("https://lists.example.com/hyperkitty/"),
            &ProviderContext::new(reqwest::Client::new()),
        )
        .map(|_| ())
        .expect_err("that URL names no list");
        assert!(
            err.to_string().contains("posting address"),
            "the message says what to paste instead: {err}"
        );
    }

    #[test]
    fn a_non_http_url_is_rejected() {
        let err = Hyperkitty::from_config(
            &cfg("lists.example.com/list/u@e.com"),
            &ProviderContext::new(reqwest::Client::new()),
        )
        .map(|_| ())
        .expect_err("no scheme");
        assert!(err.to_string().contains("http://"), "{err}");
    }

    #[test]
    fn a_thread_becomes_a_dated_readable_path() {
        let threads = mail::thread_messages(mail::parse_mbox(SAMPLE));
        let thread = &threads[0];
        let path = rel_path(thread, thread.messages.iter().filter_map(|m| m.date).max());
        assert!(
            path.starts_with("2026/09/osd-flapping-after-upgrade-"),
            "dated folders and a readable slug: {path}"
        );
        assert!(path.ends_with(".md"), "{path}");
    }

    #[test]
    fn two_threads_sharing_a_subject_and_month_get_different_paths() {
        let a = mail::Thread {
            root_id: "a@example.com".into(),
            subject: "OSD flapping".into(),
            messages: vec![],
        };
        let b = mail::Thread {
            root_id: "b@example.com".into(),
            subject: "OSD flapping".into(),
            messages: vec![],
        };
        assert_ne!(rel_path(&a, None), rel_path(&b, None));
    }

    #[test]
    fn a_subject_with_nothing_sluggable_still_yields_a_name() {
        assert_eq!(slug("——— ???"), "thread");
        assert_eq!(slug("Re: [ceph-users] OSD down!"), "re-ceph-users-osd-down");
        assert!(slug(&"word ".repeat(200)).len() <= SLUG_MAX);
        assert_eq!(
            slug(&"x".repeat(200)).len(),
            SLUG_MAX,
            "one long word is cut, not dropped"
        );
    }

    #[test]
    fn the_archive_parses_into_versioned_documents() {
        let gz = gzip(SAMPLE);
        let archive = build_archive(
            &gz[..],
            "https://lists.example.com/hyperkitty/list/users@example.com",
        )
        .expect("the sample archive builds");
        assert_eq!(archive.len(), 1, "three messages, one conversation");
        let doc = archive.values().next().expect("one document");
        assert!(
            doc.text.contains("osd_heartbeat_grace"),
            "the answer is in the document"
        );
        assert!(
            !doc.text.contains("> After upgrading"),
            "the quoted copy of the question is not"
        );
        assert!(!doc.version.is_empty());
    }

    #[test]
    fn an_empty_archive_is_an_error_not_an_empty_index() {
        // Otherwise a list that briefly serves an empty export deletes every
        // document the last sync indexed.
        let err = build_archive(&gzip(b"\n")[..], "https://lists.example.com/list/u@e.com")
            .map(|_| ())
            .expect_err("no messages is a failure");
        assert!(err.to_string().contains("no messages"), "{err}");
    }

    #[test]
    fn a_truncated_download_fails_rather_than_indexing_a_fraction() {
        let gz = gzip(SAMPLE);
        let err = build_archive(
            &gz[..gz.len() / 2],
            "https://lists.example.com/list/u@e.com",
        )
        .map(|_| ())
        .expect_err("half a gzip is not half an archive");
        assert!(
            err.to_string().contains("nothing was indexed"),
            "the message says what was done about it: {err}"
        );
    }

    fn state(list_url: &str, full_age_days: i64, covered: Date) -> CacheState {
        CacheState {
            version: CACHE_VERSION,
            list_url: list_url.to_string(),
            full_fetched_at: Timestamp::now() - Span::new().hours(full_age_days * 24),
            covered_through: covered,
        }
    }

    #[test]
    fn a_cache_is_reused_only_while_it_is_the_right_list_and_fresh_enough() {
        let url = "https://lists.example.com/hyperkitty/list/users@example.com";
        let today = today_utc();
        assert!(state(url, 1, today).is_usable_for(url, today));

        assert!(
            !state(
                "https://lists.example.com/hyperkitty/list/other@example.com",
                1,
                today
            )
            .is_usable_for(url, today),
            "repointing the collection at another list must not inherit its mail"
        );
        assert!(
            !state(url, FULL_REFRESH_AFTER_DAYS + 1, today).is_usable_for(url, today),
            "an aged-out copy is refilled, so deletions in the archive reach us"
        );

        let tomorrow = today.checked_add(Span::new().days(1)).unwrap();
        assert!(
            !state(url, 1, tomorrow).is_usable_for(url, today),
            "a cache that covers the future means the clock moved; refill rather than \
             never fetching again"
        );

        let mut wrong_version = state(url, 1, today);
        wrong_version.version = CACHE_VERSION + 1;
        assert!(!wrong_version.is_usable_for(url, today));
    }

    #[test]
    fn cache_state_round_trips_through_the_file_it_is_stored_in() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let url = "https://lists.example.com/hyperkitty/list/users@example.com";
        let before = state(url, 1, today_utc());
        before.write(dir.path()).expect("the state writes");
        let after = CacheState::read(dir.path()).expect("and reads back");
        assert_eq!(after.list_url, before.list_url);
        assert_eq!(after.covered_through, before.covered_through);
        assert_eq!(after.version, CACHE_VERSION);
        assert!(
            !dir.path().join(format!("{STATE_FILE}.part")).exists(),
            "the temp file is renamed, not left behind"
        );
    }

    /// The load path chains segment files into one gzip stream and expects
    /// them to decode as a single archive. That is a property of the format,
    /// not of our code, so it is pinned here: if it ever stopped holding,
    /// every incremental sync would silently index only the first segment.
    #[test]
    fn appended_segments_decode_as_one_archive_in_sequence_order() {
        let dir = tempfile::tempdir().expect("a temp dir");
        std::fs::write(dir.path().join("000000-full.mbox.gz"), gzip(SAMPLE))
            .expect("the full segment");
        std::fs::write(
            dir.path().join("000001-2026-09-03.mbox.gz"),
            gzip(LATE_REPLY),
        )
        .expect("the window segment");
        // Not a segment: a download that died halfway must not be decoded.
        std::fs::write(dir.path().join("000002-2026-09-04.mbox.gz.part"), b"junk")
            .expect("the partial");

        let found = segments(dir.path()).expect("the segment list");
        assert_eq!(
            found.len(),
            2,
            "the `.part` file is not a segment: {found:?}"
        );

        let mut reader: Box<dyn Read> = Box::new(std::io::empty());
        for path in &found {
            reader = Box::new(reader.chain(std::fs::File::open(path).expect("open")));
        }
        let archive = build_archive(reader, "https://lists.example.com/list/u@e.com")
            .expect("both segments decode as one stream");
        assert_eq!(archive.len(), 1, "the reply joined the existing thread");
        let doc = archive.values().next().expect("one document");
        assert!(
            doc.text.contains("osd_heartbeat_grace") && doc.text.contains("settled it"),
            "the thread carries the original messages and the appended one"
        );
    }

    /// The live server, which is where the assumptions actually live: that the
    /// export URL derives correctly, that it needs no credentials, and that a
    /// probe reads a prefix instead of the whole 57 MB. Ignored and gated on an
    /// env var so no third-party host is hardcoded here and no test reaches the
    /// network unasked:
    ///
    /// ```text
    /// HYPERKITTY_LIST_URL=https://lists.example.com/hyperkitty/list/users@example.com/ \
    ///     cargo nextest run -p gateway-features -E 'test(a_live_list)' --run-ignored all
    /// ```
    #[tokio::test]
    #[ignore = "needs network; set $HYPERKITTY_LIST_URL to a live list page"]
    async fn a_live_list_answers_a_probe_without_downloading_the_archive() {
        let Ok(list_url) = std::env::var("HYPERKITTY_LIST_URL") else {
            return;
        };
        let p = provider(&list_url);
        let started = std::time::Instant::now();
        let report = p.probe().await.expect("the live list probes");
        println!("  probe took     {:?}", started.elapsed());
        println!("  account        {:?}", report.account);
        println!("  server         {:?}", report.server);
        println!("  messages seen  {}", report.root_entries);
        assert!(
            report.root_entries > 0,
            "the first megabyte held parseable mail"
        );
        assert!(
            started.elapsed() < std::time::Duration::from_secs(60),
            "a probe that took this long is downloading the whole archive"
        );
    }

    /// The incremental path against the live server, which is the only place
    /// the window's contract actually holds. Downloads the full export once
    /// (tens of megabytes), then proves the second sync fetches a window,
    /// appends it, and still produces one coherent corpus.
    ///
    /// ```text
    /// HYPERKITTY_LIST_URL=https://lists.example.com/hyperkitty/list/users@example.com/ \
    ///     cargo nextest run -p gateway-features -E 'test(a_live_list_fills)' \
    ///     --run-ignored all --no-capture
    /// ```
    #[tokio::test]
    #[ignore = "downloads a full archive; set $HYPERKITTY_LIST_URL"]
    async fn a_live_list_fills_a_cache_and_then_only_extends_it() {
        let Ok(list_url) = std::env::var("HYPERKITTY_LIST_URL") else {
            return;
        };
        let dir = tempfile::tempdir().expect("a temp dir");
        let build = || {
            Hyperkitty::from_config(
                &cfg(&list_url),
                &ProviderContext::new(reqwest::Client::new()).with_cache_dir(dir.path()),
            )
            .expect("a well-formed list URL builds")
        };

        let started = std::time::Instant::now();
        let first = build().archive().await.expect("the first sync").clone();
        println!(
            "  full fill      {:?}, {} threads",
            started.elapsed(),
            first.len()
        );
        let after_full = segments(dir.path()).expect("segments");
        assert_eq!(after_full.len(), 1, "one segment: the full export");
        let state = CacheState::read(dir.path()).expect("state was written");
        assert_eq!(state.list_url, list_url.trim_end_matches('/'));

        // Ask again as tomorrow's sync would, with yesterday already covered.
        let mut state = CacheState::read(dir.path()).expect("state");
        state.covered_through = state
            .covered_through
            .checked_sub(Span::new().days(2))
            .expect("two days back");
        state.write(dir.path()).expect("state rewrites");

        let started = std::time::Instant::now();
        let second = build().archive().await.expect("the second sync").clone();
        let elapsed = started.elapsed();
        let after_window = segments(dir.path()).expect("segments");
        let window_bytes = std::fs::metadata(&after_window[1])
            .expect("window size")
            .len();
        println!(
            "  window fetch   {elapsed:?}, {window_bytes} bytes, {} threads",
            second.len()
        );
        assert_eq!(after_window.len(), 2, "the window was appended, not merged");
        assert!(
            window_bytes * 20 < std::fs::metadata(&after_window[0]).unwrap().len(),
            "a few days must cost far less than the whole archive ({window_bytes} bytes)"
        );
        assert!(
            second.len() >= first.len(),
            "a window only ever adds mail: {} then {}",
            first.len(),
            second.len()
        );
        // Every document the first pass produced is still there, and still
        // parses — the concatenated stream is not subtly truncated.
        for id in first.keys() {
            let Some(now) = second.get(id) else {
                panic!("thread `{id}` vanished after the incremental sync");
            };
            assert!(!now.text.is_empty(), "thread `{id}` came back empty");
        }
    }

    /// The real archive, end to end: gzip in, documents out.
    ///
    /// Ignored because it needs the 57 MB export on disk — download it from
    /// the list's `export/<list>.mbox.gz` URL and point `CEPH_MBOX_GZ` at it.
    /// What it guards is what unit fixtures cannot: that 27,000 real messages
    /// produce unique paths, that every document has a version, and that the
    /// whole thing survives the parser.
    #[test]
    #[ignore = "needs a full archive at $CEPH_MBOX_GZ"]
    fn the_real_archive_builds_an_index() {
        let Ok(path) = std::env::var("CEPH_MBOX_GZ") else {
            return;
        };
        let gz = std::fs::read(&path).expect("the archive at $CEPH_MBOX_GZ");
        let started = std::time::Instant::now();
        let archive = build_archive(
            &gz[..],
            "https://lists.example.com/hyperkitty/list/users@example.com",
        )
        .expect("the real archive builds");
        let elapsed = started.elapsed();

        let mut paths: Vec<&str> = archive.values().map(|d| d.rel_path.as_str()).collect();
        paths.sort_unstable();
        let unique = {
            let mut p = paths.clone();
            p.dedup();
            p.len()
        };
        let bytes: usize = archive.values().map(|d| d.text.len()).sum();
        println!("  threads        {:>12}", archive.len());
        println!("  unique paths   {:>12}", unique);
        println!("  rendered bytes {:>12}", bytes);
        println!("  build time     {:>12?}", elapsed);
        println!(
            "  longest path   {:>12}",
            paths.iter().map(|p| p.len()).max().unwrap_or(0)
        );

        assert_eq!(unique, paths.len(), "every thread gets its own path");
        assert!(
            archive
                .values()
                .all(|d| !d.version.is_empty() && !d.text.is_empty()),
            "no document is versionless or empty"
        );
        assert!(
            archive
                .keys()
                .all(|id| !id.contains('<') && !id.contains('>')),
            "ids are bare Message-IDs"
        );
    }

    fn gzip(raw: &[u8]) -> Vec<u8> {
        use std::io::Write as _;
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        enc.write_all(raw).expect("in-memory write");
        enc.finish().expect("in-memory finish")
    }

    /// A later reply to `SAMPLE`'s thread, as a date window would deliver it.
    const LATE_REPLY: &[u8] = b"From users@example.com Thu Sep  3 07:00:00 2026\n\
Message-ID: <reply3@example.com>\n\
In-Reply-To: <reply2@example.com>\n\
Subject: [ceph-users] Re: OSD flapping after upgrade\n\
From: Jane Roe <jane@example.com>\n\
Date: Thu, 3 Sep 2026 07:00:00 +0000\n\
Content-Type: text/plain; charset=utf-8\n\
\n\
Confirmed, that settled it.\n\
";

    /// The same miniature archive `mail.rs` tests against: a question, a
    /// quoted reply carrying the answer, and a reply linked only by subject.
    const SAMPLE: &[u8] = b"From users@example.com Mon Sep  1 09:00:00 2026\n\
Message-ID: <root@example.com>\n\
Subject: [ceph-users] OSD flapping after upgrade\n\
From: Jane Roe <jane@example.com>\n\
Date: Mon, 1 Sep 2026 09:00:00 +0000\n\
Content-Type: text/plain; charset=utf-8\n\
\n\
After upgrading to 19.2.1 our OSDs flap every few minutes.\n\
\n\
From users@example.com Mon Sep  1 10:00:00 2026\n\
Message-ID: <reply1@example.com>\n\
In-Reply-To: <root@example.com>\n\
Subject: [ceph-users] Re: OSD flapping after upgrade\n\
From: John Doe <john@example.com>\n\
Date: Mon, 1 Sep 2026 10:00:00 +0000\n\
Content-Type: text/plain; charset=utf-8\n\
\n\
On Mon, 1 Sep 2026 at 09:00, Jane Roe wrote:\n\
> After upgrading to 19.2.1 our OSDs flap every few minutes.\n\
\n\
Raise osd_heartbeat_grace to 30. Known regression in 19.2.1.\n\
\n\
From users@example.com Mon Sep  1 11:00:00 2026\n\
Message-ID: <reply2@example.com>\n\
Subject: [ceph-users] Re: OSD flapping after upgrade\n\
From: Jane Roe <jane@example.com>\n\
Date: Mon, 1 Sep 2026 11:00:00 +0000\n\
Content-Type: text/plain; charset=utf-8\n\
\n\
That fixed it, thanks!\n\
";
}
