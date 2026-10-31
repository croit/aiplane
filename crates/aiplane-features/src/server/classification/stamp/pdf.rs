// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Marking a PDF with an incremental update: a new XMP metadata stream and a
//! new revision of the catalog that points `/Metadata` at it. The original
//! bytes stay in place and signatures over them stay valid.

use std::sync::LazyLock;

use aiplane_core::server::classification::Level;
use regex::Regex;

use super::{StampOptions, xml_escape};
use crate::server::classification::markings::{normalize_guid, pdf_is_encrypted};

static SIZE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"/Size\s+(\d+)").expect("static pattern"));
static PREV: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"/Prev\s+\d+").expect("static pattern"));
static ROOT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"/Root\s+(\d+)\s+(\d+)\s+R").expect("static pattern"));
static METADATA: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"/Metadata\s+(\d+)\s+(\d+)\s+R").expect("static pattern"));
static OLD_MSIP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)<[A-Za-z0-9]+:MSIP_Label_[^>]*>[^<]*</[A-Za-z0-9]+:MSIP_Label_[^>]*>")
        .expect("static pattern")
});

fn find_last(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .rposition(|window| window == needle)
}

fn find_from(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    haystack
        .get(from..)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|at| from + at)
}

/// The text after the last `startxref`, as the offset it names.
fn last_startxref(bytes: &[u8]) -> Option<usize> {
    let at = find_last(bytes, b"startxref")? + "startxref".len();
    let digits: String = bytes[at..]
        .iter()
        .skip_while(|b| b.is_ascii_whitespace())
        .take_while(|b| b.is_ascii_digit())
        .map(|&b| b as char)
        .collect();
    digits.parse().ok()
}

/// The `<< ... >>` dictionary that starts at `from`, hex strings skipped.
fn dictionary_at(bytes: &[u8], from: usize) -> Option<&[u8]> {
    let start = find_from(bytes, b"<<", from)?;
    let mut depth = 0usize;
    let mut i = start;
    while i < bytes.len() {
        match (bytes[i], bytes.get(i + 1)) {
            (b'<', Some(b'<')) => {
                depth += 1;
                i += 2;
            }
            (b'>', Some(b'>')) => {
                depth -= 1;
                i += 2;
                if depth == 0 {
                    return Some(&bytes[start..i]);
                }
            }
            (b'<', _) => i = find_from(bytes, b">", i)? + 1,
            _ => i += 1,
        }
    }
    None
}

/// The body between `N G obj` and `endobj` of the last definition of the
/// object.
fn object_body(bytes: &[u8], number: &str, generation: &str) -> Option<(usize, usize)> {
    let header = format!("{number} {generation} obj");
    let mut found = None;
    let mut from = 0;
    while let Some(at) = find_from(bytes, header.as_bytes(), from) {
        let starts_line = at == 0 || bytes[at - 1].is_ascii_whitespace();
        if starts_line {
            found = Some(at + header.len());
        }
        from = at + 1;
    }
    let start = found?;
    let end = find_from(bytes, b"endobj", start)?;
    Some((start, end))
}

fn description(level: &Level, tenant: &str) -> String {
    let guid = normalize_guid(&level.mip_label_id);
    let set_date = jiff::Timestamp::now()
        .strftime("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    let action = uuid::Uuid::new_v4();
    let mut out = String::from(
        "<rdf:Description rdf:about=\"\" xmlns:pdfx=\"http://ns.adobe.com/pdfx/1.3/\">",
    );
    for (property, value) in [
        ("Enabled", "true".to_string()),
        ("SetDate", set_date),
        ("Method", "Standard".into()),
        ("Name", level.name.clone()),
        ("SiteId", tenant.to_string()),
        ("ActionId", action.to_string()),
        ("ContentBits", "0".into()),
    ] {
        out.push_str(&format!(
            "<pdfx:MSIP_Label_{guid}_{property}>{}</pdfx:MSIP_Label_{guid}_{property}>",
            xml_escape(&value)
        ));
    }
    out.push_str("</rdf:Description>");
    out
}

fn fresh_packet(description: &str) -> String {
    format!(
        "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n\
         <x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">{description}</rdf:RDF></x:xmpmeta>\n\
         <?xpacket end=\"w\"?>"
    )
}

/// The existing XMP stream with its label elements replaced, so unrelated
/// metadata such as the title survives. `None` when the stream is compressed
/// or not XMP this can edit.
fn merged_packet(bytes: &[u8], catalog: &str, description: &str) -> Option<Option<String>> {
    let Some(reference) = METADATA.captures(catalog) else {
        return Some(None);
    };
    let (start, end) = object_body(bytes, &reference[1], &reference[2])?;
    let body = &bytes[start..end];
    let dictionary = dictionary_at(body, 0)?;
    if find_from(dictionary, b"/Filter", 0).is_some() {
        return None;
    }
    let stream_at = find_from(body, b"stream", 0)? + "stream".len();
    let data_start = stream_at
        + if body[stream_at..].starts_with(b"\r\n") {
            2
        } else {
            1
        };
    let data_end = find_last(body, b"endstream")?;
    let packet = std::str::from_utf8(body.get(data_start..data_end)?).ok()?;
    if !packet.contains("<x:xmpmeta") {
        return None;
    }
    let mut packet = OLD_MSIP.replace_all(packet, "").into_owned();
    let close = find_last(packet.as_bytes(), b"</rdf:RDF>")?;
    packet.insert_str(close, description);
    Some(Some(packet))
}

/// `None` leaves the PDF as it is: it is encrypted, ends in a cross-reference
/// stream or a hybrid table, or its catalog or XMP cannot be edited here.
pub(super) fn stamp(bytes: &[u8], level: &Level, options: &StampOptions) -> Option<Vec<u8>> {
    if pdf_is_encrypted(bytes) {
        return None;
    }
    let previous = last_startxref(bytes)?;
    if !bytes.get(previous..)?.starts_with(b"xref") {
        return None;
    }
    let trailer_at = find_from(bytes, b"trailer", previous)? + "trailer".len();
    let trailer = std::str::from_utf8(dictionary_at(bytes, trailer_at)?).ok()?;
    if trailer.contains("/XRefStm") {
        return None;
    }
    let size: usize = SIZE.captures(trailer)?[1].parse().ok()?;
    let root = ROOT.captures(trailer)?;
    let (root_number, root_generation) = (root[1].to_string(), root[2].to_string());

    let (catalog_start, catalog_end) = object_body(bytes, &root_number, &root_generation)?;
    let catalog = std::str::from_utf8(&bytes[catalog_start..catalog_end])
        .ok()?
        .trim();
    if !catalog.starts_with("<<") || !catalog.ends_with(">>") {
        return None;
    }

    let description = description(level, &options.mip_tenant_id);
    let packet = match merged_packet(bytes, catalog, &description)? {
        Some(merged) => merged,
        None => fresh_packet(&description),
    };

    let metadata_number = size;
    let mut catalog = METADATA.replace(catalog, "").into_owned();
    let close = catalog.rfind(">>")?;
    catalog.insert_str(close, &format!("/Metadata {metadata_number} 0 R "));

    let mut out = bytes.to_vec();
    if !out.ends_with(b"\n") {
        out.push(b'\n');
    }
    let metadata_offset = out.len();
    out.extend_from_slice(
        format!(
            "{metadata_number} 0 obj\n<< /Type /Metadata /Subtype /XML /Length {} >>\nstream\n",
            packet.len()
        )
        .as_bytes(),
    );
    out.extend_from_slice(packet.as_bytes());
    out.extend_from_slice(b"\nendstream\nendobj\n");
    let catalog_offset = out.len();
    out.extend_from_slice(
        format!("{root_number} {root_generation} obj\n{catalog}\nendobj\n").as_bytes(),
    );

    let xref_offset = out.len();
    let mut subsections = [
        (
            root_number.parse::<usize>().ok()?,
            catalog_offset,
            root_generation.parse::<u32>().ok()?,
        ),
        (metadata_number, metadata_offset, 0),
    ];
    subsections.sort_by_key(|(number, _, _)| *number);
    out.extend_from_slice(b"xref\n");
    for (number, offset, generation) in subsections {
        out.extend_from_slice(format!("{number} 1\n{offset:010} {generation:05} n \n").as_bytes());
    }
    let carried = PREV.replace(&SIZE.replace(trailer, ""), "").into_owned();
    let open = carried.find("<<")? + 2;
    let mut new_trailer = carried;
    new_trailer.insert_str(open, &format!(" /Size {} /Prev {previous}", size + 1));
    out.extend_from_slice(
        format!("trailer\n{new_trailer}\nstartxref\n{xref_offset}\n%%EOF\n").as_bytes(),
    );
    Some(out)
}
