// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use std::sync::LazyLock;

use aiplane_core::server::classification::Level;
use regex::Regex;

use super::{StampOptions, xml_escape};

static OLD_META: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<meta\s+name="classification"[^>]*>\s*"#).expect("static pattern")
});
static OLD_BANNER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?is)<div\s+data-classification-banner[^>]*>.*?</div>\s*").expect("static pattern")
});
static BODY_OPEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<body\b[^>]*>").expect("static pattern"));
static HEAD_CLOSE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)</head\s*>").expect("static pattern"));

/// `<meta name="classification">` before `</head>` (metadata) and a banner
/// element at the start of the body (visible). A marking written earlier is
/// replaced. `None` when the file is not UTF-8 or nothing is to be written.
pub(super) fn stamp(bytes: &[u8], level: &Level, options: &StampOptions) -> Option<Vec<u8>> {
    let write_meta = options.metadata;
    let write_banner = options.visible && !level.marking_text.is_empty();
    if !write_meta && !write_banner {
        return None;
    }
    let html = std::str::from_utf8(bytes).ok()?;
    let mut html = OLD_BANNER
        .replace_all(&OLD_META.replace_all(html, ""), "")
        .into_owned();

    if write_meta && let Some(head) = HEAD_CLOSE.find(&html) {
        let meta = format!(
            "<meta name=\"classification\" content=\"{}\">\n",
            xml_escape(&level.name)
        );
        html.insert_str(head.start(), &meta);
    }
    if write_banner {
        let banner = format!(
            "<div data-classification-banner role=\"note\" style=\"font-weight:bold;text-align:center\">{}</div>\n",
            xml_escape(&level.marking_text)
        );
        let at = BODY_OPEN.find(&html).map_or(0, |body| body.end());
        html.insert_str(at, &banner);
    }
    Some(html.into_bytes())
}
