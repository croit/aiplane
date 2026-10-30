// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Marking an OOXML package: the Purview label properties and, for a Word
//! document, a header paragraph in every section.

use std::collections::{BTreeSet, HashMap};
use std::sync::LazyLock;

use aiplane_core::server::classification::Level;
use regex::Regex;

use super::{StampOptions, xml_escape};
use crate::server::classification::StampError;
use crate::server::classification::markings::normalize_guid;
use crate::server::classification::zip_io::{self, MAX_PART_BYTES, Package};

const CONTENT_TYPES: &str = "[Content_Types].xml";
const PACKAGE_RELS: &str = "_rels/.rels";
const CUSTOM: &str = "docProps/custom.xml";
const LABEL_INFO: &str = "docMetadata/LabelInfo.xml";
const DOCUMENT: &str = "word/document.xml";
const DOCUMENT_RELS: &str = "word/_rels/document.xml.rels";
const NEW_HEADER: &str = "word/header-classification.xml";
const NEW_HEADER_ID: &str = "rIdAiplaneClassification";

const NS_W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const NS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const NS_VT: &str = "http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes";
const NS_CUSTOM: &str = "http://schemas.openxmlformats.org/officeDocument/2006/custom-properties";
const CUSTOM_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/custom-properties";
const HEADER_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/header";
const FMTID: &str = "{D5CDD505-2E9C-101B-9397-08002B2CF9AE}";
const MARKER: &str = "aiplane_classification";

static OLD_MSIP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?s)<property\b[^>]*\bname="MSIP_Label_[^"]*"[^>]*>.*?</property>"#)
        .expect("static pattern")
});
static PID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\bpid="(\d+)""#).expect("static pattern"));
static RELATIONSHIP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<Relationship\b[^>]*>").expect("static pattern"));
static SECT_PR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<w:sectPr[\s>/]").expect("static pattern"));
static HEADER_REFERENCE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<w:headerReference\b[^>]*>").expect("static pattern"));

fn attribute(tag: &str, name: &str) -> Option<String> {
    let pattern = format!(r#"(?:^|[\s:]){}\s*=\s*"([^"]*)""#, regex::escape(name));
    Regex::new(&pattern)
        .ok()?
        .captures(tag)
        .map(|captures| captures[1].to_string())
}

fn failure(part: &str, reason: &str) -> StampError {
    StampError::Metadata {
        part: part.to_string(),
        reason: reason.to_string(),
    }
}

fn read_text(package: &mut Package<'_>, part: &str) -> Result<Option<String>, StampError> {
    match zip_io::read_part(package, part, MAX_PART_BYTES) {
        Ok(Some(bytes)) => String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| failure(part, "the part is not UTF-8")),
        Ok(None) => Ok(None),
        Err(_) => Err(failure(part, "the part is damaged or too large")),
    }
}

fn insert_before_last(
    xml: &mut String,
    closing: &str,
    insertion: &str,
    part: &str,
) -> Result<(), StampError> {
    match xml.rfind(closing) {
        Some(at) => {
            xml.insert_str(at, insertion);
            Ok(())
        }
        None => Err(failure(part, &format!("the part has no `{closing}`"))),
    }
}

/// Label properties first, then the Word header, as two rewrites of the
/// package so each touches the parts it owns.
pub(super) fn stamp(
    bytes: &[u8],
    level: &Level,
    options: &StampOptions,
) -> Result<Option<Vec<u8>>, StampError> {
    let mut current: Option<Vec<u8>> = None;
    if options.metadata && !level.mip_label_id.is_empty() {
        current = Some(stamp_label(bytes, level, &options.mip_tenant_id)?);
    }
    if options.visible && !level.marking_text.is_empty() {
        let source = current.as_deref().unwrap_or(bytes);
        if let Some(with_header) = stamp_docx_header(source, &level.marking_text)? {
            current = Some(with_header);
        }
    }
    Ok(current)
}

fn label_properties(level: &Level, tenant: &str) -> Vec<(&'static str, String)> {
    vec![
        ("Enabled", "true".into()),
        (
            "SetDate",
            jiff::Timestamp::now()
                .strftime("%Y-%m-%dT%H:%M:%SZ")
                .to_string(),
        ),
        ("Method", "Standard".into()),
        ("Name", level.name.clone()),
        ("SiteId", tenant.to_string()),
        ("ActionId", uuid::Uuid::new_v4().to_string()),
        ("ContentBits", "0".into()),
    ]
}

fn stamp_label(bytes: &[u8], level: &Level, tenant: &str) -> Result<Vec<u8>, StampError> {
    let mut package =
        zip_io::open(bytes).ok_or_else(|| failure(CUSTOM, "the package cannot be opened"))?;
    let guid = normalize_guid(&level.mip_label_id);
    let mut replace: HashMap<String, Vec<u8>> = HashMap::new();
    let mut add: Vec<(String, Vec<u8>)> = Vec::new();

    let existing = read_text(&mut package, CUSTOM)?;
    let mut custom = match &existing {
        Some(xml) => OLD_MSIP.replace_all(xml, "").into_owned(),
        None => format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
             <Properties xmlns=\"{NS_CUSTOM}\" xmlns:vt=\"{NS_VT}\"></Properties>"
        ),
    };
    let first_pid = PID
        .captures_iter(&custom)
        .filter_map(|c| c[1].parse::<u32>().ok())
        .max()
        .unwrap_or(1)
        + 1;
    let mut properties = String::new();
    for (pid, (property, value)) in (first_pid..).zip(label_properties(level, tenant)) {
        properties.push_str(&format!(
            "<property fmtid=\"{FMTID}\" pid=\"{pid}\" name=\"MSIP_Label_{guid}_{property}\">\
             <vt:lpwstr>{}</vt:lpwstr></property>",
            xml_escape(&value)
        ));
    }
    if !custom.contains("xmlns:vt=") {
        let at = custom
            .find("<Properties")
            .ok_or_else(|| failure(CUSTOM, "the part has no Properties element"))?
            + "<Properties".len();
        custom.insert_str(at, &format!(" xmlns:vt=\"{NS_VT}\""));
    }
    if let Some(empty) = custom
        .rfind("/>")
        .filter(|_| !custom.contains("</Properties>"))
    {
        custom.replace_range(empty..empty + 2, &format!(">{properties}</Properties>"));
    } else {
        insert_before_last(&mut custom, "</Properties>", &properties, CUSTOM)?;
    }
    if existing.is_some() {
        replace.insert(CUSTOM.into(), custom.into_bytes());
    } else {
        add.push((CUSTOM.into(), custom.into_bytes()));
    }

    let mut content_types = read_text(&mut package, CONTENT_TYPES)?
        .ok_or_else(|| failure(CONTENT_TYPES, "the package has no content type list"))?;
    if !content_types.contains("/docProps/custom.xml") {
        insert_before_last(
            &mut content_types,
            "</Types>",
            "<Override PartName=\"/docProps/custom.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.custom-properties+xml\"/>",
            CONTENT_TYPES,
        )?;
        replace.insert(CONTENT_TYPES.into(), content_types.into_bytes());
    }

    let mut rels = read_text(&mut package, PACKAGE_RELS)?
        .ok_or_else(|| failure(PACKAGE_RELS, "the package has no relationship list"))?;
    if !rels.contains(CUSTOM_REL) {
        insert_before_last(
            &mut rels,
            "</Relationships>",
            &format!(
                "<Relationship Id=\"rIdAiplaneCustom\" Type=\"{CUSTOM_REL}\" Target=\"docProps/custom.xml\"/>"
            ),
            PACKAGE_RELS,
        )?;
        replace.insert(PACKAGE_RELS.into(), rels.into_bytes());
    }

    if read_text(&mut package, LABEL_INFO)?.is_some() {
        let site = xml_escape(tenant);
        replace.insert(
            LABEL_INFO.into(),
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
                 <clbl:labelList xmlns:clbl=\"http://schemas.microsoft.com/office/2020/mipLabelMetadata\">\
                 <clbl:label id=\"{{{guid}}}\" enabled=\"1\" method=\"Standard\" siteId=\"{{{site}}}\" contentBits=\"0\" removed=\"0\"/>\
                 </clbl:labelList>"
            )
            .into_bytes(),
        );
    }
    drop(package);
    zip_io::rewrite(bytes, &replace, &add)
}

fn header_paragraph(text: &str) -> String {
    format!(
        "<w:p xmlns:w=\"{NS_W}\"><w:pPr><w:jc w:val=\"center\"/></w:pPr>\
         <w:bookmarkStart w:id=\"90210\" w:name=\"{MARKER}\"/>\
         <w:r><w:rPr><w:b/></w:rPr><w:t xml:space=\"preserve\">{}</w:t></w:r>\
         <w:bookmarkEnd w:id=\"90210\"/></w:p>",
        xml_escape(text)
    )
}

fn strip_marked_paragraph(xml: &mut String) {
    while let Some(marker) = xml.find(MARKER) {
        let start = [xml[..marker].rfind("<w:p>"), xml[..marker].rfind("<w:p ")]
            .into_iter()
            .flatten()
            .max();
        let end = xml[marker..]
            .find("</w:p>")
            .map(|at| marker + at + "</w:p>".len());
        match (start, end) {
            (Some(start), Some(end)) => xml.replace_range(start..end, ""),
            _ => break,
        }
    }
}

fn put_paragraph_in_header(header: &mut String, paragraph: &str) -> Result<(), String> {
    strip_marked_paragraph(header);
    let open = header
        .find("<w:hdr")
        .ok_or_else(|| "the header part has no w:hdr element".to_string())?;
    let tag_end = header[open..]
        .find('>')
        .map(|at| open + at)
        .ok_or_else(|| "the header part is not well-formed".to_string())?;
    if header[..tag_end].ends_with('/') {
        header.replace_range(tag_end - 1..=tag_end, &format!(">{paragraph}</w:hdr>"));
    } else {
        header.insert_str(tag_end + 1, paragraph);
    }
    Ok(())
}

struct Section {
    start: usize,
    open_end: usize,
    end: usize,
}

fn sections(document: &str) -> Vec<Section> {
    SECT_PR
        .find_iter(document)
        .filter_map(|found| {
            let start = found.start();
            let open_end = start + document[start..].find('>')? + 1;
            let end = if document[..open_end].ends_with("/>") {
                open_end
            } else {
                open_end + document[open_end..].find("</w:sectPr>")? + "</w:sectPr>".len()
            };
            Some(Section {
                start,
                open_end,
                end,
            })
        })
        .collect()
}

/// A header paragraph with the marking text in every section of a Word
/// document: the headers already referenced get it as their first paragraph,
/// and the first section gets a header of its own for each type it lacks.
/// A package without `word/document.xml` is not a Word document and gives
/// `None`.
fn stamp_docx_header(bytes: &[u8], text: &str) -> Result<Option<Vec<u8>>, StampError> {
    let mut package =
        zip_io::open(bytes).ok_or_else(|| failure(DOCUMENT, "the package cannot be opened"))?;
    let Some(mut document) = read_text(&mut package, DOCUMENT)? else {
        return Ok(None);
    };
    let rels_before = read_text(&mut package, DOCUMENT_RELS)?;
    let mut rels = rels_before.clone().unwrap_or_else(|| {
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"></Relationships>".to_string()
    });

    let header_parts: HashMap<String, String> = RELATIONSHIP
        .find_iter(&rels)
        .filter(|found| found.as_str().contains("/header\""))
        .filter_map(|found| {
            let tag = found.as_str();
            let target = attribute(tag, "Target")?;
            let part = match target.strip_prefix('/') {
                Some(absolute) => absolute.to_string(),
                None => format!("word/{target}"),
            };
            Some((attribute(tag, "Id")?, part))
        })
        .collect();

    let reference_attributes = if document.contains("xmlns:r=") {
        String::new()
    } else {
        format!(" xmlns:r=\"{NS_R}\"")
    };
    let new_references = ["default", "first", "even"]
        .iter()
        .map(|kind| {
            format!(
                "<w:headerReference{reference_attributes} w:type=\"{kind}\" r:id=\"{NEW_HEADER_ID}\"/>"
            )
        })
        .collect::<String>();

    let found = sections(&document);
    let mut referenced: BTreeSet<String> = BTreeSet::new();
    for section in &found {
        for reference in HEADER_REFERENCE.find_iter(&document[section.start..section.end]) {
            if let Some(part) =
                attribute(reference.as_str(), "id").and_then(|id| header_parts.get(&id))
            {
                referenced.insert(part.clone());
            }
        }
    }

    let mut needs_new_header = false;
    if let Some(first) = found.first() {
        let section_xml = document[first.start..first.end].to_string();
        let present: BTreeSet<String> = HEADER_REFERENCE
            .find_iter(&section_xml)
            .filter_map(|reference| attribute(reference.as_str(), "type"))
            .collect();
        let missing: String = ["default", "first", "even"]
            .iter()
            .filter(|kind| !present.contains(**kind))
            .map(|kind| {
                format!(
                    "<w:headerReference{reference_attributes} w:type=\"{kind}\" r:id=\"{NEW_HEADER_ID}\"/>"
                )
            })
            .collect();
        if !missing.is_empty() {
            needs_new_header = true;
            if document[..first.open_end].ends_with("/>") {
                let name_end = first.open_end - 2;
                document.replace_range(name_end..first.open_end, &format!(">{missing}</w:sectPr>"));
            } else {
                document.insert_str(first.open_end, &missing);
            }
        }
    } else {
        let body_close = document
            .rfind("</w:body>")
            .ok_or_else(|| failure(DOCUMENT, "the document has no w:body"))?;
        document.insert_str(
            body_close,
            &format!("<w:sectPr>{new_references}</w:sectPr>"),
        );
        needs_new_header = true;
    }

    let paragraph = header_paragraph(text);
    let mut replace: HashMap<String, Vec<u8>> = HashMap::new();
    let mut add: Vec<(String, Vec<u8>)> = Vec::new();
    for part in &referenced {
        let Some(mut header) = read_text(&mut package, part)? else {
            continue;
        };
        put_paragraph_in_header(&mut header, &paragraph)
            .map_err(|reason| failure(part, &reason))?;
        replace.insert(part.clone(), header.into_bytes());
    }
    if needs_new_header {
        add.push((
            NEW_HEADER.into(),
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:hdr xmlns:w=\"{NS_W}\">{paragraph}</w:hdr>"
            )
            .into_bytes(),
        ));
        insert_before_last(
            &mut rels,
            "</Relationships>",
            &format!(
                "<Relationship Id=\"{NEW_HEADER_ID}\" Type=\"{HEADER_REL}\" Target=\"header-classification.xml\"/>"
            ),
            DOCUMENT_RELS,
        )?;
        let mut content_types = read_text(&mut package, CONTENT_TYPES)?
            .ok_or_else(|| failure(CONTENT_TYPES, "the package has no content type list"))?;
        insert_before_last(
            &mut content_types,
            "</Types>",
            "<Override PartName=\"/word/header-classification.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml\"/>",
            CONTENT_TYPES,
        )?;
        replace.insert(CONTENT_TYPES.into(), content_types.into_bytes());
        if rels_before.is_some() {
            replace.insert(DOCUMENT_RELS.into(), rels.into_bytes());
        } else {
            add.push((DOCUMENT_RELS.into(), rels.into_bytes()));
        }
    }
    replace.insert(DOCUMENT.into(), document.into_bytes());
    drop(package);
    zip_io::rewrite(bytes, &replace, &add).map(Some)
}
