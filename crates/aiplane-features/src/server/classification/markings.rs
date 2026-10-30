// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Reading the digital markings a file carries.
//!
//! | Format | Where |
//! | --- | --- |
//! | OOXML (docx, xlsx, pptx, macro and template variants) | `docProps/custom.xml` (`MSIP_Label_<GUID>_*`, TSCP), `docMetadata/LabelInfo.xml` |
//! | OpenDocument (odt, ods, odp, templates) | `meta.xml` TSCP user-defined property |
//! | PDF | the last uncompressed XMP packet (`MSIP_Label_<GUID>_*`) |
//! | E-mail | `Sensitivity` and `msip_labels` headers |
//!
//! Visible stamps are not read here: they are text, found in the extracted
//! text by [`stamp_matches`](super::stamp_matches).
//!
//! A PDF whose XMP lives in a compressed stream is read as having no label.
//! Compressed XMP is not supported: it is rare for a labelled PDF, and
//! inflating arbitrary PDF streams is the part of a PDF reader that needs the
//! most hardening.

use std::collections::BTreeMap;

use aiplane_core::server::classification::MarkingKind;
use mail_parser::MessageParser;

use super::xml::{Element, elements};
use super::zip_io::{self, MAX_PART_BYTES, Package, PartError};

pub const TSCP_CATEGORY_NAME: &str =
    "urn:bails:IntellectualProperty:BusinessAuthorizationCategory:Name";

const MSIP_PREFIX: &str = "MSIP_Label_";

/// One marking found in a file. For a Purview label `value` is the label GUID
/// (lower case, no braces) and `name` the label's name when the file carries
/// one; for TSCP and `Sensitivity` it is the marking's text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marking {
    pub kind: MarkingKind,
    pub value: String,
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Markings {
    pub found: Vec<Marking>,
    /// The file may carry a marking that could not be read: it is encrypted,
    /// damaged or too large to inspect.
    pub unreadable: bool,
}

const OOXML_EXTENSIONS: &[&str] = &[
    "docx", "docm", "dotx", "dotm", "xlsx", "xlsm", "xltx", "xltm", "pptx", "pptm", "potx", "potm",
    "ppsx", "ppsm",
];
const ODF_EXTENSIONS: &[&str] = &["odt", "ods", "odp", "ott", "ots", "otp"];
const LEGACY_OFFICE_EXTENSIONS: &[&str] = &["doc", "xls", "ppt"];

pub(super) fn extension(filename: &str) -> String {
    filename
        .rsplit('/')
        .next()
        .unwrap_or(filename)
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default()
}

pub(super) fn is_office_extension(ext: &str) -> bool {
    OOXML_EXTENSIONS.contains(&ext)
        || ODF_EXTENSIONS.contains(&ext)
        || LEGACY_OFFICE_EXTENSIONS.contains(&ext)
}

pub(super) fn is_pdf(ext: &str, mime: &str, bytes: &[u8]) -> bool {
    ext == "pdf" || mime == "application/pdf" || bytes.starts_with(b"%PDF-")
}

pub(super) fn is_eml(ext: &str, mime: &str) -> bool {
    ext == "eml" || mime == "message/rfc822"
}

pub(super) fn normalized_mime(mime: &str) -> String {
    mime.split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
}

pub(super) fn normalize_guid(raw: &str) -> String {
    raw.trim().trim_matches(['{', '}']).to_ascii_lowercase()
}

/// Read every marking `bytes` carries. `mime` may be empty.
pub fn read_markings(filename: &str, mime: &str, bytes: &[u8]) -> Markings {
    let ext = extension(filename);
    let mime = normalized_mime(mime);
    let office = is_office_extension(&ext);

    if office && zip_io::is_ole(bytes) {
        return Markings::unreadable();
    }
    if zip_io::looks_like_zip(bytes) {
        return match zip_io::open(bytes) {
            Some(mut package) => read_package(&mut package),
            None if office => Markings::unreadable(),
            None => Markings::default(),
        };
    }
    if office {
        return Markings::unreadable();
    }
    if is_pdf(&ext, &mime, bytes) {
        return read_pdf(bytes);
    }
    if is_eml(&ext, &mime) {
        return read_eml(bytes);
    }
    Markings::default()
}

impl Markings {
    fn unreadable() -> Self {
        Self {
            found: Vec::new(),
            unreadable: true,
        }
    }

    fn push_mip(&mut self, guid: String, name: Option<String>) {
        match self
            .found
            .iter_mut()
            .find(|marking| marking.kind == MarkingKind::Mip && marking.value == guid)
        {
            Some(existing) => {
                if existing.name.is_none() {
                    existing.name = name;
                }
            }
            None => self.found.push(Marking {
                kind: MarkingKind::Mip,
                value: guid,
                name,
            }),
        }
    }

    fn push_all(&mut self, labels: BTreeMap<String, Label>) {
        for (guid, label) in labels {
            if label.enabled {
                self.push_mip(guid, label.name);
            }
        }
    }
}

#[derive(Default)]
struct Label {
    enabled: bool,
    name: Option<String>,
}

/// Fold `MSIP_Label_<GUID>_<Property>` name/value pairs into labels.
fn collect_msip(pairs: impl Iterator<Item = (String, String)>) -> BTreeMap<String, Label> {
    let mut labels: BTreeMap<String, Label> = BTreeMap::new();
    for (name, value) in pairs {
        let Some(rest) = name.strip_prefix(MSIP_PREFIX) else {
            continue;
        };
        let Some((guid, property)) = rest.rsplit_once('_') else {
            continue;
        };
        let label = labels.entry(normalize_guid(guid)).or_default();
        match property {
            "Enabled" => label.enabled = value.trim().eq_ignore_ascii_case("true"),
            "Name" => label.name = Some(value.trim().to_string()).filter(|n| !n.is_empty()),
            _ => {}
        }
    }
    labels
}

fn read_package(package: &mut Package<'_>) -> Markings {
    let names = zip_io::part_names(package);
    let has = |name: &str| names.iter().any(|candidate| candidate == name);
    if has("[Content_Types].xml") {
        read_ooxml(package)
    } else if has("meta.xml") {
        read_odf(package)
    } else {
        Markings::default()
    }
}

fn part(package: &mut Package<'_>, name: &str, markings: &mut Markings) -> Option<Vec<Element>> {
    match zip_io::read_part(package, name, MAX_PART_BYTES) {
        Ok(None) => None,
        Ok(Some(bytes)) => {
            let parsed = elements(&bytes);
            if parsed.is_none() {
                markings.unreadable = true;
            }
            parsed
        }
        Err(PartError::TooLarge | PartError::Unreadable) => {
            markings.unreadable = true;
            None
        }
    }
}

fn read_ooxml(package: &mut Package<'_>) -> Markings {
    let mut markings = Markings::default();

    if let Some(custom) = part(package, "docProps/custom.xml", &mut markings) {
        let properties: Vec<(String, String)> = custom
            .iter()
            .filter(|element| element.name == "property")
            .filter_map(|element| {
                Some((element.attribute("name")?.to_string(), element.text.clone()))
            })
            .collect();
        for (name, value) in &properties {
            if name == TSCP_CATEGORY_NAME && !value.trim().is_empty() {
                markings.found.push(Marking {
                    kind: MarkingKind::Tscp,
                    value: value.trim().to_string(),
                    name: None,
                });
            }
        }
        markings.push_all(collect_msip(properties.into_iter()));
    }

    if let Some(label_info) = part(package, "docMetadata/LabelInfo.xml", &mut markings) {
        for element in label_info.iter().filter(|element| element.name == "label") {
            let enabled = matches!(element.attribute("enabled"), Some("1" | "true"));
            let removed = matches!(element.attribute("removed"), Some("1" | "true"));
            if let (true, false, Some(id)) = (enabled, removed, element.attribute("id")) {
                markings.push_mip(normalize_guid(id), None);
            }
        }
    }
    markings
}

fn read_odf(package: &mut Package<'_>) -> Markings {
    let mut markings = Markings::default();
    if let Some(meta) = part(package, "meta.xml", &mut markings) {
        for element in meta.iter().filter(|e| e.name == "user-defined") {
            let value = element.text.trim();
            if element.attribute("name") == Some(TSCP_CATEGORY_NAME) && !value.is_empty() {
                markings.found.push(Marking {
                    kind: MarkingKind::Tscp,
                    value: value.to_string(),
                    name: None,
                });
            }
        }
    }
    markings
}

fn find_last(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .rposition(|window| window == needle)
}

/// A PDF is encrypted when a trailer or cross-reference stream names an
/// `/Encrypt` entry.
pub(super) fn pdf_is_encrypted(bytes: &[u8]) -> bool {
    bytes.windows(9).any(|window| {
        window.starts_with(b"/Encrypt")
            && (window[8].is_ascii_whitespace()
                || window[8].is_ascii_digit()
                || matches!(window[8], b'<' | b'['))
    })
}

pub(super) const XMP_OPEN: &[u8] = b"<x:xmpmeta";
pub(super) const XMP_CLOSE: &[u8] = b"</x:xmpmeta>";

fn read_pdf(bytes: &[u8]) -> Markings {
    if pdf_is_encrypted(bytes) {
        return Markings::unreadable();
    }
    let mut markings = Markings::default();
    let Some(open) = find_last(bytes, XMP_OPEN) else {
        return markings;
    };
    let Some(close) = find_last(&bytes[open..], XMP_CLOSE) else {
        return markings;
    };
    let packet = &bytes[open..open + close + XMP_CLOSE.len()];
    match elements(packet) {
        Some(all) => {
            let pairs = all
                .into_iter()
                .filter(|element| element.name.starts_with(MSIP_PREFIX))
                .map(|element| (element.name, element.text));
            markings.push_all(collect_msip(pairs));
        }
        None => markings.unreadable = true,
    }
    markings
}

fn read_eml(bytes: &[u8]) -> Markings {
    let mut markings = Markings::default();
    let Some(message) = MessageParser::default().parse(bytes) else {
        return Markings::unreadable();
    };
    let headers = message.headers();
    if let Some(value) = headers
        .value("Sensitivity")
        .and_then(|value| value.as_text().map(str::trim))
        .filter(|value| !value.is_empty())
    {
        markings.found.push(Marking {
            kind: MarkingKind::Sensitivity,
            value: value.to_string(),
            name: None,
        });
    }
    for header in headers.all("msip_labels") {
        let Some(text) = header.value().as_text() else {
            continue;
        };
        let pairs = text.split(';').filter_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            Some((key.trim().to_string(), value.trim().to_string()))
        });
        markings.push_all(collect_msip(pairs));
    }
    markings
}
