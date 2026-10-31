// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Marking generated files with the conversation's level.
//!
//! | Format | File name | Metadata | Visible |
//! | --- | --- | --- | --- |
//! | docx, xlsx, pptx (and macro/template variants) | tag | `MSIP_Label_<GUID>_*` in `docProps/custom.xml`, `LabelInfo.xml` when present | docx only: a header paragraph |
//! | odt, ods, odp (and templates) | tag | TSCP user-defined properties in `meta.xml` | none |
//! | PDF with a classic xref table | tag | `MSIP_Label_<GUID>_*` in an XMP stream, added by an incremental update | none |
//! | Markdown, text | tag | none | first line |
//! | HTML | tag | `<meta name="classification">` | banner at the start of the body |
//! | everything else | tag | none | none |
//!
//! A PDF that is encrypted, ends in a cross-reference stream, or has an XMP
//! stream this cannot merge into is left byte for byte as it is: it gets the
//! file name tag only.

mod html;
mod odf;
mod ooxml;
mod pdf;
mod text;

use super::StampError;
use super::filename::with_tag;
use super::markings::{extension, normalized_mime};
use super::zip_io;
use aiplane_core::server::classification::{Level, Scheme};

/// Which parts of a generated file carry its level, and the tenant the
/// Purview label belongs to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StampOptions {
    pub visible: bool,
    pub metadata: bool,
    pub filename: bool,
    pub mip_tenant_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stamped {
    pub filename: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Ooxml,
    Odf,
    Pdf,
    Text,
    Html,
    Other,
}

fn detect(filename: &str, mime: &str, bytes: &[u8]) -> Format {
    if zip_io::looks_like_zip(bytes) {
        let Some(mut package) = zip_io::open(bytes) else {
            return Format::Other;
        };
        let names = zip_io::part_names(&package);
        if names.iter().any(|name| name == "[Content_Types].xml") {
            return Format::Ooxml;
        }
        if let Ok(Some(mimetype)) = zip_io::read_part(&mut package, "mimetype", 256)
            && mimetype.starts_with(b"application/vnd.oasis.opendocument")
        {
            return Format::Odf;
        }
        return Format::Other;
    }
    let ext = extension(filename);
    let mime = normalized_mime(mime);
    if bytes.starts_with(b"%PDF-") {
        Format::Pdf
    } else if matches!(ext.as_str(), "md" | "markdown" | "txt" | "text")
        || matches!(mime.as_str(), "text/markdown" | "text/plain")
    {
        Format::Text
    } else if matches!(ext.as_str(), "html" | "htm") || mime == "text/html" {
        Format::Html
    } else {
        Format::Other
    }
}

pub(super) fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// `bytes` marked with `level`. A level of the lowest rank, or one with no
/// marking text, file name tag or label id, leaves everything as it is. The
/// file name tag replaces a tag of any level of `scheme`.
///
/// An error means the document could not be rewritten; the caller decides
/// whether to deliver the file unmarked.
pub fn stamp_output(
    filename: &str,
    mime: &str,
    bytes: &[u8],
    level: &Level,
    scheme: &Scheme,
    options: &StampOptions,
) -> Result<Stamped, StampError> {
    let unchanged = || Stamped {
        filename: filename.to_string(),
        bytes: bytes.to_vec(),
    };
    if level.rank == 0
        || (level.marking_text.is_empty()
            && level.filename_tag.is_empty()
            && level.mip_label_id.is_empty())
    {
        return Ok(unchanged());
    }

    let new_name = if options.filename && !level.filename_tag.is_empty() {
        with_tag(filename, level, scheme)
    } else {
        filename.to_string()
    };

    let marked = match detect(filename, mime, bytes) {
        Format::Ooxml => ooxml::stamp(bytes, level, options)?,
        Format::Odf if options.metadata => odf::stamp(bytes, level)?,
        Format::Pdf if options.metadata && !level.mip_label_id.is_empty() => {
            pdf::stamp(bytes, level, options)
        }
        Format::Text if options.visible => text::stamp(bytes, level, scheme),
        Format::Html => html::stamp(bytes, level, options),
        _ => None,
    };
    Ok(Stamped {
        filename: new_name,
        bytes: marked.unwrap_or_else(|| bytes.to_vec()),
    })
}
