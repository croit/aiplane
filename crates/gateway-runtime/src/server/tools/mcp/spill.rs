// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Spill file bytes out of an MCP tool result and into the conversation's
//! attachment store.
//!
//! MCP servers hand files back the only way the protocol lets them: base64,
//! inline, in the tool result. A single PDF invoice on a Gmail message is a
//! megabyte of text in the model's context — money, latency and, well past
//! that, a context window with no room left for the actual work. And it buys
//! nothing: a model cannot *do* anything with base64 except copy it somewhere,
//! which costs the same bytes again on the way out.
//!
//! So the gateway intercepts it. Every base64 payload above
//! [`MIN_SPILL_CHARS`] in a tool result is decoded, stored as an ordinary chat
//! attachment (same bucket, same `<turn_id>/<filename>` ids, same chip in the
//! reply as an upload), and replaced in the result with one line naming the
//! id. The model then reads it with `fetch_attachment`, stages it into
//! `run_in_sandbox`, renders it into a document, or hands it to the user with
//! `offer_download` — all by reference, at a few dozen tokens.
//!
//! This is deliberately generic rather than a Gmail special case. Two shapes
//! turn up in practice and both are covered:
//!
//! - **A labelled block**, e.g. the Google Workspace server's
//!   `get_gmail_attachment_content` (`📦 Base64 content (…): <one huge line>`).
//! - **Raw MIME**, e.g. the same server's `get_gmail_message_content` with
//!   `body_format="raw"`, where every attachment is a base64 part wrapped at
//!   76 columns, preceded by its own `Content-Type` / `filename=` headers.
//!
//! The MIME headers are why the extractor looks *backwards* from a payload for
//! a filename and a content type: that is where a real name for the file is,
//! and a spilled artifact called `invoice.pdf` is worth a great deal more to
//! both model and user than `gmail-attachment-1.bin`.
//!
//! Nothing here can lose data silently. When the conversation has nowhere to
//! store an attachment (the `/v1` proxy paths have no turn to attach to) or
//! the payload is too large, the base64 is still removed — putting it back
//! would be the bug this module exists to fix — and the replacement line says
//! exactly why, so the model can tell the user instead of hallucinating a
//! file.

use gateway_features::server::chat_attachments::{self, UploadOutcome};
use rmcp::model::{CallToolResult, Content, RawContent};
use serde_json::Value;
use session_core::db as chat;

use super::super::ToolContext;

/// Shortest base64 run treated as a file rather than as text: 4 KiB of
/// base64, about 3 KiB of bytes.
///
/// Well below anything worth calling an attachment, and well above the
/// incidental base64 that turns up *as data* in a tool result — an ETag, a
/// cursor, a signature, a 32-byte key. Spilling one of those would cost the
/// model the value it was about to read.
const MIN_SPILL_CHARS: usize = 4096;

/// Ceiling on one decoded payload. Matches the image ceiling the fetch tools
/// use, so "too big to hand you" means the same size everywhere.
const MAX_SPILL_BYTES: usize = 25 * 1024 * 1024;

/// Most artifacts one tool call may produce. A mail with forty inline images
/// would otherwise fill the reply with forty chips (and do forty uploads);
/// past this the rest are dropped with a note rather than stored.
const MAX_SPILLS_PER_CALL: usize = 10;

/// How far back from a payload to look for the headers that name it.
const METADATA_LOOKBEHIND: usize = 2048;

/// One base64 payload lifted out of a text block, with whatever the
/// surrounding text said about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Payload {
    /// The base64 itself, line breaks removed.
    pub data: String,
    pub filename: Option<String>,
    pub mime: Option<String>,
}

/// A text block after extraction: prose interleaved with the payloads that
/// were carved out of it, in the order they appeared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Chunk {
    Text(String),
    Payload(Payload),
}

/// Split a text block into prose and base64 payloads.
///
/// Pure, and the interesting half of this module. A line qualifies as base64
/// only if it is non-empty, contains no whitespace at all, and is made
/// entirely of alphabet characters — which by itself rejects essentially all
/// prose, because English has spaces. Consecutive qualifying lines form one
/// run (that is how MIME wraps a part at 76 columns); the run is spilled only
/// if it is long enough to be a file and its length is a multiple of four, as
/// real base64 always is.
pub(crate) fn split_base64(text: &str) -> Vec<Chunk> {
    let mut chunks: Vec<Chunk> = Vec::new();
    let mut prose = String::new();
    // Each pending line as (verbatim, alphabet-only) — the verbatim copy is
    // what goes back into the prose if the run turns out not to be a file, so
    // a false positive costs the text nothing, not even its line endings.
    let mut run: Vec<(&str, &str)> = Vec::new();

    fn flush(run: &mut Vec<(&str, &str)>, prose: &mut String, chunks: &mut Vec<Chunk>) {
        if run.is_empty() {
            return;
        }
        let joined: String = run.iter().map(|(_, b64)| *b64).collect();
        if joined.len() >= MIN_SPILL_CHARS && joined.len().is_multiple_of(4) {
            let filename = scrape(prose, &["filename", "name"]);
            let mime = scrape_mime(prose);
            chunks.push(Chunk::Text(std::mem::take(prose)));
            chunks.push(Chunk::Payload(Payload {
                data: joined,
                filename,
                mime,
            }));
        } else {
            for (verbatim, _) in run.iter() {
                prose.push_str(verbatim);
            }
        }
        run.clear();
    }

    // `split_inclusive` keeps each line's terminator and yields no phantom
    // empty line at the end, so prose reassembles byte-for-byte.
    for verbatim in text.split_inclusive('\n') {
        let candidate = verbatim.trim_end_matches('\n').trim_end_matches('\r');
        if is_base64_line(candidate) {
            run.push((verbatim, candidate));
        } else {
            flush(&mut run, &mut prose, &mut chunks);
            prose.push_str(verbatim);
        }
    }
    flush(&mut run, &mut prose, &mut chunks);
    if !prose.is_empty() {
        chunks.push(Chunk::Text(prose));
    }
    chunks
}

/// Whether one line is nothing but base64 characters.
///
/// Both alphabets are accepted: standard (`+/`) and URL-safe (`-_`), because
/// Google's APIs return the latter and a server may pass it through verbatim.
fn is_base64_line(line: &str) -> bool {
    // A short line is never worth treating as part of a file, and requiring
    // some length here keeps single words ("Attachment") out of a run.
    if line.len() < 60 {
        return false;
    }
    line.bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'=' | b'-' | b'_'))
}

/// Pull a quoted (or bare) value for one of `keys` out of the tail of the
/// preceding text — `filename="invoice.pdf"`, `Filename: invoice.pdf`.
/// The *last* match wins: it is the one closest to the payload.
fn scrape(before: &str, keys: &[&str]) -> Option<String> {
    let tail = tail_of(before);
    let lower = tail.to_ascii_lowercase();
    let mut best: Option<(usize, String)> = None;
    for key in keys {
        let mut from = 0;
        while let Some(found) = lower[from..].find(key) {
            let at = from + found;
            from = at + key.len();
            // Must be a whole word: `filename` yes, `myfilename` no.
            if at > 0 && lower.as_bytes()[at - 1].is_ascii_alphanumeric() {
                continue;
            }
            let Some(value) = value_after(&tail[at + key.len()..]) else {
                continue;
            };
            if value.is_empty() || value.len() > 200 {
                continue;
            }
            if best.as_ref().is_none_or(|(prev, _)| at >= *prev) {
                best = Some((at, value));
            }
        }
    }
    best.map(|(_, v)| v)
}

/// The value part of `= "x"` / `=x` / `: x`, up to the delimiter.
fn value_after(rest: &str) -> Option<String> {
    let rest = rest.trim_start();
    let rest = rest.strip_prefix('=').or_else(|| rest.strip_prefix(':'))?;
    let rest = rest.trim_start();
    if let Some(quoted) = rest.strip_prefix('"') {
        return quoted.find('"').map(|end| quoted[..end].to_string());
    }
    Some(
        rest.split([';', '\r', '\n'])
            .next()
            .unwrap_or("")
            .trim()
            .to_string(),
    )
}

/// The `Content-Type:` (or `mimeType`) nearest above a payload, media type
/// only — parameters like `charset` or `name=` are dropped.
fn scrape_mime(before: &str) -> Option<String> {
    let tail = tail_of(before);
    let lower = tail.to_ascii_lowercase();
    let at = lower
        .rfind("content-type")
        .or_else(|| lower.rfind("mimetype"))
        .or_else(|| lower.rfind("mime_type"))?;
    let rest = &tail[at..];
    let rest = rest.split_once([':', '='])?.1;
    let value = rest
        .split([';', '\r', '\n', ','])
        .next()?
        .trim()
        .trim_matches('"')
        .to_ascii_lowercase();
    // A media type, not a sentence that happened to follow the word.
    let looks_like_mime = value.contains('/')
        && !value.contains(' ')
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'+' | b'.' | b'_'));
    looks_like_mime.then_some(value)
}

/// The last [`METADATA_LOOKBEHIND`] bytes of `text`, cut on a char boundary.
fn tail_of(text: &str) -> &str {
    if text.len() <= METADATA_LOOKBEHIND {
        return text;
    }
    let mut start = text.len() - METADATA_LOOKBEHIND;
    while start < text.len() && !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..]
}

/// What became of one payload. Both variants replace the base64 in the result
/// with prose — the model is never handed the bytes back.
enum Spilled {
    Stored(UploadOutcome, String),
    Refused(String),
}

impl Spilled {
    /// The line that takes the payload's place in the tool result.
    fn line(&self) -> String {
        match self {
            Spilled::Stored(att, id) => format!(
                "[the {} of file bytes here were stored as a conversation artifact \
                 and removed from this result — filename=\"{}\" mime=\"{}\" size={} \
                 id=\"{}\". Use it BY ID: `fetch_attachment` reads it, `run_in_sandbox` \
                 stages it into /work, `offer_download` hands it to the user, a \
                 `typst_*` render embeds it as `att:{}`. It is already shown in your \
                 reply — do not describe or repeat this line.]",
                human_size(att.bytes),
                att.filename,
                att.mime,
                att.bytes,
                id,
                id,
            ),
            Spilled::Refused(why) => format!(
                "[file bytes were removed from this result and NOT stored: {why}. \
                 Tell the user rather than pretending you have the file.]"
            ),
        }
    }
}

fn human_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} bytes")
    }
}

/// Rewrite a tool result so no large base64 payload survives in it.
///
/// Walks the text blocks, the blob half of any embedded resource, and every
/// string leaf of `structured_content` — a server is free to put the same
/// payload in any of the three.
pub(crate) async fn spill_payloads(
    server: &str,
    tool: &str,
    mut res: CallToolResult,
    ctx: &ToolContext,
) -> CallToolResult {
    let mut budget = MAX_SPILLS_PER_CALL;
    let mut content: Vec<Content> = Vec::with_capacity(res.content.len());
    for block in std::mem::take(&mut res.content) {
        match &*block {
            RawContent::Text(text) => {
                let chunks = split_base64(&text.text);
                if chunks.iter().all(|c| matches!(c, Chunk::Text(_))) {
                    content.push(block);
                    continue;
                }
                let mut rebuilt = String::new();
                for chunk in chunks {
                    match chunk {
                        Chunk::Text(t) => rebuilt.push_str(&t),
                        Chunk::Payload(p) => {
                            rebuilt
                                .push_str(&handle(server, tool, p, ctx, &mut budget).await.line());
                            rebuilt.push('\n');
                        }
                    }
                }
                content.push(Content::text(rebuilt));
            }
            RawContent::Resource(resource) => {
                let rmcp::model::ResourceContents::BlobResourceContents {
                    blob,
                    mime_type,
                    uri,
                    ..
                } = &resource.resource
                else {
                    content.push(block);
                    continue;
                };
                if blob.len() < MIN_SPILL_CHARS {
                    content.push(block);
                    continue;
                }
                let payload = Payload {
                    data: blob.clone(),
                    filename: filename_from_uri(uri),
                    mime: mime_type.clone(),
                };
                content.push(Content::text(
                    handle(server, tool, payload, ctx, &mut budget).await.line(),
                ));
            }
            _ => content.push(block),
        }
    }
    res.content = content;

    if let Some(structured) = res.structured_content.take() {
        res.structured_content = Some(spill_json(server, tool, structured, ctx, &mut budget).await);
    }
    res
}

/// Same treatment for every string leaf of a structured result.
async fn spill_json(
    server: &str,
    tool: &str,
    value: Value,
    ctx: &ToolContext,
    budget: &mut usize,
) -> Value {
    match value {
        Value::String(s) => {
            let chunks = split_base64(&s);
            if chunks.iter().all(|c| matches!(c, Chunk::Text(_))) {
                return Value::String(s);
            }
            let mut rebuilt = String::new();
            for chunk in chunks {
                match chunk {
                    Chunk::Text(t) => rebuilt.push_str(&t),
                    Chunk::Payload(p) => {
                        rebuilt.push_str(&handle(server, tool, p, ctx, budget).await.line());
                        rebuilt.push('\n');
                    }
                }
            }
            Value::String(rebuilt)
        }
        Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(Box::pin(spill_json(server, tool, item, ctx, budget)).await);
            }
            Value::Array(out)
        }
        Value::Object(map) => {
            let mut out = serde_json::Map::with_capacity(map.len());
            for (k, v) in map {
                out.insert(k, Box::pin(spill_json(server, tool, v, ctx, budget)).await);
            }
            Value::Object(out)
        }
        other => other,
    }
}

/// Decode, check and store one payload.
async fn handle(
    server: &str,
    tool: &str,
    payload: Payload,
    ctx: &ToolContext,
    budget: &mut usize,
) -> Spilled {
    if *budget == 0 {
        return Spilled::Refused(format!(
            "this call already produced {MAX_SPILLS_PER_CALL} artifacts, the per-call \
             limit — ask for the remaining files one at a time"
        ));
    }
    // Both alphabets reach here; the decoder speaks the standard one.
    let normalised: String = payload
        .data
        .chars()
        .map(|c| match c {
            '-' => '+',
            '_' => '/',
            other => other,
        })
        .collect();
    let bytes = match chat_attachments::decode_base64(&normalised) {
        Ok(bytes) => bytes,
        Err(e) => return Spilled::Refused(format!("the base64 did not decode ({e})")),
    };
    if bytes.is_empty() {
        return Spilled::Refused("the payload decoded to zero bytes".into());
    }
    if bytes.len() > MAX_SPILL_BYTES {
        return Spilled::Refused(format!(
            "it is {} — over the {} ceiling for one artifact",
            human_size(bytes.len() as u64),
            human_size(MAX_SPILL_BYTES as u64)
        ));
    }

    let (Some(s3), Some(turn_id), Some(reservations)) = (
        ctx.s3.as_ref(),
        ctx.assistant_turn_id.as_ref(),
        ctx.attachment_reservations.as_ref(),
    ) else {
        return Spilled::Refused(
            "this request has no conversation to store files in (an API/proxy call, or \
             a gateway without [chat.s3] configured)"
                .into(),
        );
    };

    let mime = payload
        .mime
        .filter(|m| m.contains('/'))
        .or_else(|| sniff_mime(&bytes).map(str::to_string))
        .unwrap_or_else(|| "application/octet-stream".to_string());
    let base = desired_filename(server, tool, payload.filename.as_deref(), &mime);
    let filename =
        match chat_attachments::reserve_filename(&ctx.db, turn_id, reservations, &base).await {
            Ok(name) => name,
            Err(e) => return Spilled::Refused(format!("a filename could not be reserved ({e})")),
        };

    match chat_attachments::upload(s3, turn_id, &filename, &mime, bytes).await {
        Ok(outcome) => {
            // Same splice `upload_attachment` does: the marker is what makes
            // the file real — it renders the chip AND is the only record that
            // ties the id to this session, so `fetch_attachment` and the
            // sandbox will accept it.
            let marker = chat_attachments::marker_line(turn_id, &outcome);
            if let Err(e) =
                chat::append_content(&ctx.db, turn_id, &format!("\n\n{marker}\n\n")).await
            {
                return Spilled::Refused(format!("the artifact could not be recorded ({e})"));
            }
            *budget -= 1;
            let id = format!("{turn_id}/{}", outcome.filename);
            Spilled::Stored(outcome, id)
        }
        Err(e) => Spilled::Refused(format!("the upload failed ({e})")),
    }
}

/// A safe, meaningful filename for a spilled payload.
fn desired_filename(server: &str, tool: &str, given: Option<&str>, mime: &str) -> String {
    if let Some(name) = given.map(sanitize_filename).filter(|n| !n.is_empty()) {
        // A name with no extension gets one from the mime, so downstream
        // tooling (and the user's browser) knows what it is.
        if name.contains('.') {
            return name;
        }
        return format!("{name}{}", ext_for(mime));
    }
    let stem = sanitize_filename(&format!("{server}-{tool}"));
    let stem = if stem.is_empty() {
        "mcp-file".to_string()
    } else {
        stem
    };
    format!("{stem}{}", ext_for(mime))
}

/// Keep a filename to a predictable charset, and to its last path segment —
/// a MIME header may carry `../` or a full path, and the storage layer
/// rejects a `/` outright.
fn sanitize_filename(raw: &str) -> String {
    let base = raw.rsplit(['/', '\\']).next().unwrap_or(raw);
    let mut out = String::with_capacity(base.len());
    let mut prev_dash = false;
    for ch in base.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-') {
            out.push(ch);
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    out.trim_matches(['-', '.']).chars().take(80).collect()
}

/// Extension for a mime, falling back to `.bin` — the point is only that a
/// stored artifact has *some* honest extension.
fn ext_for(mime: &str) -> &'static str {
    if let Some(ext) = chat_attachments::ext_for_mime(mime) {
        return ext;
    }
    match mime {
        "application/pdf" => ".pdf",
        "text/plain" => ".txt",
        "text/csv" => ".csv",
        "text/html" => ".html",
        "application/json" => ".json",
        "application/zip" => ".zip",
        "message/rfc822" => ".eml",
        "application/msword" => ".doc",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => ".docx",
        "application/vnd.ms-excel" => ".xls",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => ".xlsx",
        "application/vnd.ms-powerpoint" => ".ppt",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation" => ".pptx",
        _ => ".bin",
    }
}

/// Last-resort type detection from the first bytes, for a payload whose
/// surroundings said nothing about it.
fn sniff_mime(bytes: &[u8]) -> Option<&'static str> {
    let head = &bytes[..bytes.len().min(16)];
    if head.starts_with(b"%PDF-") {
        return Some("application/pdf");
    }
    if head.starts_with(&[0x89, b'P', b'N', b'G']) {
        return Some("image/png");
    }
    if head.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }
    if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    if bytes.len() > 12 && head.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    if head.starts_with(b"PK\x03\x04") {
        // Every Office/OpenDocument file is a zip; without unpacking the
        // central directory this is as specific as it gets honestly.
        return Some("application/zip");
    }
    None
}

/// The trailing name of a resource URI, if it has one.
fn filename_from_uri(uri: &str) -> Option<String> {
    let trimmed = uri.split(['?', '#']).next().unwrap_or(uri);
    let last = trimmed.rsplit('/').next()?;
    (!last.is_empty() && last.contains('.')).then(|| last.to_string())
}

/// Default a server's "give me the bytes" switch to on.
///
/// Some MCP servers gate file bytes behind a boolean (the Google Workspace
/// server's `return_base64` on `get_gmail_attachment_content`) whose default
/// is off, because for an ordinary client the bytes would flood the context.
/// Here they never reach the context: they are spilled to an artifact above.
/// So a model that asks this gateway for an attachment's content should get
/// the attachment, not a 100-character preview it can do nothing with.
///
/// Only fills in an argument the model did not supply — an explicit `false`
/// stands.
pub(crate) fn default_blob_switches(schema: &Value, args: &mut Value) {
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return;
    };
    let Some(object) = args.as_object_mut() else {
        return;
    };
    for (name, spec) in properties {
        if !is_blob_switch(name, spec) || object.contains_key(name) {
            continue;
        }
        object.insert(name.clone(), Value::Bool(true));
    }
}

/// Whether a tool offers a "give me the bytes" switch at all — the cue to
/// tell the model, in the tool's own description, that asking for them here
/// is free.
pub(crate) fn has_blob_switch(schema: &Value) -> bool {
    schema
        .get("properties")
        .and_then(Value::as_object)
        .is_some_and(|properties| {
            properties
                .iter()
                .any(|(name, spec)| is_blob_switch(name, spec))
        })
}

/// Appended to such a tool's description. Without it the model has every
/// reason to leave the switch off — the servers' own docs warn that the
/// bytes flood the context, which on this gateway they cannot.
pub(crate) const BLOB_SWITCH_NOTE: &str = " This gateway never puts returned file bytes in your context: they are stored as a conversation artifact and you get its id back, so ask for the content (the bytes parameter defaults to on here) and then work with the file by id — `fetch_attachment` reads it, `run_in_sandbox` stages it, `offer_download` hands it to the user.";

/// A boolean parameter that asks for raw bytes back.
fn is_blob_switch(name: &str, spec: &Value) -> bool {
    let boolean = spec.get("type").and_then(Value::as_str) == Some("boolean");
    boolean && matches!(name, "return_base64" | "return_bytes" | "include_base64")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `n` chars of valid base64 (length kept a multiple of 4).
    fn b64(n: usize) -> String {
        "QUJDRA".chars().cycle().take(n / 4 * 4).collect()
    }

    #[test]
    fn prose_is_left_alone() {
        let text = "Subject: hello\n\nA perfectly ordinary mail body.\n";
        assert_eq!(split_base64(text), vec![Chunk::Text(text.to_string())]);
    }

    #[test]
    fn short_base64_is_data_not_a_file() {
        // An ETag, a cursor, a signature: the model is meant to read these.
        let text = format!("next_page_token: {}\n", b64(200));
        assert!(
            split_base64(&text)
                .iter()
                .all(|c| matches!(c, Chunk::Text(_)))
        );
    }

    #[test]
    fn a_long_single_line_payload_is_lifted_out() {
        let text = format!(
            "Attachment downloaded successfully!\nFilename: invoice.pdf\n\n\
             📦 Base64 content (99 chars, standard base64):\n{}\n\nDone.\n",
            b64(9000)
        );
        let chunks = split_base64(&text);
        let payloads: Vec<&Payload> = chunks
            .iter()
            .filter_map(|c| match c {
                Chunk::Payload(p) => Some(p),
                _ => None,
            })
            .collect();
        assert_eq!(payloads.len(), 1);
        assert_eq!(payloads[0].filename.as_deref(), Some("invoice.pdf"));
        // The prose survives, the payload does not.
        let prose: String = chunks
            .iter()
            .filter_map(|c| match c {
                Chunk::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect();
        assert!(prose.contains("Attachment downloaded successfully!"));
        assert!(prose.contains("Done."));
        assert!(!prose.contains(&b64(9000)));
    }

    #[test]
    fn a_wrapped_mime_part_is_one_payload_with_its_headers() {
        let wrapped: String = b64(8000)
            .as_bytes()
            .chunks(76)
            .map(|c| format!("{}\r\n", std::str::from_utf8(c).unwrap()))
            .collect();
        let text = format!(
            "--boundary\r\nContent-Type: application/pdf; name=\"report.pdf\"\r\n\
             Content-Transfer-Encoding: base64\r\n\
             Content-Disposition: attachment; filename=\"report.pdf\"\r\n\r\n\
             {wrapped}--boundary--\r\n"
        );
        let payloads: Vec<Payload> = split_base64(&text)
            .into_iter()
            .filter_map(|c| match c {
                Chunk::Payload(p) => Some(p),
                _ => None,
            })
            .collect();
        assert_eq!(
            payloads.len(),
            1,
            "the wrapped lines are one file, not many"
        );
        assert_eq!(payloads[0].filename.as_deref(), Some("report.pdf"));
        assert_eq!(payloads[0].mime.as_deref(), Some("application/pdf"));
        // Re-joined without the line breaks, so it decodes.
        assert!(chat_attachments::decode_base64(&payloads[0].data).is_ok());
    }

    #[test]
    fn two_parts_in_one_message_stay_separate() {
        let text = format!(
            "Content-Type: image/png; name=\"a.png\"\n\n{}\n\n\
             Content-Type: text/csv; name=\"b.csv\"\n\n{}\n",
            b64(6000),
            b64(7000)
        );
        let payloads: Vec<Payload> = split_base64(&text)
            .into_iter()
            .filter_map(|c| match c {
                Chunk::Payload(p) => Some(p),
                _ => None,
            })
            .collect();
        assert_eq!(payloads.len(), 2);
        assert_eq!(payloads[0].filename.as_deref(), Some("a.png"));
        assert_eq!(payloads[0].mime.as_deref(), Some("image/png"));
        assert_eq!(payloads[1].filename.as_deref(), Some("b.csv"));
        assert_eq!(payloads[1].mime.as_deref(), Some("text/csv"));
    }

    #[test]
    fn a_json_blob_field_is_not_mistaken_for_prose() {
        // A payload with no headers anywhere near it still gets lifted.
        let text = format!("{{\"data\":\"{}\"}}", b64(9000));
        // The quotes make the line non-base64 as a whole, so this one stays —
        // the JSON case is handled by walking `structured_content` instead.
        assert!(
            split_base64(&text)
                .iter()
                .all(|c| matches!(c, Chunk::Text(_)))
        );
    }

    #[test]
    fn filenames_are_made_safe() {
        assert_eq!(sanitize_filename("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_filename("in voice (1).pdf"), "in-voice-1-.pdf");
        assert_eq!(sanitize_filename("///"), "");
    }

    #[test]
    fn a_nameless_payload_still_gets_an_honest_name() {
        assert_eq!(
            desired_filename(
                "google_workspace",
                "get_gmail_attachment_content",
                None,
                "application/pdf"
            ),
            "google_workspace-get_gmail_attachment_content.pdf"
        );
        assert_eq!(
            desired_filename("gmail", "x", Some("scan"), "image/png"),
            "scan.png"
        );
        assert_eq!(
            desired_filename("gmail", "x", Some("keep.this.name.pdf"), "application/pdf"),
            "keep.this.name.pdf"
        );
    }

    #[test]
    fn types_are_sniffed_when_nothing_declared_one() {
        assert_eq!(sniff_mime(b"%PDF-1.7\n..."), Some("application/pdf"));
        assert_eq!(sniff_mime(b"\x89PNG\r\n\x1a\n"), Some("image/png"));
        assert_eq!(sniff_mime(b"not a known header"), None);
    }

    #[test]
    fn the_bytes_switch_defaults_on_only_when_unset() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "message_id": { "type": "string" },
                "return_base64": { "type": "boolean" }
            }
        });

        let mut args = serde_json::json!({ "message_id": "m1" });
        default_blob_switches(&schema, &mut args);
        assert_eq!(args["return_base64"], Value::Bool(true));

        // An explicit refusal is the model's to make.
        let mut args = serde_json::json!({ "message_id": "m1", "return_base64": false });
        default_blob_switches(&schema, &mut args);
        assert_eq!(args["return_base64"], Value::Bool(false));

        // Nothing is invented for a tool that has no such switch.
        let plain =
            serde_json::json!({ "type": "object", "properties": { "q": { "type": "string" } } });
        let mut args = serde_json::json!({ "q": "hi" });
        default_blob_switches(&plain, &mut args);
        assert_eq!(args, serde_json::json!({ "q": "hi" }));
    }

    #[tokio::test]
    async fn a_result_never_carries_the_bytes_out_even_when_nothing_can_store_them() {
        // The proxy paths have no turn to attach to. The base64 must still go:
        // handing it back would be the very bug this module exists to fix.
        let pool = gateway_core::server::db::open(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        let ctx = ToolContext::for_test(pool);
        let payload = b64(9000);
        let mut res = CallToolResult::default();
        res.content = vec![Content::text(format!(
            "Filename: invoice.pdf\nContent-Type: application/pdf\n\n{payload}\n"
        ))];

        let out = spill_payloads(
            "google_workspace",
            "get_gmail_attachment_content",
            res,
            &ctx,
        )
        .await;
        let text = out
            .content
            .iter()
            .filter_map(|c| c.as_text().map(|t| t.text.clone()))
            .collect::<String>();
        assert!(
            !text.contains(&payload),
            "the base64 survived into the result"
        );
        assert!(
            text.contains("NOT stored"),
            "the model is not told why: {text}"
        );
        assert!(
            text.contains("invoice.pdf") || text.contains("Filename"),
            "{text}"
        );
    }

    #[tokio::test]
    async fn a_result_with_no_payload_is_returned_untouched() {
        let pool = gateway_core::server::db::open(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        let ctx = ToolContext::for_test(pool);
        let mut res = CallToolResult::default();
        res.content = vec![Content::text("From: a@example.com\nSubject: hi\n")];

        let out = spill_payloads("s", "t", res, &ctx).await;
        assert_eq!(
            out.content[0].as_text().unwrap().text,
            "From: a@example.com\nSubject: hi\n"
        );
    }

    #[tokio::test]
    async fn a_spilled_artifact_is_reachable_by_the_id_it_advertises() {
        // The invariant the whole feature rests on, and the one that fails
        // silently: an attachment exists in a session only because a marker
        // sits in a turn's content. Splice the wrong thing (or nothing) and
        // the upload succeeds, the model gets an id, and every later
        // `fetch_attachment` / sandbox staging rejects it as not belonging to
        // the session. S3 is not involved in that half — the marker is — so
        // this proves it without a bucket.
        use gateway_features::server::chat_attachments as att;

        let pool = gateway_core::server::db::open(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        sqlx::query(
            r#"INSERT INTO users (id, email, created_at, updated_at)
               VALUES ('u', 'u@example.com', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"#,
        )
        .execute(&pool)
        .await
        .unwrap();
        let session = chat::create_session(&pool, "u").await.unwrap();
        chat::create_user_turn(&pool, &session.id, "u1", "read my mail")
            .await
            .unwrap();
        chat::create_assistant_turn_in_progress(&pool, &session.id, "a1", "m")
            .await
            .unwrap();

        let outcome = UploadOutcome {
            filename: "invoice.pdf".into(),
            mime: "application/pdf".into(),
            bytes: 1234,
        };
        let marker = att::marker_line("a1", &outcome);
        chat::append_content(&pool, "a1", &format!("\n\n{marker}\n\n"))
            .await
            .unwrap();

        let advertised = format!("a1/{}", outcome.filename);
        let found = att::list_session_attachments(&pool, &session.id)
            .await
            .unwrap();
        assert!(
            found.iter().any(|a| a.id == advertised),
            "the id handed to the model is not in the session: {found:?}"
        );
        assert!(att::attachment_in_session(&found, &advertised));
    }

    #[tokio::test]
    async fn a_payload_hidden_in_structured_content_is_spilled_too() {
        let pool = gateway_core::server::db::open(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        let ctx = ToolContext::for_test(pool);
        let payload = b64(9000);
        let mut res = CallToolResult::default();
        res.structured_content = Some(serde_json::json!({
            "attachments": [{ "name": "scan.png", "data": payload.clone() }]
        }));

        let out = spill_payloads("s", "t", res, &ctx).await;
        let rendered = out.structured_content.unwrap().to_string();
        assert!(
            !rendered.contains(&payload),
            "the base64 survived: {rendered}"
        );
    }

    #[test]
    fn a_blob_switch_is_advertised_in_the_description() {
        let with = serde_json::json!({
            "type": "object",
            "properties": { "return_base64": { "type": "boolean" } }
        });
        let without = serde_json::json!({
            "type": "object",
            "properties": { "return_base64": { "type": "string" } }
        });
        assert!(has_blob_switch(&with));
        assert!(!has_blob_switch(&without));
    }

    #[test]
    fn a_refusal_names_the_reason_and_never_returns_the_bytes() {
        let line = Spilled::Refused("it is 40.0 MB — over the ceiling".into()).line();
        assert!(line.contains("NOT stored"));
        assert!(line.contains("over the ceiling"));
    }

    #[test]
    fn a_stored_payload_is_advertised_by_id() {
        let line = Spilled::Stored(
            UploadOutcome {
                filename: "invoice.pdf".into(),
                mime: "application/pdf".into(),
                bytes: 84_000,
            },
            "turn-1/invoice.pdf".into(),
        )
        .line();
        assert!(line.contains("id=\"turn-1/invoice.pdf\""));
        assert!(line.contains("fetch_attachment"));
        assert!(line.contains("82.0 KB"));
    }
}
