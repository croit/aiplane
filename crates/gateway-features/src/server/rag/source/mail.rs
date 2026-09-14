// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Turning a mailing-list archive into documents worth retrieving.
//!
//! Pure functions, no I/O: an mbox goes in, threads come out. The provider
//! ([`super::hyperkitty`]) does the fetching; everything that decides retrieval
//! *quality* lives here, where it can be tested against real messages.
//!
//! # Why a thread is the document
//!
//! The answer to a debugging question is almost never in the question. It is
//! three replies down, in someone saying "that's the `bluestore_allocator`
//! bug, set X". Indexing messages individually retrieves the question and
//! leaves the fix behind. Measured on one month of `ceph-users`: the median
//! *message* is 568 characters — too thin to embed usefully — while the median
//! *thread* is 1,912, and 78% of threads fit in a single 4,000-character chunk.
//! So a thread is one document, its messages delimited inside it, and a hit
//! anywhere in it returns the whole exchange.
//!
//! # Why quoting is stripped
//!
//! Also measured, on the same month: **71% of all body lines were quoted
//! reply-chain text**, and stripping quotes, signatures and the list footer
//! removed **76% of the bytes**. Left in, that text is not merely wasted
//! embedding compute — it actively breaks retrieval. The original question is
//! repeated verbatim in every reply, so BM25 ranks whatever was quoted most
//! rather than whatever is most relevant, and dense search returns a page of
//! near-identical neighbours. Stripping is the single biggest quality lever in
//! this module.
//!
//! What is *not* stripped: attribution. The archive is public and knowing who
//! answered is part of judging the answer.
//!
//! # Why threading is not just `References`
//!
//! The textbook way to rebuild a thread is the `References` header. HyperKitty's
//! mbox export does not emit it — 0 of 182 messages in the sample carried one,
//! while 78% carried `In-Reply-To`. So threading walks `In-Reply-To` where it
//! exists and falls back to the normalised subject, which is what recovers the
//! remaining fifth.

use std::collections::{BTreeMap, HashMap};

use jiff::Timestamp;
use mail_parser::MessageParser;

/// One message, already decoded and cleaned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// RFC 5322 `Message-ID`, angle brackets stripped. The stable identity:
    /// the same message appears in both the full archive and every date window
    /// that covers it, and this is what de-duplicates them.
    pub id: String,
    pub subject: String,
    /// Display form, e.g. `Jane Roe <jane@example.com>`. Kept deliberately.
    pub from: String,
    pub date: Option<Timestamp>,
    pub in_reply_to: Option<String>,
    /// Body with quotes, signature and list footer removed.
    pub body: String,
}

/// A reconstructed conversation, oldest message first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thread {
    /// `Message-ID` of the oldest message. The document's stable id.
    pub root_id: String,
    /// Subject with `Re:` and the list tag removed.
    pub subject: String,
    pub messages: Vec<Message>,
}

impl Thread {
    /// Changes iff the thread's content does.
    ///
    /// Drives the indexer's "has this document changed?" check, so it must
    /// move when a reply lands and stay put otherwise — hence the message ids
    /// *and* their cleaned bodies, not a message count or a newest-date.
    pub fn version(&self) -> String {
        use sha1::{Digest, Sha1};
        let mut hasher = Sha1::new();
        for m in &self.messages {
            hasher.update(m.id.as_bytes());
            hasher.update([0]);
            hasher.update(m.body.as_bytes());
            hasher.update([0]);
        }
        data_encoding::HEXLOWER.encode(&hasher.finalize())
    }
}

/// Split an mbox into messages and clean each one.
///
/// Anything unparseable is skipped rather than failing the batch: a 27,000
/// message archive that refuses to index because of one malformed message from
/// 2019 is worth less than one that indexes 26,999.
pub fn parse_mbox(raw: &[u8]) -> Vec<Message> {
    let mut out = Vec::new();
    // A slice reader cannot fail, so there is no error to propagate here.
    let _ = parse_mbox_reader(raw, &mut out);
    out
}

/// Parse an mbox as it arrives, appending to `out`.
///
/// The archive this exists for is 187 MB of plain text behind 40 MB of gzip.
/// Reading it as a slice means holding the decompressed whole in memory next
/// to everything parsed out of it; streaming it means the peak is the messages
/// that are kept, which is a fifth of that. Hence a reader rather than another
/// `&[u8]` entry point.
///
/// **Messages parsed before an I/O error are kept, and the error is still
/// returned.** Both halves matter: a caller reading a deliberate prefix (the
/// connection probe) wants what parsed and expects the truncation, while a
/// caller indexing the real archive must treat a short read as a failure —
/// half an archive is indistinguishable, to the sync planner, from an archive
/// whose other half was deleted.
///
/// Separator: `From ` at the start of a line. A body line that would look like
/// one is escaped to `>From ` by every mbox writer, so a plain prefix test is
/// correct here and needs none of the date-shape guessing that `mboxo`-tolerant
/// readers use. Bytes before the first separator are not part of any message.
pub fn parse_mbox_reader<R: std::io::BufRead>(
    mut reader: R,
    out: &mut Vec<Message>,
) -> std::io::Result<()> {
    let mut current: Option<Vec<u8>> = None;
    let mut line: Vec<u8> = Vec::new();
    let mut failure: Option<std::io::Error> = None;
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) => {
                failure = Some(e);
                break;
            }
        }
        if line.starts_with(b"From ") {
            if let Some(raw) = current.replace(Vec::new())
                && let Some(m) = parse_one(&raw)
            {
                out.push(m);
            }
        } else if let Some(buf) = current.as_mut() {
            buf.extend_from_slice(&line);
        }
    }
    if let Some(raw) = current
        && let Some(m) = parse_one(&raw)
    {
        out.push(m);
    }
    match failure {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

fn parse_one(raw: &[u8]) -> Option<Message> {
    let parsed = MessageParser::default().parse(raw)?;
    let id = parsed
        .message_id()?
        .trim()
        .trim_matches(['<', '>'])
        .to_string();
    if id.is_empty() {
        return None;
    }
    // `text_bodies` already walked the MIME tree, decoded the transfer encoding
    // and transcoded the charset. Every message in the sample was multipart and
    // every one carried a `text/plain` part, so preferring it costs nothing and
    // avoids indexing marked-up HTML.
    let body_raw = parsed
        .body_text(0)
        .map(|b| b.to_string())
        .or_else(|| parsed.body_html(0).map(|h| strip_html(&h)))?;
    let body = clean_body(&body_raw);
    Some(Message {
        id,
        subject: normalise_subject(parsed.subject().unwrap_or_default()),
        from: parsed
            .from()
            .and_then(|a| a.first())
            .map(|a| match (a.name(), a.address()) {
                (Some(n), Some(e)) => format!("{n} <{e}>"),
                (None, Some(e)) => e.to_string(),
                (Some(n), None) => n.to_string(),
                _ => String::new(),
            })
            .unwrap_or_default(),
        date: parsed
            .date()
            .and_then(|d| Timestamp::from_second(d.to_timestamp()).ok()),
        in_reply_to: parsed
            .in_reply_to()
            .as_text_list()
            .and_then(|l| {
                l.first()
                    .map(|s| s.trim().trim_matches(['<', '>']).to_string())
            })
            .filter(|s| !s.is_empty()),
        body,
    })
}

/// Crude tag strip, for the rare message with no `text/plain` part.
fn strip_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

/// Strip the list footer, the signature, attribution lines and quoted text.
///
/// Order matters: the footer and signature are removed before quote stripping,
/// because a forwarded mail can carry a *quoted* footer that the line filter
/// would otherwise leave behind as `> ceph-users mailing list`.
pub fn clean_body(raw: &str) -> String {
    let mut text = raw.replace("\r\n", "\n");

    // Mailman's footer, and everything after it.
    if let Some(cut) = find_footer(&text) {
        text.truncate(cut);
    }
    // RFC 3676 signature delimiter: a line of exactly "-- ".
    if let Some(cut) = text
        .match_indices('\n')
        .map(|(i, _)| i + 1)
        .chain(std::iter::once(0))
        .find(|&i| {
            let line_end = text[i..].find('\n').map(|e| i + e).unwrap_or(text.len());
            matches!(text[i..line_end].trim_end_matches('\r'), "-- " | "--")
        })
    {
        text.truncate(cut);
    }

    let mut out: Vec<&str> = Vec::new();
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with('>') {
            continue;
        }
        if is_attribution(t) {
            continue;
        }
        out.push(line);
    }
    collapse_blank_lines(&out.join("\n")).trim().to_string()
}

fn find_footer(text: &str) -> Option<usize> {
    // Mailman 3 writes a rule of underscores, then the list's own address.
    let mut best: Option<usize> = None;
    for (idx, _) in text.match_indices("____") {
        let tail = &text[idx..];
        // Walk back to a character boundary: a fixed byte offset lands inside a
        // multi-byte character often enough that the real archive panics on it
        // (a `ü` 400 bytes past a footer rule was the first one).
        let mut window_end = tail.len().min(400);
        while window_end > 0 && !tail.is_char_boundary(window_end) {
            window_end -= 1;
        }
        let window = &tail[..window_end];
        if window.contains("mailing list")
            || window.contains("To unsubscribe send an email")
            || window.contains("unsubscribe")
        {
            best = Some(best.map_or(idx, |b: usize| b.min(idx)));
        }
    }
    best
}

/// `On Tue, 2 Sep 2026 at 09:14, Jane Roe wrote:` and its German twin.
///
/// These carry no information once the quote beneath them is gone, and left in
/// they are the most repeated sentence in the corpus.
fn is_attribution(line: &str) -> bool {
    let l = line.trim();
    if l.len() > 200 {
        return false;
    }
    let lower = l.to_lowercase();
    (lower.ends_with("wrote:") || lower.ends_with("schrieb:") || lower.ends_with("a écrit :"))
        && (lower.starts_with("on ")
            || lower.starts_with("am ")
            || lower.starts_with("le ")
            || lower.contains(" <")
            || lower.len() < 120)
}

fn collapse_blank_lines(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut blanks = 0usize;
    for line in s.lines() {
        if line.trim().is_empty() {
            blanks += 1;
            if blanks > 1 {
                continue;
            }
        } else {
            blanks = 0;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Drop `Re:`/`AW:` prefixes and the list tag, so replies group with their root.
pub fn normalise_subject(subject: &str) -> String {
    let mut s = subject.trim();
    loop {
        let lower = s.to_lowercase();
        let trimmed = if let Some(rest) = lower.strip_prefix("re:") {
            &s[s.len() - rest.len()..]
        } else if let Some(rest) = lower.strip_prefix("aw:") {
            &s[s.len() - rest.len()..]
        } else if lower.starts_with('[') {
            match s.find(']') {
                Some(i) => &s[i + 1..],
                None => break,
            }
        } else {
            break;
        };
        s = trimmed.trim_start();
    }
    s.trim().to_string()
}

/// Group messages into threads, oldest first within each.
///
/// `In-Reply-To` where present, normalised subject otherwise — see the module
/// docs for why `References` is not an option here.
pub fn thread_messages(messages: Vec<Message>) -> Vec<Thread> {
    // The same message legitimately arrives more than once: the provider
    // stitches the full archive together with overlapping date windows, and
    // the overlap is deliberate (a message HyperKitty filed a day either side
    // of where we expected it is worth catching twice rather than missing).
    // `Message-ID` is the identity that makes that safe, so it is enforced
    // here, once, rather than in every caller.
    let mut seen: HashMap<String, ()> = HashMap::with_capacity(messages.len());
    let messages: Vec<Message> = messages
        .into_iter()
        .filter(|m| seen.insert(m.id.clone(), ()).is_none())
        .collect();

    let by_id: HashMap<&str, usize> = messages
        .iter()
        .enumerate()
        .map(|(i, m)| (m.id.as_str(), i))
        .collect();

    // Walk each message up its reply chain to a root, with a hop cap so a
    // malformed archive that points two messages at each other cannot spin.
    let root_of: Vec<usize> = (0..messages.len())
        .map(|i| {
            let mut cur = i;
            for _ in 0..64 {
                let Some(parent) = messages[cur].in_reply_to.as_deref() else {
                    break;
                };
                match by_id.get(parent) {
                    Some(&p) if p != cur => cur = p,
                    _ => break,
                }
            }
            cur
        })
        .collect();

    // Subject is the fallback *and* the merge key: two roots sharing a subject
    // are one conversation whose linking header the export dropped.
    let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, &root) in root_of.iter().enumerate() {
        let subject = messages[root].subject.to_lowercase();
        let key = if subject.is_empty() {
            format!("id:{}", messages[root].id)
        } else {
            format!("subj:{subject}")
        };
        groups.entry(key).or_default().push(i);
    }

    let mut threads: Vec<Thread> = groups
        .into_values()
        .map(|mut idx| {
            idx.sort_by_key(|&i| (messages[i].date, i));
            let messages: Vec<Message> = idx.iter().map(|&i| messages[i].clone()).collect();
            Thread {
                root_id: messages[0].id.clone(),
                subject: messages[0].subject.clone(),
                messages,
            }
        })
        .collect();
    threads.sort_by(|a, b| {
        b.messages[0]
            .date
            .cmp(&a.messages[0].date)
            .then_with(|| a.root_id.cmp(&b.root_id))
    });
    threads
}

/// HyperKitty's permalink for a message.
///
/// Derived, not looked up: HyperKitty keys a message by
/// `base32(sha1(message-id))`, so a citation costs no network call. Verified
/// against the live archive.
pub fn permalink(list_base_url: &str, message_id: &str) -> String {
    format!(
        "{}/message/{}/",
        list_base_url.trim_end_matches('/'),
        message_hash(message_id)
    )
}

/// The last path segment of a [`permalink`]: `base32(sha1(message-id))`.
///
/// Exposed separately because it is also the only short, collision-free,
/// stable handle a message has — which is what a filename needs.
pub fn message_hash(message_id: &str) -> String {
    use sha1::{Digest, Sha1};
    data_encoding::BASE32_NOPAD.encode(&Sha1::digest(message_id.as_bytes()))
}

/// Render a thread as the markdown the indexer chunks and the model reads.
///
/// Each message keeps a header line so the model can attribute an answer and
/// tell a 2019 reply from a 2026 one — which matters a lot when the advice is
/// version-specific.
pub fn render_thread(thread: &Thread, list_base_url: &str) -> String {
    let mut out = String::with_capacity(2048);
    out.push_str("# ");
    out.push_str(&thread.subject);
    out.push_str("\n\n");
    out.push_str("Mailing-list thread, ");
    out.push_str(&thread.messages.len().to_string());
    out.push_str(if thread.messages.len() == 1 {
        " message.\n\n"
    } else {
        " messages.\n\n"
    });
    for (i, m) in thread.messages.iter().enumerate() {
        out.push_str("## ");
        out.push_str(&(i + 1).to_string());
        out.push_str(". ");
        out.push_str(if m.from.is_empty() {
            "unknown sender"
        } else {
            &m.from
        });
        if let Some(d) = m.date {
            out.push_str(" — ");
            out.push_str(&d.strftime("%Y-%m-%d").to_string());
        }
        out.push('\n');
        out.push_str(&permalink(list_base_url, &m.id));
        out.push_str("\n\n");
        out.push_str(&m.body);
        out.push_str("\n\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A miniature archive carrying the shapes the real one does: multipart
    /// bodies, quoted replies, an attribution line, a signature, the Mailman
    /// footer, `In-Reply-To` on one reply and nothing but a subject on the
    /// other — which is the 22% case HyperKitty's export leaves unlinked.
    const SAMPLE: &[u8] = b"From ceph-users@ceph.io Mon Sep  1 09:00:00 2026\n\
Message-ID: <root@example.com>\n\
Subject: [ceph-users] OSD flapping after upgrade\n\
From: Jane Roe <jane@example.com>\n\
Date: Mon, 1 Sep 2026 09:00:00 +0000\n\
Content-Type: text/plain; charset=utf-8\n\
\n\
After upgrading to 19.2.1 our OSDs flap every few minutes.\n\
osd.12 marked down\n\
\n\
-- \n\
Jane Roe, Storage Team\n\
\n\
_______________________________________________\n\
ceph-users mailing list -- ceph-users@ceph.io\n\
To unsubscribe send an email to ceph-users-leave@ceph.io\n\
From ceph-users@ceph.io Mon Sep  1 10:00:00 2026\n\
Message-ID: <reply1@example.com>\n\
In-Reply-To: <root@example.com>\n\
Subject: [ceph-users] Re: OSD flapping after upgrade\n\
From: John Doe <john@example.com>\n\
Date: Mon, 1 Sep 2026 10:00:00 +0000\n\
Content-Type: text/plain; charset=utf-8\n\
\n\
On Mon, 1 Sep 2026 at 09:00, Jane Roe wrote:\n\
> After upgrading to 19.2.1 our OSDs flap every few minutes.\n\
> osd.12 marked down\n\
\n\
Raise osd_heartbeat_grace to 30. Known regression in 19.2.1.\n\
\n\
_______________________________________________\n\
ceph-users mailing list -- ceph-users@ceph.io\n\
From ceph-users@ceph.io Mon Sep  1 11:00:00 2026\n\
Message-ID: <reply2@example.com>\n\
Subject: [ceph-users] Re: OSD flapping after upgrade\n\
From: Jane Roe <jane@example.com>\n\
Date: Mon, 1 Sep 2026 11:00:00 +0000\n\
Content-Type: text/plain; charset=utf-8\n\
\n\
That fixed it, thanks!\n\
";

    /// Hands out `SAMPLE` up to `fail_after` bytes, then fails — a truncated
    /// gzip stream, which is what a capped or interrupted download looks like.
    struct Truncating {
        pos: usize,
        fail_after: usize,
    }

    impl std::io::Read for Truncating {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.pos >= self.fail_after {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "stream ended early",
                ));
            }
            let end = (self.pos + buf.len()).min(self.fail_after);
            let n = end - self.pos;
            buf[..n].copy_from_slice(&SAMPLE[self.pos..end]);
            self.pos = end;
            Ok(n)
        }
    }

    #[test]
    fn a_short_read_keeps_what_parsed_and_still_reports_the_failure() {
        let mut out = Vec::new();
        let err = parse_mbox_reader(
            std::io::BufReader::new(Truncating {
                pos: 0,
                fail_after: 900,
            }),
            &mut out,
        )
        .expect_err("the stream ended before the archive did");
        assert_eq!(err.kind(), std::io::ErrorKind::UnexpectedEof);
        assert!(
            !out.is_empty(),
            "the messages that did arrive are kept, so a probe can show them"
        );
        assert_eq!(out[0].id, "root@example.com");
    }

    #[test]
    fn the_same_message_twice_is_one_message() {
        // The provider overlaps its date windows on purpose, so the same mail
        // arrives in two segments. Left alone it would appear twice in the
        // thread, be embedded twice, and move the thread's version on every
        // sync forever.
        let mut doubled = parse_mbox(SAMPLE);
        doubled.extend(parse_mbox(SAMPLE));
        let threads = thread_messages(doubled);
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].messages.len(), 3, "three, not six");
        assert_eq!(
            threads[0].version(),
            thread_messages(parse_mbox(SAMPLE))[0].version(),
            "and the duplicate does not move the version"
        );
    }

    #[test]
    fn an_mbox_splits_into_its_messages() {
        let msgs = parse_mbox(SAMPLE);
        assert_eq!(msgs.len(), 3, "three messages in the sample");
        assert_eq!(msgs[0].id, "root@example.com");
        assert_eq!(msgs[0].from, "Jane Roe <jane@example.com>");
        assert_eq!(msgs[1].in_reply_to.as_deref(), Some("root@example.com"));
    }

    #[test]
    fn quoting_signature_and_footer_are_removed_but_the_answer_is_kept() {
        let msgs = parse_mbox(SAMPLE);
        let reply = &msgs[1].body;
        assert!(
            reply.contains("osd_heartbeat_grace"),
            "the actual answer must survive: {reply:?}"
        );
        assert!(!reply.contains('>'), "quoted lines must go: {reply:?}");
        assert!(
            !reply.to_lowercase().contains("wrote:"),
            "the attribution line must go: {reply:?}"
        );
        assert!(
            !reply.contains("mailing list"),
            "the list footer must go: {reply:?}"
        );
        assert!(
            !msgs[0].body.contains("Storage Team"),
            "the signature must go: {:?}",
            msgs[0].body
        );
        assert!(
            msgs[0].body.contains("osd.12 marked down"),
            "log lines are the point — keep them: {:?}",
            msgs[0].body
        );
    }

    #[test]
    fn a_reply_without_in_reply_to_still_joins_its_thread() {
        // The 22% case: HyperKitty emits no `References`, and not every reply
        // carries `In-Reply-To` either. Subject is what recovers those.
        let threads = thread_messages(parse_mbox(SAMPLE));
        assert_eq!(
            threads.len(),
            1,
            "one conversation, not three: {threads:#?}"
        );
        let t = &threads[0];
        assert_eq!(t.subject, "OSD flapping after upgrade");
        assert_eq!(t.root_id, "root@example.com");
        assert_eq!(t.messages.len(), 3);
        assert_eq!(
            t.messages.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            [
                "root@example.com",
                "reply1@example.com",
                "reply2@example.com"
            ],
            "oldest first, so the question precedes its answer"
        );
    }

    #[test]
    fn the_list_tag_and_re_prefixes_are_stripped() {
        assert_eq!(normalise_subject("[ceph-users] Re: Re: Bad PG"), "Bad PG");
        assert_eq!(normalise_subject("AW: [ceph-users] Bad PG"), "Bad PG");
        assert_eq!(normalise_subject("  Bad PG  "), "Bad PG");
    }

    #[test]
    fn a_threads_version_moves_only_when_its_content_does() {
        let threads = thread_messages(parse_mbox(SAMPLE));
        let before = threads[0].version();
        assert_eq!(before, thread_messages(parse_mbox(SAMPLE))[0].version());

        let mut grown = threads[0].clone();
        grown.messages.push(Message {
            id: "reply3@example.com".into(),
            subject: "OSD flapping after upgrade".into(),
            from: "Someone <s@example.com>".into(),
            date: None,
            in_reply_to: None,
            body: "One more thing.".into(),
        });
        assert_ne!(
            before,
            grown.version(),
            "a new reply must re-index the thread"
        );
    }

    #[test]
    fn a_permalink_is_derived_from_the_message_id() {
        // Pinned against the live archive: this id really does resolve.
        assert_eq!(
            permalink(
                "https://lists.ceph.io/hyperkitty/list/ceph-users@ceph.io",
                "an69-D7z4cqxdSQx@hazard.jcu.cz"
            ),
            "https://lists.ceph.io/hyperkitty/list/ceph-users@ceph.io/message/\
             2JQNBF6477FUKCLBDJV5QCBAAXJUHCKP/"
        );
    }

    #[test]
    fn a_rendered_thread_carries_attribution_dates_and_links() {
        let threads = thread_messages(parse_mbox(SAMPLE));
        let doc = render_thread(
            &threads[0],
            "https://lists.ceph.io/hyperkitty/list/ceph-users@ceph.io",
        );
        assert!(doc.starts_with("# OSD flapping after upgrade"));
        assert!(doc.contains("Jane Roe <jane@example.com>"), "{doc}");
        assert!(
            doc.contains("2026-09-01"),
            "dates matter for version-specific advice"
        );
        assert!(doc.contains("/message/"), "every message is citable");
        assert!(
            doc.contains("osd_heartbeat_grace"),
            "the answer is in the document"
        );
        assert!(!doc.contains("unsubscribe"), "no footer noise: {doc}");
    }

    /// Run the real archive through the pipeline.
    ///
    /// Ignored by default: the corpus is 187 MB and does not belong in git.
    /// Point `CEPH_MBOX` at an unpacked HyperKitty export to run it. Kept
    /// because a synthetic fixture cannot tell you whether threading actually
    /// works on eight years of real mail clients.
    #[test]
    #[ignore = "needs CEPH_MBOX=<path to an unpacked mbox>"]
    fn the_real_archive_parses_threads_and_sheds_its_quoting() {
        let path = std::env::var("CEPH_MBOX").expect("CEPH_MBOX");
        let raw = std::fs::read(&path).expect("read mbox");
        let raw_len = raw.len();

        let messages = parse_mbox(&raw);
        let kept: usize = messages.iter().map(|m| m.body.len()).sum();
        let threads = thread_messages(messages.clone());
        let sizes: Vec<usize> = threads
            .iter()
            .map(|t| {
                render_thread(
                    t,
                    "https://lists.ceph.io/hyperkitty/list/ceph-users@ceph.io",
                )
                .len()
            })
            .collect();
        let total: usize = sizes.iter().sum();
        let mut sorted = sizes.clone();
        sorted.sort_unstable();
        let singletons = threads.iter().filter(|t| t.messages.len() == 1).count();

        println!("  raw mbox            {raw_len:>12}");
        println!("  messages parsed     {:>12}", messages.len());
        println!(
            "  body kept           {kept:>12}  ({}% of raw)",
            kept * 100 / raw_len
        );
        println!("  threads             {:>12}", threads.len());
        println!(
            "  singleton threads   {singletons:>12}  ({}%)",
            singletons * 100 / threads.len().max(1)
        );
        println!("  rendered corpus     {total:>12}");
        println!("  thread chars median {:>12}", sorted[sorted.len() / 2]);
        println!(
            "  thread chars p90    {:>12}",
            sorted[sorted.len() * 9 / 10]
        );
        println!("  thread chars max    {:>12}", sorted[sorted.len() - 1]);
        println!(
            "  fit in one 4k chunk {:>12}  ({}%)",
            sorted.iter().filter(|&&s| s <= 4000).count(),
            sorted.iter().filter(|&&s| s <= 4000).count() * 100 / sorted.len()
        );

        assert!(messages.len() > 20_000, "expected the full archive");
        // Threading must actually group. If every message became its own
        // thread the fallback is broken and retrieval loses every answer.
        assert!(
            threads.len() < messages.len() / 2,
            "threading collapsed: {} threads for {} messages",
            threads.len(),
            messages.len()
        );
        assert!(
            kept * 100 / raw_len < 40,
            "quote stripping is not working: kept {}% of the raw archive",
            kept * 100 / raw_len
        );
    }

    #[test]
    fn an_unparseable_message_is_skipped_rather_than_failing_the_batch() {
        let mut raw = b"From nobody\nnot a message at all\n".to_vec();
        raw.extend_from_slice(SAMPLE);
        let msgs = parse_mbox(&raw);
        assert_eq!(msgs.len(), 3, "the three good messages still come through");
    }
}
