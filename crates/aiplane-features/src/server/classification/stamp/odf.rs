// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

use std::collections::HashMap;
use std::sync::LazyLock;

use aiplane_core::server::classification::Level;
use regex::Regex;

use super::xml_escape;
use crate::server::classification::StampError;
use crate::server::classification::markings::TSCP_CATEGORY_NAME;
use crate::server::classification::zip_io::{self, MAX_PART_BYTES};

const TSCP_CATEGORY_IDENTIFIER: &str =
    "urn:bails:IntellectualProperty:BusinessAuthorizationCategory:Identifier";

static OLD_PROPERTIES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r#"(?s)<meta:user-defined\b[^>]*meta:name="(?:{}|{})"[^>]*>.*?</meta:user-defined>"#,
        regex::escape(TSCP_CATEGORY_NAME),
        regex::escape(TSCP_CATEGORY_IDENTIFIER)
    ))
    .expect("static pattern")
});

fn failure(reason: &str) -> StampError {
    StampError::Metadata {
        part: "meta.xml".to_string(),
        reason: reason.to_string(),
    }
}

/// TSCP business authorization category in `meta.xml`: the level's name and
/// id, replacing the ones already there.
pub(super) fn stamp(bytes: &[u8], level: &Level) -> Result<Option<Vec<u8>>, StampError> {
    let mut package = zip_io::open(bytes).ok_or_else(|| failure("the package cannot be opened"))?;
    let meta = zip_io::read_part(&mut package, "meta.xml", MAX_PART_BYTES)
        .map_err(|_| failure("the part is damaged or too large"))?
        .ok_or_else(|| failure("the package has no meta.xml"))?;
    let meta = String::from_utf8(meta).map_err(|_| failure("the part is not UTF-8"))?;

    let mut meta = OLD_PROPERTIES.replace_all(&meta, "").into_owned();
    let properties = format!(
        "<meta:user-defined meta:name=\"{TSCP_CATEGORY_NAME}\">{}</meta:user-defined>\
         <meta:user-defined meta:name=\"{TSCP_CATEGORY_IDENTIFIER}\">{}</meta:user-defined>",
        xml_escape(&level.name),
        xml_escape(&level.id)
    );
    if let Some(close) = meta.rfind("</office:meta>") {
        meta.insert_str(close, &properties);
    } else if let Some(empty) = meta.find("<office:meta/>") {
        meta.replace_range(
            empty..empty + "<office:meta/>".len(),
            &format!("<office:meta>{properties}</office:meta>"),
        );
    } else {
        return Err(failure("the part has no office:meta element"));
    }
    let replace = HashMap::from([("meta.xml".to_string(), meta.into_bytes())]);
    zip_io::rewrite(bytes, &replace, &[]).map(Some)
}
