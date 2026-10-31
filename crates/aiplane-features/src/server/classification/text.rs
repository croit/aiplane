// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The plain text of a file, for the detectors that read text: visible
//! stamps, patterns, word lists.

use aiplane_core::server::classification::{MarkingKind, MarkingRule};
use mail_parser::MessageParser;
use regex::RegexBuilder;

use super::markings::{extension, is_eml, normalized_mime};
use super::xml::append_text;
use super::zip_io::{self, MAX_PART_BYTES, Package};
use crate::server::pdf;
use crate::server::rag::extract::{DocKind, classify};
use crate::server::rag::source::mail::strip_html;

/// A visible stamp counts only near the top of a document.
pub const STAMP_WINDOW_BYTES: usize = 4096;

/// The text of `bytes`, at most `cap` bytes of it, or `None` when the format
/// has no text this reads (images, a scanned PDF, an unknown binary).
///
/// Plain text, Markdown, CSV, JSON and HTML (tags stripped) are decoded as
/// UTF-8. Word, Excel and PowerPoint packages and OpenDocument files give
/// the text of their XML parts, headers first so a header stamp is in the
/// stamp window. A PDF gives its text layer through
/// [`pdf::extract_text`](crate::server::pdf::extract_text). An e-mail gives
/// its subject and body.
pub fn detection_text(filename: &str, mime: &str, bytes: &[u8], cap: usize) -> Option<String> {
    let ext = extension(filename);
    let mime = normalized_mime(mime);
    let text = if is_eml(&ext, &mime) {
        eml_text(bytes)?
    } else if zip_io::looks_like_zip(bytes) {
        package_text(bytes, cap)?
    } else {
        match classify(filename, Some(&mime)) {
            DocKind::Pdf => pdf::extract_text(bytes).ok()?,
            DocKind::Image | DocKind::Office => return None,
            DocKind::Text | DocKind::Unknown => {
                let decoded = String::from_utf8(bytes.to_vec()).ok()?;
                if matches!(ext.as_str(), "html" | "htm") || mime == "text/html" {
                    strip_html(&decoded)
                } else {
                    decoded
                }
            }
        }
    };
    Some(truncated(text, cap))
}

fn truncated(mut text: String, cap: usize) -> String {
    if text.len() > cap {
        let mut end = cap;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    text
}

fn eml_text(bytes: &[u8]) -> Option<String> {
    let message = MessageParser::default().parse(bytes)?;
    let body = message
        .body_text(0)
        .map(|body| body.into_owned())
        .or_else(|| message.body_html(0).map(|html| strip_html(&html)))
        .unwrap_or_default();
    Some(format!(
        "{}\n\n{body}",
        message.subject().unwrap_or_default()
    ))
}

fn part_order(names: &[String]) -> Vec<String> {
    let mut ordered = Vec::new();
    let mut take = |matches: &dyn Fn(&str) -> bool| {
        let mut group: Vec<&String> = names.iter().filter(|name| matches(name)).collect();
        group.sort_by_key(|name| (name.len(), name.as_str()));
        ordered.extend(group.into_iter().cloned());
    };
    take(&|n| n.starts_with("word/header") && n.ends_with(".xml"));
    take(&|n| n == "word/document.xml");
    take(&|n| n.starts_with("word/footer") && n.ends_with(".xml"));
    take(&|n| n.starts_with("ppt/slides/slide") && n.ends_with(".xml"));
    take(&|n| n == "xl/sharedStrings.xml");
    take(&|n| n == "styles.xml" || n == "content.xml");
    ordered
}

fn package_text(bytes: &[u8], cap: usize) -> Option<String> {
    let mut package: Package<'_> = zip_io::open(bytes)?;
    let names = zip_io::part_names(&package);
    let mut out = String::new();
    for name in part_order(&names) {
        if out.len() >= cap {
            break;
        }
        if let Ok(Some(xml)) = zip_io::read_part(&mut package, &name, MAX_PART_BYTES) {
            append_text(&xml, &mut out, cap);
        }
    }
    Some(out)
}

/// The levels whose configured stamp text appears, case-insensitively and on
/// word boundaries, in the first [`STAMP_WINDOW_BYTES`] of `text`. A level is
/// listed once; `rules` of other kinds are ignored.
pub fn stamp_matches(text: &str, rules: &[MarkingRule]) -> Vec<String> {
    let window = truncated(text.to_string(), STAMP_WINDOW_BYTES);
    let mut levels: Vec<String> = Vec::new();
    for rule in rules.iter().filter(|rule| rule.kind == MarkingKind::Stamp) {
        let stamp = rule.value.trim();
        if stamp.is_empty() || levels.contains(&rule.level_id) {
            continue;
        }
        let pattern = format!(
            r"(?:^|[^\p{{L}}\p{{N}}_]){}(?:$|[^\p{{L}}\p{{N}}_])",
            regex::escape(stamp)
        );
        let hit = RegexBuilder::new(&pattern)
            .case_insensitive(true)
            .build()
            .is_ok_and(|re| re.is_match(&window));
        if hit {
            levels.push(rule.level_id.clone());
        }
    }
    levels
}
