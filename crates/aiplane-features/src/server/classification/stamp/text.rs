// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use aiplane_core::server::classification::{Level, Scheme};

/// The marking text as the first line, followed by a blank line. A first
/// line that is the marking text of any level is replaced, never stacked.
/// `None` when the level has no text or the file is not UTF-8.
pub(super) fn stamp(bytes: &[u8], level: &Level, scheme: &Scheme) -> Option<Vec<u8>> {
    if level.marking_text.is_empty() {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?;
    let body = match text.split_once('\n') {
        Some((first, rest))
            if scheme.levels().iter().any(|other| {
                !other.marking_text.is_empty() && first.trim() == other.marking_text
            }) =>
        {
            rest.strip_prefix('\n')
                .or_else(|| rest.strip_prefix("\r\n"))
                .unwrap_or(rest)
        }
        _ => text,
    };
    Some(format!("{}\n\n{body}", level.marking_text).into_bytes())
}
