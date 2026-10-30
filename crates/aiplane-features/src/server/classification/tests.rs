// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The readers, the resolver, the text and the stamper against the fixture
//! files in `testdata/classification/` (see its README).

use std::collections::HashMap;
use std::io::Read;

use aiplane_core::server::classification::{
    ClassificationConfig, Level, MarkingKind, MarkingRule, Scheme,
};

use super::zip_io::tests::build;
use super::*;

const CONFIDENTIAL_GUID: &str = "9fbde396-1a24-4c79-8edf-9254a0f35055";
const INTERNAL_GUID: &str = "87ba5c36-b7cf-4793-bbc2-bd5b3a9f95ca";
const UNMAPPED_GUID: &str = "f42aa342-8706-4288-bd11-ebb85995028c";
const TENANT: &str = "72f988bf-86f1-41af-91ab-2d7cd011db47";

fn fixture(name: &str) -> Vec<u8> {
    let path = format!(
        "{}/src/server/testdata/classification/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path).unwrap_or_else(|e| panic!("fixture {path}: {e}"))
}

fn level(id: &str, rank: u32, name: &str, tag: &str, mip: &str, text: &str) -> Level {
    Level {
        id: id.into(),
        rank,
        name: name.into(),
        description: String::new(),
        color: "neutral".into(),
        marking_text: text.into(),
        mip_label_id: mip.into(),
        filename_tag: tag.into(),
    }
}

fn scheme() -> Scheme {
    Scheme::new(
        vec![
            level("public", 0, "Public", "", "", ""),
            level(
                "internal",
                1,
                "Internal",
                "internal",
                INTERNAL_GUID,
                "INTERNAL",
            ),
            level(
                "confidential",
                2,
                "Confidential",
                "confidential",
                CONFIDENTIAL_GUID,
                "CONFIDENTIAL",
            ),
            level(
                "strict",
                3,
                "Highly Confidential",
                "",
                UNMAPPED_GUID,
                "HIGHLY CONFIDENTIAL",
            ),
        ],
        Vec::new(),
    )
}

fn rule(kind: MarkingKind, value: &str, level_id: &str) -> MarkingRule {
    MarkingRule {
        kind,
        value: value.into(),
        level_id: level_id.into(),
    }
}

fn rules() -> Vec<MarkingRule> {
    vec![
        rule(MarkingKind::Mip, CONFIDENTIAL_GUID, "confidential"),
        rule(
            MarkingKind::Mip,
            &format!("{{{}}}", INTERNAL_GUID.to_uppercase()),
            "internal",
        ),
        rule(MarkingKind::Tscp, "confidential", "confidential"),
        rule(MarkingKind::Tscp, "Internal", "internal"),
        rule(
            MarkingKind::Sensitivity,
            "Company-Confidential",
            "confidential",
        ),
        rule(MarkingKind::Stamp, "VERTRAULICH", "internal"),
        rule(MarkingKind::Stamp, "STRENG VERTRAULICH", "confidential"),
        rule(MarkingKind::Stamp, "TLP:AMBER", "confidential"),
        rule(MarkingKind::Stamp, "CONFIDENTIAL", "confidential"),
    ]
}

fn policy() -> ClassificationConfig {
    ClassificationConfig {
        unknown_label_level: "strict".into(),
        unreadable_level: "strict".into(),
        mip_tenant_id: TENANT.into(),
        ..Default::default()
    }
}

fn resolve_file(filename: &str, mime: &str, bytes: &[u8]) -> Resolved {
    let scheme = scheme();
    let rules = rules();
    let policy = policy();
    let mapping = Mapping {
        scheme: &scheme,
        rules: &rules,
        policy: &policy,
    };
    resolve(&read_markings(filename, mime, bytes), filename, &mapping)
}

fn mip(guid: &str, name: Option<&str>) -> Marking {
    Marking {
        kind: MarkingKind::Mip,
        value: guid.into(),
        name: name.map(str::to_string),
    }
}

fn options() -> StampOptions {
    StampOptions {
        visible: true,
        metadata: true,
        filename: true,
        mip_tenant_id: TENANT.into(),
    }
}

fn stamp(filename: &str, bytes: &[u8], level_id: &str) -> Stamped {
    let scheme = scheme();
    stamp_output(
        filename,
        "",
        bytes,
        scheme.level(level_id).unwrap(),
        &scheme,
        &options(),
    )
    .unwrap()
}

fn part(zip: &[u8], name: &str) -> String {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip)).expect("a readable zip");
    let mut out = String::new();
    archive
        .by_name(name)
        .unwrap_or_else(|_| panic!("part {name}"))
        .read_to_string(&mut out)
        .unwrap();
    out
}

fn all_parts(zip: &[u8]) -> HashMap<String, Vec<u8>> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip)).expect("a readable zip");
    (0..archive.len())
        .map(|i| {
            let mut file = archive.by_index(i).unwrap();
            let mut content = Vec::new();
            file.read_to_end(&mut content).unwrap();
            (file.name().unwrap().into_owned(), content)
        })
        .collect()
}

fn assert_parts_well_formed(zip: &[u8]) {
    for (name, content) in all_parts(zip) {
        if name.ends_with(".xml") || name.ends_with(".rels") {
            assert!(
                xml::elements(&content).is_some(),
                "{name} is not well-formed"
            );
        }
    }
}

mod readers {
    use super::*;

    #[test]
    fn a_docx_label_is_read_from_the_custom_properties() {
        let found = read_markings(
            "mip_confidential.docx",
            "",
            &fixture("mip_confidential.docx"),
        );
        assert_eq!(
            found.found,
            vec![mip(CONFIDENTIAL_GUID, Some("Confidential"))]
        );
        assert!(!found.unreadable);
    }

    #[test]
    fn an_xlsx_label_is_read_the_same_way() {
        let found = read_markings(
            "mip_confidential.xlsx",
            "",
            &fixture("mip_confidential.xlsx"),
        );
        assert_eq!(
            found.found,
            vec![mip(CONFIDENTIAL_GUID, Some("Confidential"))]
        );
    }

    #[test]
    fn label_info_gives_the_guid_without_braces_and_without_a_name() {
        let found = read_markings(
            "labelinfo_internal.docx",
            "",
            &fixture("labelinfo_internal.docx"),
        );
        assert_eq!(found.found, vec![mip(INTERNAL_GUID, None)]);
    }

    #[test]
    fn a_package_is_recognised_by_its_content_not_its_extension() {
        let found = read_markings(
            "renamed.bin",
            "application/octet-stream",
            &fixture("mip_confidential.docx"),
        );
        assert_eq!(found.found.len(), 1);
    }

    #[test]
    fn an_unlabelled_docx_has_no_marking() {
        assert_eq!(
            read_markings("unlabelled.docx", "", &fixture("unlabelled.docx")),
            Markings::default()
        );
    }

    #[test]
    fn a_disabled_label_is_not_a_marking() {
        let custom = format!(
            "<Properties xmlns=\"x\" xmlns:vt=\"y\"><property pid=\"2\" name=\"MSIP_Label_{CONFIDENTIAL_GUID}_Enabled\"><vt:lpwstr>False</vt:lpwstr></property></Properties>"
        );
        let docx = build(&[
            ("[Content_Types].xml", b"<Types/>"),
            ("docProps/custom.xml", custom.as_bytes()),
        ]);
        assert!(read_markings("a.docx", "", &docx).found.is_empty());
    }

    #[test]
    fn a_tscp_property_in_an_ooxml_package_is_read() {
        let custom = format!(
            "<Properties xmlns=\"x\" xmlns:vt=\"y\"><property pid=\"2\" name=\"{TSCP_CATEGORY_NAME}\"><vt:lpwstr>Confidential</vt:lpwstr></property></Properties>"
        );
        let docx = build(&[
            ("[Content_Types].xml", b"<Types/>"),
            ("docProps/custom.xml", custom.as_bytes()),
        ]);
        let found = read_markings("a.docx", "", &docx);
        assert_eq!(
            found.found,
            vec![Marking {
                kind: MarkingKind::Tscp,
                value: "Confidential".into(),
                name: None
            }]
        );
    }

    #[test]
    fn an_odt_tscp_category_is_read_from_meta_xml() {
        let found = read_markings(
            "tscp_confidential.odt",
            "",
            &fixture("tscp_confidential.odt"),
        );
        assert_eq!(
            found.found,
            vec![Marking {
                kind: MarkingKind::Tscp,
                value: "Confidential".into(),
                name: None
            }]
        );
    }

    #[test]
    fn a_pdf_label_is_read_from_the_xmp_packet() {
        let found = read_markings(
            "mip_internal.pdf",
            "application/pdf",
            &fixture("mip_internal.pdf"),
        );
        assert_eq!(found.found, vec![mip(INTERNAL_GUID, Some("Internal"))]);
    }

    #[test]
    fn an_encrypted_pdf_is_unreadable() {
        let mut pdf = crate::server::pdf::test_support::hello_pdf();
        let at = pdf.windows(5).rposition(|w| w == b"/Root").unwrap();
        pdf.splice(at..at, b"/Encrypt 9 0 R ".iter().copied());
        let found = read_markings("a.pdf", "", &pdf);
        assert!(found.unreadable);
        assert!(found.found.is_empty());
    }

    #[test]
    fn an_email_gives_its_sensitivity_header() {
        let found = read_markings(
            "sensitivity_confidential.eml",
            "",
            &fixture("sensitivity_confidential.eml"),
        );
        assert_eq!(
            found.found,
            vec![Marking {
                kind: MarkingKind::Sensitivity,
                value: "Company-Confidential".into(),
                name: None
            }]
        );
    }

    #[test]
    fn an_email_gives_its_msip_labels_header() {
        let found = read_markings("m.eml", "message/rfc822", &fixture("msip_confidential.eml"));
        assert_eq!(
            found.found,
            vec![mip(CONFIDENTIAL_GUID, Some("Confidential"))]
        );
    }

    #[test]
    fn an_encrypted_office_container_is_unreadable() {
        let found = read_markings("encrypted.docx", "", &fixture("encrypted.docx"));
        assert!(found.unreadable);
        assert!(found.found.is_empty());
    }

    #[test]
    fn a_corrupt_zip_for_an_office_extension_is_unreadable_but_not_for_other_names() {
        assert!(read_markings("broken.xlsx", "", b"PK\x03\x04 not really").unreadable);
        assert!(
            read_markings("notes.txt", "", b"PK\x03\x04 not really")
                .found
                .is_empty()
        );
        assert!(!read_markings("notes.txt", "", b"PK\x03\x04 not really").unreadable);
        assert!(read_markings("old.doc", "", &fixture("encrypted.docx")).unreadable);
    }

    #[test]
    fn plain_files_and_the_json_fixture_have_no_marking() {
        assert_eq!(
            read_markings(
                "offer-confidential.json",
                "application/json",
                &fixture("offer-confidential.json")
            ),
            Markings::default()
        );
    }

    #[test]
    fn a_part_that_inflates_past_the_cap_makes_the_file_unreadable_without_exhausting_memory() {
        let bomb = vec![b' '; 12 * 1024 * 1024];
        let docx = build(&[
            ("[Content_Types].xml", b"<Types/>"),
            ("docProps/custom.xml", &bomb),
        ]);
        assert!(docx.len() < 64 * 1024, "the fixture must compress hard");
        let found = read_markings("bomb.docx", "", &docx);
        assert!(found.unreadable);
        assert!(found.found.is_empty());
    }
}

mod resolution {
    use super::*;

    #[test]
    fn a_mapped_label_resolves_by_guid_with_a_content_free_cause() {
        let resolved = resolve_file("a.docx", "", &fixture("mip_confidential.docx"));
        assert_eq!(resolved.level_id.as_deref(), Some("confidential"));
        assert_eq!(
            resolved
                .causes
                .iter()
                .map(Cause::as_str)
                .collect::<Vec<_>>(),
            [format!("marking:mip:{CONFIDENTIAL_GUID}")]
        );
    }

    #[test]
    fn a_guid_rule_may_be_written_with_braces_and_capitals() {
        let resolved = resolve_file("l.docx", "", &fixture("labelinfo_internal.docx"));
        assert_eq!(resolved.level_id.as_deref(), Some("internal"));
    }

    #[test]
    fn an_unknown_guid_falls_back_to_the_label_name() {
        let scheme = scheme();
        let rules = vec![rule(MarkingKind::Mip, "highly confidential", "strict")];
        let policy = policy();
        let mapping = Mapping {
            scheme: &scheme,
            rules: &rules,
            policy: &policy,
        };
        let markings = read_markings("u.docx", "", &fixture("mip_unmapped.docx"));
        assert_eq!(
            resolve(&markings, "u.docx", &mapping).level_id.as_deref(),
            Some("strict")
        );
    }

    #[test]
    fn an_unmapped_label_takes_the_unknown_label_level_and_says_so() {
        let mut policy = policy();
        policy.unknown_label_level = "internal".into();
        let scheme = scheme();
        let rules = rules();
        let mapping = Mapping {
            scheme: &scheme,
            rules: &rules,
            policy: &policy,
        };
        let markings = read_markings("u.docx", "", &fixture("mip_unmapped.docx"));
        let resolved = resolve(&markings, "u.docx", &mapping);
        assert_eq!(resolved.level_id.as_deref(), Some("internal"));
        assert_eq!(resolved.causes[0].as_str(), "marking:unmapped");
    }

    #[test]
    fn an_unreadable_file_takes_the_unreadable_level() {
        let resolved = resolve_file("encrypted.docx", "", &fixture("encrypted.docx"));
        assert_eq!(resolved.level_id.as_deref(), Some("strict"));
        assert_eq!(resolved.causes[0].as_str(), "marking:unreadable");
    }

    #[test]
    fn an_unlabelled_file_resolves_to_nothing() {
        assert_eq!(
            resolve_file("unlabelled.docx", "", &fixture("unlabelled.docx")),
            Resolved::default()
        );
    }

    #[test]
    fn the_filename_tag_alone_resolves_a_level() {
        let resolved = resolve_file(
            "offer-confidential.json",
            "application/json",
            &fixture("offer-confidential.json"),
        );
        assert_eq!(resolved.level_id.as_deref(), Some("confidential"));
        assert_eq!(resolved.causes[0].as_str(), "marking:filename");
    }

    #[test]
    fn the_sensitivity_header_and_tscp_resolve_by_value_ignoring_case() {
        assert_eq!(
            resolve_file("s.eml", "", &fixture("sensitivity_confidential.eml"))
                .level_id
                .as_deref(),
            Some("confidential")
        );
        assert_eq!(
            resolve_file("t.odt", "", &fixture("tscp_confidential.odt"))
                .level_id
                .as_deref(),
            Some("confidential")
        );
    }

    #[test]
    fn when_markings_disagree_the_highest_rank_wins_and_every_cause_is_listed() {
        let resolved = resolve_file(
            "report-internal.docx",
            "",
            &fixture("mip_confidential.docx"),
        );
        assert_eq!(resolved.level_id.as_deref(), Some("confidential"));
        assert_eq!(resolved.causes.len(), 2);
        let lower = resolve_file(
            "report-confidential.docx",
            "",
            &fixture("labelinfo_internal.docx"),
        );
        assert_eq!(lower.level_id.as_deref(), Some("confidential"));
    }
}

mod text_detection {
    use super::*;

    #[test]
    fn a_markdown_stamp_in_the_first_line_is_matched() {
        let text = detection_text(
            "tlp_amber.md",
            "text/markdown",
            &fixture("tlp_amber.md"),
            100_000,
        )
        .unwrap();
        assert_eq!(stamp_matches(&text, &rules()), ["confidential"]);
    }

    #[test]
    fn a_german_stamp_matches_case_insensitively_and_only_the_stamps_that_are_there() {
        let text = detection_text(
            "v.txt",
            "text/plain",
            &fixture("vertraulich_stamp.txt"),
            100_000,
        )
        .unwrap();
        assert_eq!(stamp_matches(&text, &rules()), ["internal"]);
        assert_eq!(
            stamp_matches("streng vertraulich: Plan", &rules()),
            ["internal", "confidential"]
        );
    }

    #[test]
    fn a_stamp_must_sit_on_word_boundaries_and_in_the_first_4_kib() {
        assert!(stamp_matches("NONCONFIDENTIAL stuff", &rules()).is_empty());
        let late = format!("{}VERTRAULICH", "x ".repeat(3000));
        assert!(stamp_matches(&late, &rules()).is_empty());
        assert!(stamp_matches("TLP:AMBER+STRICT", &rules()).contains(&"confidential".to_string()));
    }

    #[test]
    fn stamp_matching_ignores_rules_of_other_kinds() {
        let only_mip = vec![rule(MarkingKind::Mip, "CONFIDENTIAL", "confidential")];
        assert!(stamp_matches("CONFIDENTIAL", &only_mip).is_empty());
    }

    #[test]
    fn html_loses_its_tags_and_json_and_csv_are_plain_text() {
        let html = detection_text(
            "a.html",
            "",
            b"<html><body><h1>Hi</h1><p>there</p></body></html>",
            1000,
        )
        .unwrap();
        assert!(html.contains("Hi") && !html.contains('<'));
        assert!(
            detection_text("a.json", "application/json", br#"{"k":"v"}"#, 1000)
                .unwrap()
                .contains("\"k\"")
        );
        assert!(
            detection_text("a.csv", "", b"a,b\n1,2\n", 1000)
                .unwrap()
                .contains("1,2")
        );
    }

    #[test]
    fn office_text_comes_from_the_xml_parts() {
        let text = detection_text("a.docx", "", &fixture("unlabelled.docx"), 1000).unwrap();
        assert_eq!(text.trim(), "Public press release draft.");
        let odt = detection_text("a.odt", "", &fixture("tscp_confidential.odt"), 1000).unwrap();
        assert!(!odt.is_empty());
    }

    #[test]
    fn a_pdf_gives_its_text_layer() {
        let pdf = crate::server::pdf::test_support::hello_pdf();
        assert!(
            detection_text("a.pdf", "application/pdf", &pdf, 1000)
                .unwrap()
                .contains("Hello PDF")
        );
    }

    #[test]
    fn an_email_gives_subject_and_body() {
        let text =
            detection_text("m.eml", "", &fixture("sensitivity_confidential.eml"), 1000).unwrap();
        assert!(text.contains("Draft contract") && text.contains("attached tomorrow"));
    }

    #[test]
    fn the_text_is_cut_at_the_cap_on_a_character_boundary() {
        let text = detection_text("a.txt", "", "ä".repeat(100).as_bytes(), 11).unwrap();
        assert_eq!(text.len(), 10);
    }

    #[test]
    fn binary_files_and_images_have_no_text() {
        assert!(
            detection_text("a.png", "image/png", &[0x89, b'P', b'N', b'G', 0xff], 100).is_none()
        );
        assert!(detection_text("a.bin", "", &[0xff, 0xfe, 0x00], 100).is_none());
        assert!(detection_text("e.docx", "", &fixture("encrypted.docx"), 100).is_none());
    }

    #[test]
    fn an_oversized_office_part_is_skipped_instead_of_inflated() {
        let bomb = vec![b'a'; 12 * 1024 * 1024];
        let docx = build(&[
            ("[Content_Types].xml", b"<Types/>"),
            ("word/document.xml", &bomb),
        ]);
        assert_eq!(detection_text("bomb.docx", "", &docx, 1000).unwrap(), "");
    }
}

mod stamping {
    use super::*;

    fn label_of(filename: &str, bytes: &[u8]) -> Resolved {
        resolve_file(filename, "", bytes)
    }

    #[test]
    fn a_docx_gets_the_label_the_name_tag_and_a_header_and_reads_back() {
        let original = fixture("unlabelled.docx");
        let stamped = stamp("report.docx", &original, "confidential");
        assert_eq!(stamped.filename, "report-confidential.docx");
        assert_parts_well_formed(&stamped.bytes);

        let read = read_markings(&stamped.filename, "", &stamped.bytes);
        assert_eq!(
            read.found,
            vec![mip(CONFIDENTIAL_GUID, Some("Confidential"))]
        );
        assert_eq!(
            label_of("report.docx", &stamped.bytes).level_id.as_deref(),
            Some("confidential")
        );

        let custom = part(&stamped.bytes, "docProps/custom.xml");
        assert!(
            custom.contains(&format!("MSIP_Label_{CONFIDENTIAL_GUID}_SiteId"))
                && custom.contains(TENANT)
        );
        assert!(custom.contains("_Method\"><vt:lpwstr>Standard"));
        assert!(part(&stamped.bytes, "[Content_Types].xml").contains("/docProps/custom.xml"));
        assert!(part(&stamped.bytes, "_rels/.rels").contains("custom-properties"));

        let text = detection_text("report.docx", "", &stamped.bytes, 10_000).unwrap();
        assert!(
            text.starts_with("CONFIDENTIAL"),
            "the header comes first: {text:?}"
        );
        assert!(text.contains("Public press release draft."));
        assert_eq!(stamp_matches(&text, &rules()), ["confidential"]);
    }

    #[test]
    fn every_original_part_stays_byte_for_byte() {
        let original = fixture("unlabelled.docx");
        let stamped = stamp("r.docx", &original, "internal");
        let before = all_parts(&original);
        let after = all_parts(&stamped.bytes);
        for (name, content) in &before {
            if name == "[Content_Types].xml" || name == "_rels/.rels" || name == "word/document.xml"
            {
                continue;
            }
            assert_eq!(after.get(name), Some(content), "{name} changed");
        }
        assert!(after.contains_key("docProps/custom.xml"));
    }

    #[test]
    fn stamping_twice_replaces_the_label_the_tag_and_the_header() {
        let first = stamp("r.docx", &fixture("unlabelled.docx"), "internal");
        let second = stamp(&first.filename, &first.bytes, "confidential");
        assert_eq!(second.filename, "r-confidential.docx");
        let read = read_markings(&second.filename, "", &second.bytes);
        assert_eq!(
            read.found,
            vec![mip(CONFIDENTIAL_GUID, Some("Confidential"))]
        );
        let text = detection_text("r.docx", "", &second.bytes, 10_000).unwrap();
        assert_eq!(text.matches("CONFIDENTIAL").count(), 1);
        assert!(!text.contains("INTERNAL"));
        assert_parts_well_formed(&second.bytes);
    }

    #[test]
    fn a_label_already_in_the_document_is_replaced_not_added_to() {
        let stamped = stamp("m.docx", &fixture("mip_confidential.docx"), "internal");
        let read = read_markings(&stamped.filename, "", &stamped.bytes);
        assert_eq!(read.found, vec![mip(INTERNAL_GUID, Some("Internal"))]);
        let custom = part(&stamped.bytes, "docProps/custom.xml");
        assert!(!custom.contains(CONFIDENTIAL_GUID));
    }

    #[test]
    fn label_info_is_rewritten_for_the_new_label_when_present() {
        let stamped = stamp(
            "l.docx",
            &fixture("labelinfo_internal.docx"),
            "confidential",
        );
        let read = read_markings(&stamped.filename, "", &stamped.bytes);
        assert_eq!(
            read.found,
            vec![mip(CONFIDENTIAL_GUID, Some("Confidential"))]
        );
    }

    #[test]
    fn an_xlsx_gets_the_label_but_no_header() {
        let stamped = stamp("sheet.xlsx", &fixture("mip_confidential.xlsx"), "internal");
        assert_eq!(stamped.filename, "sheet-internal.xlsx");
        assert_eq!(
            label_of(&stamped.filename, &stamped.bytes)
                .level_id
                .as_deref(),
            Some("internal")
        );
        assert_parts_well_formed(&stamped.bytes);
    }

    #[test]
    fn a_pptx_built_from_scratch_gets_a_custom_properties_part() {
        let pptx = build(&[
            ("[Content_Types].xml", br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/></Types>"#),
            ("_rels/.rels", br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/></Relationships>"#),
            ("ppt/presentation.xml", b"<p:presentation xmlns:p=\"x\"/>"),
        ]);
        let stamped = stamp("deck.pptx", &pptx, "confidential");
        assert_eq!(
            label_of("deck.pptx", &stamped.bytes).level_id.as_deref(),
            Some("confidential")
        );
        assert_parts_well_formed(&stamped.bytes);
        assert_eq!(
            all_parts(&stamped.bytes)["ppt/presentation.xml"],
            all_parts(&pptx)["ppt/presentation.xml"]
        );
    }

    #[test]
    fn an_existing_word_header_keeps_its_text_and_gets_the_marking_first() {
        let docx = build(&[
            ("[Content_Types].xml", br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/></Types>"#),
            ("_rels/.rels", br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"></Relationships>"#),
            ("word/document.xml", br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:p><w:pPr><w:sectPr><w:headerReference w:type="default" r:id="rId7"/></w:sectPr></w:pPr><w:r><w:t>One</w:t></w:r></w:p><w:p><w:r><w:t>Two</w:t></w:r></w:p><w:sectPr/></w:body></w:document>"#),
            ("word/_rels/document.xml.rels", br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId7" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header7.xml"/></Relationships>"#),
            ("word/header7.xml", br#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>Letterhead</w:t></w:r></w:p></w:hdr>"#),
        ]);
        let stamped = stamp("l.docx", &docx, "confidential");
        assert_parts_well_formed(&stamped.bytes);
        let header = part(&stamped.bytes, "word/header7.xml");
        assert!(header.find("CONFIDENTIAL").unwrap() < header.find("Letterhead").unwrap());
        let document = part(&stamped.bytes, "word/document.xml");
        assert!(
            document.contains("<w:sectPr><w:headerReference"),
            "the last section gets its own header"
        );
        assert!(
            part(&stamped.bytes, "word/_rels/document.xml.rels")
                .contains("header-classification.xml")
        );
        let again = stamp(&stamped.filename, &stamped.bytes, "confidential");
        assert_eq!(
            part(&again.bytes, "word/header7.xml")
                .matches("CONFIDENTIAL")
                .count(),
            1
        );
    }

    #[test]
    fn an_odt_gets_the_tscp_category_of_the_level() {
        let stamped = stamp("note.odt", &fixture("tscp_confidential.odt"), "internal");
        assert_eq!(stamped.filename, "note-internal.odt");
        let read = read_markings(&stamped.filename, "", &stamped.bytes);
        assert_eq!(
            read.found,
            vec![Marking {
                kind: MarkingKind::Tscp,
                value: "Internal".into(),
                name: None
            }]
        );
        assert_eq!(
            label_of(&stamped.filename, &stamped.bytes)
                .level_id
                .as_deref(),
            Some("internal")
        );
        let names: Vec<String> = {
            let archive = zip::ZipArchive::new(std::io::Cursor::new(&stamped.bytes[..])).unwrap();
            archive
                .file_names()
                .filter_map(|n| n.ok().map(|n| n.into_owned()))
                .collect()
        };
        assert_eq!(names[0], "mimetype");
        assert_eq!(
            all_parts(&stamped.bytes)["content.xml"],
            all_parts(&fixture("tscp_confidential.odt"))["content.xml"]
        );
    }

    #[test]
    fn a_labelled_pdf_gets_a_different_label_by_incremental_update() {
        let original = fixture("mip_internal.pdf");
        let stamped = stamp("scan.pdf", &original, "confidential");
        assert_eq!(stamped.filename, "scan-confidential.pdf");
        assert!(
            stamped.bytes.starts_with(&original),
            "the original bytes stay in place"
        );
        let read = read_markings(&stamped.filename, "", &stamped.bytes);
        assert_eq!(
            read.found,
            vec![mip(CONFIDENTIAL_GUID, Some("Confidential"))]
        );
        assert_eq!(
            label_of("scan.pdf", &stamped.bytes).level_id.as_deref(),
            Some("confidential")
        );
        assert_pdf_structure(&stamped.bytes, original.len());
    }

    #[test]
    fn an_unlabelled_pdf_gets_a_label_and_stays_a_readable_pdf() {
        let original = crate::server::pdf::test_support::hello_pdf();
        let stamped = stamp("hello.pdf", &original, "internal");
        assert_eq!(
            label_of("hello.pdf", &stamped.bytes).level_id.as_deref(),
            Some("internal")
        );
        assert_pdf_structure(&stamped.bytes, original.len());
        let text = detection_text("hello.pdf", "application/pdf", &stamped.bytes, 1000).unwrap();
        assert!(
            text.contains("Hello PDF"),
            "a PDF reader still opens it: {text:?}"
        );
        let twice = stamp(&stamped.filename, &stamped.bytes, "confidential");
        assert_eq!(read_markings("x.pdf", "", &twice.bytes).found.len(), 1);
        assert_pdf_structure(&twice.bytes, stamped.bytes.len());
    }

    fn assert_pdf_structure(pdf: &[u8], original_len: usize) {
        let tail = String::from_utf8_lossy(&pdf[original_len..]).into_owned();
        let startxref: usize = tail
            .rsplit("startxref")
            .next()
            .unwrap()
            .trim()
            .trim_end_matches("%%EOF")
            .trim()
            .parse()
            .unwrap();
        assert!(
            pdf[startxref..].starts_with(b"xref"),
            "startxref points at the xref keyword"
        );
        let section = String::from_utf8_lossy(&pdf[startxref..]).into_owned();
        let mut lines = section.lines().skip(1);
        while let Some(header) = lines.next() {
            let Some((first, count)) = header.split_once(' ') else {
                break;
            };
            let (Ok(first), Ok(count)) = (first.parse::<usize>(), count.parse::<usize>()) else {
                break;
            };
            for entry in 0..count {
                let line = lines.next().unwrap();
                let offset: usize = line[..10].parse().unwrap();
                let object = format!("{} ", first + entry);
                assert!(
                    String::from_utf8_lossy(&pdf[offset..offset + 20]).starts_with(&object),
                    "xref entry {line:?} must point at object {first}"
                );
                assert_eq!(line.len(), 19, "entries are 20 bytes with the newline");
            }
        }
        assert!(
            section.contains("/Prev "),
            "the new trailer chains to the old xref"
        );
    }

    #[test]
    fn a_pdf_that_cannot_be_updated_safely_is_left_as_it_was() {
        let mut encrypted = crate::server::pdf::test_support::hello_pdf();
        let at = encrypted.windows(5).rposition(|w| w == b"/Root").unwrap();
        encrypted.splice(at..at, b"/Encrypt 9 0 R ".iter().copied());
        assert_eq!(stamp("e.pdf", &encrypted, "internal").bytes, encrypted);

        let mut xref_stream = crate::server::pdf::test_support::hello_pdf();
        let at = xref_stream.windows(4).rposition(|w| w == b"xref").unwrap();
        xref_stream.splice(at..at + 4, b"fxrf".iter().copied());
        assert_eq!(stamp("x.pdf", &xref_stream, "internal").bytes, xref_stream);
        assert_eq!(
            stamp("x.pdf", &xref_stream, "internal").filename,
            "x-internal.pdf"
        );
    }

    #[test]
    fn markdown_and_text_get_the_marking_as_a_first_line_and_it_is_replaced_not_stacked() {
        let stamped = stamp("notes.md", b"# Title\n\nBody\n", "confidential");
        assert_eq!(
            String::from_utf8(stamped.bytes.clone()).unwrap(),
            "CONFIDENTIAL\n\n# Title\n\nBody\n"
        );
        assert_eq!(stamped.filename, "notes-confidential.md");
        let restamped = stamp(&stamped.filename, &stamped.bytes, "internal");
        assert_eq!(
            String::from_utf8(restamped.bytes).unwrap(),
            "INTERNAL\n\n# Title\n\nBody\n"
        );
        assert_eq!(restamped.filename, "notes-internal.md");
        assert_eq!(stamp("a.txt", b"x", "internal").bytes, b"INTERNAL\n\nx");
    }

    #[test]
    fn html_gets_a_meta_tag_and_an_escaped_banner_and_a_restamp_replaces_both() {
        let mut scheme_levels = scheme().levels().to_vec();
        scheme_levels[2].marking_text = "A & <B>".into();
        let scheme = Scheme::new(scheme_levels, Vec::new());
        let level = scheme.level("confidential").unwrap();
        let html = b"<html><head><title>t</title></head><body class=\"x\"><p>hi</p></body></html>";
        let stamped =
            stamp_output("p.html", "text/html", html, level, &scheme, &options()).unwrap();
        let out = String::from_utf8(stamped.bytes.clone()).unwrap();
        assert!(out.contains("<meta name=\"classification\" content=\"Confidential\">"));
        assert!(out.contains("A &amp; &lt;B&gt;</div>"));
        assert!(
            out.find("<body class=\"x\">").unwrap()
                < out.find("data-classification-banner").unwrap()
        );
        let again = stamp_output(
            "p-confidential.html",
            "text/html",
            &stamped.bytes,
            level,
            &scheme,
            &options(),
        )
        .unwrap();
        let out = String::from_utf8(again.bytes).unwrap();
        assert_eq!(out.matches("data-classification-banner").count(), 1);
        assert_eq!(out.matches("name=\"classification\"").count(), 1);
    }

    #[test]
    fn json_csv_and_images_only_get_the_name_tag() {
        let json = fixture("offer-confidential.json");
        let stamped = stamp("offer.json", &json, "confidential");
        assert_eq!(stamped.bytes, json);
        assert_eq!(stamped.filename, "offer-confidential.json");
        assert_eq!(
            stamp("offer-internal.csv", b"a,b\n", "confidential").filename,
            "offer-confidential.csv"
        );
        assert_eq!(
            stamp("p.png", &[0x89, b'P'], "internal").bytes,
            [0x89, b'P']
        );
    }

    #[test]
    fn the_lowest_level_and_an_unmarkable_level_leave_everything_alone() {
        let scheme = scheme();
        let original = fixture("unlabelled.docx");
        let public = stamp_output(
            "a.docx",
            "",
            &original,
            scheme.level("public").unwrap(),
            &scheme,
            &options(),
        )
        .unwrap();
        assert_eq!(
            (public.filename.as_str(), public.bytes.as_slice()),
            ("a.docx", original.as_slice())
        );
        let bare = level("bare", 5, "Bare", "", "", "");
        let out = stamp_output("a.docx", "", &original, &bare, &scheme, &options()).unwrap();
        assert_eq!(
            (out.filename.as_str(), out.bytes.as_slice()),
            ("a.docx", original.as_slice())
        );
    }

    #[test]
    fn a_level_without_a_tag_leaves_the_name_and_each_option_gates_its_part() {
        let stamped = stamp("report.docx", &fixture("unlabelled.docx"), "strict");
        assert_eq!(stamped.filename, "report.docx");
        let off = StampOptions {
            visible: false,
            metadata: false,
            filename: false,
            mip_tenant_id: TENANT.into(),
        };
        let scheme = scheme();
        let out = stamp_output(
            "r.docx",
            "",
            &fixture("unlabelled.docx"),
            scheme.level("confidential").unwrap(),
            &scheme,
            &off,
        )
        .unwrap();
        assert_eq!(out.filename, "r.docx");
        assert_eq!(out.bytes, fixture("unlabelled.docx"));
    }

    #[test]
    fn a_damaged_package_is_an_error_that_says_what_to_do() {
        let damaged = build(&[("[Content_Types].xml", b"<Types></Types>")]);
        let scheme = scheme();
        let err = stamp_output(
            "a.docx",
            "",
            &damaged,
            scheme.level("confidential").unwrap(),
            &scheme,
            &options(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("generate it again"));
    }

    #[test]
    fn the_stamped_docx_and_pdf_are_written_for_a_manual_look_in_an_office_suite() {
        let dir = std::env::temp_dir().join("aiplane-classification-stamps");
        std::fs::create_dir_all(&dir).unwrap();
        let docx = stamp("stamped.docx", &fixture("unlabelled.docx"), "confidential");
        let pdf = stamp("stamped.pdf", &fixture("mip_internal.pdf"), "confidential");
        for stamped in [&docx, &pdf] {
            let path = dir.join(&stamped.filename);
            std::fs::write(&path, &stamped.bytes).unwrap();
            println!("stamped file: {}", path.display());
        }
    }
}
