// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! End-to-end typst compile test against the bundled example
//! template. Requires the `typst` CLI on PATH; gated on
//! `typst --version` succeeding so machines without typst skip
//! gracefully instead of failing the suite.
//!
//! What we're pinning here is the whole compile-and-handoff path:
//! discovery picks up the example, `typst::compile` produces real
//! PDF + PNG bytes, and the bytes look right (PDF magic bytes,
//! PNG signature, source file round-tripped).

use aiplane_features::server::typst as gw_typst;

/// Skip the test body when typst isn't installed; lets CI machines
/// without typst run the rest of the suite.
fn typst_available() -> bool {
    std::process::Command::new("typst")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn example_dir() -> std::path::PathBuf {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    // crates/gateway → workspace root → examples
    manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .join("examples/typst-templates")
}

#[tokio::test]
async fn compiles_example_letter_template() {
    if !typst_available() {
        eprintln!("skipping: typst CLI not on PATH");
        return;
    }
    let templates = gw_typst::discover_templates(&example_dir()).expect("discover");
    let letter = templates
        .iter()
        .find(|t| t.id == "letter")
        .expect("letter template not found under examples/typst-templates");
    let inputs = vec![
        ("recipient_name".to_string(), "Acme Corp".to_string()),
        (
            "recipient_address".to_string(),
            "123 Main St\nSpringfield, USA".to_string(),
        ),
        ("subject".to_string(), "Integration test".to_string()),
        (
            "body".to_string(),
            "This letter is produced by the typst_compile integration test.".to_string(),
        ),
        ("sender_name".to_string(), "Jane Doe".to_string()),
    ];
    let rendered = gw_typst::compile(letter, &inputs, 1, None)
        .await
        .expect("compile");
    // PDF magic: "%PDF-"
    assert!(
        rendered.pdf.starts_with(b"%PDF-"),
        "PDF magic bytes missing"
    );
    assert!(rendered.pdf.len() > 1024, "pdf suspiciously small");
    // The letter sets `font: "Urbanist"`, shipped in the template's
    // `fonts/` subdir. `compile` must hand typst `--font-path` so the
    // brand typeface is found and embedded — otherwise typst warns
    // "unknown font family" and silently falls back. An embedded subset
    // carries the family name in its font descriptor, so the bytes of a
    // correctly-rendered PDF contain "Urbanist"; a fallback render does
    // not. This pins the --font-path wiring against regressions.
    assert!(
        rendered
            .pdf
            .windows(b"Urbanist".len())
            .any(|w| w == b"Urbanist"),
        "Urbanist not embedded — --font-path likely not passed to typst"
    );
    // PNG signature: 89 50 4E 47 0D 0A 1A 0A
    assert_eq!(
        &rendered.png[..8],
        &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
        "PNG signature missing"
    );
    // Source round-trip — the bytes we get back must be the same
    // template.typ on disk, so the model gets a faithful copy to
    // iterate on.
    let on_disk = std::fs::read(letter.root.join(&letter.source_file)).unwrap();
    assert_eq!(rendered.source, on_disk);
}

/// The presentation example is discoverable and renders through the gateway
/// path (`--input deck=<inline JSON>`): its deck field carries the whole deck
/// as a JSON string, exercising the inline branch of the template's input
/// handling and the shipped neutral assets (logo, icons, fonts, gradient
/// backgrounds). Also confirms an escaped `@` in an eval'd prose field renders.
#[tokio::test]
async fn compiles_example_presentation_template() {
    if !typst_available() {
        eprintln!("skipping: typst CLI not on PATH");
        return;
    }
    let templates = gw_typst::discover_templates(&example_dir()).expect("discover");
    let deck = templates
        .iter()
        .find(|t| t.id == "presentation")
        .expect("presentation template not found under examples/typst-templates");
    // Minimal inline deck: a cover + a content slide whose body has an escaped
    // email (`\@`) in the eval'd markup, plus a card with a built-in icon.
    let deck_json = r#"{
        "deck_title": "Example Corp",
        "theme": "dark",
        "slides": [
            {"layout": "cover", "title": "Hello", "subtitle": "A sample deck"},
            {"layout": "content", "title": "Contact",
             "body": "Reach us at hi\\@example.com for *details*."},
            {"layout": "cards", "title": "Why us",
             "cards": [{"title": "Fast", "body": "Quick.", "icon": "zap"}]}
        ]
    }"#;
    let inputs = vec![("deck".to_string(), deck_json.to_string())];
    let rendered = gw_typst::compile(deck, &inputs, 1, None)
        .await
        .expect("presentation compile");
    assert!(rendered.pdf.starts_with(b"%PDF-"), "PDF magic missing");
    assert!(rendered.pdf.len() > 1024, "pdf suspiciously small");
    assert_eq!(
        &rendered.png[..8],
        &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
        "PNG signature missing"
    );
}

/// One slide in, one slide out — for every combination of the image layouts.
///
/// The `image` layout used to size the picture in absolute centimetres
/// (`large` = 12cm, `full` = 14.5cm) against a content box only ~14.4cm tall, so
/// a title and a caption pushed it over budget and typst — which never
/// paginates a deck for us — quietly spilled the overflow onto extra, chromeless
/// slides: a 7-slide deck came out as a 10-page PDF with a cropped picture and a
/// caption stranded on a blank page. Sizes are fractions of the free space now.
/// Page count is the cheapest way to pin that: any layout that overflows adds a
/// page, so `pages == slides` is the contract.
#[tokio::test]
async fn presentation_image_slides_never_overflow() {
    if !typst_available() {
        eprintln!("skipping: typst CLI not on PATH");
        return;
    }
    let templates = gw_typst::discover_templates(&example_dir()).expect("discover");
    let deck = templates.iter().find(|t| t.id == "presentation").unwrap();
    // The bundled sample ships no photos, so use its logo SVG as the picture —
    // what matters here is the geometry, not the pixels.
    let img = "assets/logo-light.svg";
    let slides: Vec<String> = [
        // `image`: every size, with and without title/caption/frame.
        r#"{"layout":"image","image":"IMG"}"#.to_string(),
        r#"{"layout":"image","image":"IMG","size":"small","title":"T","caption":"C"}"#.to_string(),
        r#"{"layout":"image","image":"IMG","size":"medium","title":"T","caption":"C"}"#.to_string(),
        r#"{"layout":"image","image":"IMG","size":"large","title":"T","caption":"C"}"#.to_string(),
        r#"{"layout":"image","image":"IMG","size":"full","title":"T","caption":"C"}"#.to_string(),
        r#"{"layout":"image","image":"IMG","size":"full","frame":true,"title":"T","caption":"C"}"#
            .to_string(),
        // `media`: bare (edge-to-edge), with text (→ clean image slide), with an
        // opt-in overlay at each position, shrunk, and framed.
        r#"{"layout":"media","image":"IMG"}"#.to_string(),
        r#"{"layout":"media","image":"IMG","title":"T","caption":"C"}"#.to_string(),
        r#"{"layout":"media","image":"IMG","title":"T","caption":"C","overlay_position":"bottom-left"}"#.to_string(),
        r#"{"layout":"media","image":"IMG","title":"T","overlay_position":"bottom-right"}"#
            .to_string(),
        r#"{"layout":"media","image":"IMG","title":"T","caption":"C","overlay_position":"center"}"#
            .to_string(),
        r#"{"layout":"media","image":"IMG","size":"medium","title":"T","caption":"C"}"#.to_string(),
        r#"{"layout":"media","image":"IMG","frame":true,"title":"T","caption":"C"}"#.to_string(),
        // no image at all → the gradient statement fallback, still one slide
        r#"{"layout":"media","title":"T","caption":"C"}"#.to_string(),
    ]
    .iter()
    .map(|s| s.replace("IMG", img))
    .collect();
    let deck_json = format!(
        r#"{{"deck_title":"Overflow probe","slides":[{}]}}"#,
        slides.join(",")
    );
    let rendered = gw_typst::compile(deck, &[("deck".to_string(), deck_json)], 1, None)
        .await
        .expect("image-layout deck compiles");
    // typst writes one uncompressed `/MediaBox` per page; if that ever changes
    // this assert fails loudly rather than silently passing.
    let pages = rendered
        .pdf
        .windows(b"/MediaBox".len())
        .filter(|w| *w == b"/MediaBox")
        .count();
    assert_eq!(
        pages,
        slides.len(),
        "{} slides rendered as {pages} pages — an image layout is overflowing onto extra slides",
        slides.len()
    );
}

#[tokio::test]
async fn compile_surfaces_typst_error_on_bad_input() {
    if !typst_available() {
        eprintln!("skipping: typst CLI not on PATH");
        return;
    }
    let templates = gw_typst::discover_templates(&example_dir()).expect("discover");
    let letter = templates.iter().find(|t| t.id == "letter").unwrap();
    // Omit required `recipient_name`. typst will fail when the
    // template tries to read it from `sys.inputs`; the error must
    // come back as `CompileError::Failed` carrying the stderr so
    // the model can see what's wrong and try again.
    let inputs = vec![
        ("subject".to_string(), "x".to_string()),
        ("body".to_string(), "x".to_string()),
        ("sender_name".to_string(), "x".to_string()),
        ("recipient_address".to_string(), "x".to_string()),
    ];
    let err = gw_typst::compile(letter, &inputs, 1, None)
        .await
        .unwrap_err();
    match err {
        gw_typst::CompileError::Failed(msg) => {
            assert!(
                msg.contains("recipient_name") || msg.contains("error"),
                "unexpected stderr: {msg}"
            );
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}

/// Pins the premise behind the gateway's auto-escape-on-`@` retry (see
/// `typst_render::escape_unescaped_ats`): the example body is `eval`'d as
/// markup, so an unescaped `@` (email/handle) crashes the WHOLE render with
/// the exact ``does not exist in the document`` signature the retry keys on,
/// and escaping it (`\@`) — what the retry does — makes the same input render.
/// If typst ever changes that error text or `@`-handling, this breaks and we
/// learn before the retry silently stops firing.
#[tokio::test]
async fn unescaped_at_crashes_and_escaped_at_renders() {
    if !typst_available() {
        eprintln!("skipping: typst CLI not on PATH");
        return;
    }
    let templates = gw_typst::discover_templates(&example_dir()).expect("discover");
    let letter = templates.iter().find(|t| t.id == "letter").unwrap();
    let with_body = |body: &str| {
        vec![
            ("recipient_name".to_string(), "Acme Co.".to_string()),
            ("recipient_address".to_string(), "123 Main St".to_string()),
            ("subject".to_string(), "Hi".to_string()),
            ("body".to_string(), body.to_string()),
            ("sender_name".to_string(), "Jane Doe".to_string()),
        ]
    };
    // Bare `@` → crash with the signature.
    let err = gw_typst::compile(
        letter,
        &with_body("Dear Ms. Roe,\n\nReach me at jane@example.com."),
        1,
        None,
    )
    .await
    .unwrap_err();
    match err {
        gw_typst::CompileError::Failed(msg) => assert!(
            msg.contains("does not exist in the document"),
            "signature the retry keys on changed: {msg}"
        ),
        other => panic!("expected Failed, got {other:?}"),
    }
    // Escaped `@` → renders (this is what the auto-escape retry produces).
    let rendered = gw_typst::compile(
        letter,
        &with_body("Dear Ms. Roe,\n\nReach me at jane\\@example.com."),
        1,
        None,
    )
    .await
    .expect("escaped @ should compile");
    assert!(rendered.pdf.starts_with(b"%PDF-"), "PDF magic missing");
}

/// Count PDF pages via the uncompressed `/MediaBox` page dictionaries (content
/// streams are Flate-compressed, so text can't be grepped reliably).
fn pdf_pages(pdf: &[u8]) -> usize {
    pdf.windows(b"/MediaBox".len())
        .filter(|w| *w == b"/MediaBox")
        .count()
}

/// Every layout in the catalogue renders exactly one slide each — no layout may
/// overflow onto extra, chromeless pages. One slide per layout (all 15 of the
/// chart / structured-content / compose layouts plus the global fields:
/// deck `meta`, agenda `page`, `stats` `sub`, and the `takeaway` band), so
/// `pages == slides` is the contract.
#[tokio::test]
async fn presentation_all_layouts_render_one_page_each() {
    if !typst_available() {
        eprintln!("skipping: typst CLI not on PATH");
        return;
    }
    let templates = gw_typst::discover_templates(&example_dir()).expect("discover");
    let deck = templates
        .iter()
        .find(|t| t.id == "presentation")
        .expect("presentation template not found under examples/typst-templates");
    let deck_json = r#"{
        "deck_title": "Layout probe",
        "meta": { "author": "Test", "date": "Oct 2026", "confidential": true },
        "theme": "dark",
        "slides": [
            {"layout": "agenda", "title": "Agenda",
             "items": [{"label": "Intro", "note": "short", "page": "03"}]},
            {"layout": "stats", "title": "Stats",
             "stats": [{"value": "42%", "label": "Faster", "note": "vs last q", "sub": "n = 1000"}]},
            {"layout": "hbar", "title": "HBar", "caption": "Benchmarks",
             "categories": ["Alpha", "Beta", "Gamma"],
             "series": [{"name": "Us", "values": [8, 5, 3]}, {"name": "Them", "values": [6, 6, 7]}]},
            {"layout": "bar", "title": "Bar",
             "categories": ["Q1", "Q2", "Q3", "Q4"],
             "series": [{"name": "Revenue", "values": [12, 15, 14, 20]}],
             "callouts": ["", "+25%", "", "+43%"]},
            {"layout": "line", "title": "Line",
             "categories": ["Jan", "Feb", "Mar", "Apr", "May"],
             "series": [{"name": "Uptime", "values": [99.1, 99.4, 99.2, 99.8, 99.9]}]},
            {"layout": "donut", "title": "Donut", "center_label": "100",
             "slices": [{"name": "EU", "value": 50}, {"name": "US", "value": 30}, {"name": "APAC", "value": 20}]},
            {"layout": "matrix", "title": "Matrix",
             "columns": ["Objects", "Tiering", "Compliance"],
             "rows": [{"name": "Ours", "cells": ["✓", "✓", "✓"]},
                      {"name": "Other", "cells": ["✓", "✗", "–"]}],
             "highlight": "Ours"},
            {"layout": "stack", "title": "Stack",
             "layers": [{"label": "UI", "note": "web"}, {"label": "API"}, {"label": "Storage"}],
             "highlight": "Storage", "side": "Why this order: *simplicity*."},
            {"layout": "do-dont", "title": "Do / Don't",
             "left": {"heading": "Do", "points": ["Use objects", "Enable tiering", "Set retention"]},
             "right": {"heading": "Don't", "points": ["Store VMs", "Ignore lifecycle", "Over-provision"]}},
            {"layout": "numbered-cards", "title": "Steps",
             "cards": [{"title": "One", "body": "First."}, {"title": "Two", "body": "Second."}, {"title": "Three"}]},
            {"layout": "persona", "title": "Personas",
             "people": [{"name": "Ada", "role": "CTO", "pains": ["Cost"], "needs": ["Simplicity"],
                         "quote": "We need less.", "message": "Lead with savings."}]},
            {"layout": "findings", "title": "Findings",
             "items": [{"title": "First", "detail": "Detail line."}, {"title": "Second", "detail": "More."}, {"title": "Third"}]},
            {"layout": "qa", "title": "Q&A",
             "items": [{"q": "Why?", "a": "Because *reasons*."}, {"q": "How?", "a": "Simply."}]},
            {"layout": "objection", "title": "Objections",
             "rows": [{"objection": "Too expensive", "response": "It pays for itself."},
                      {"objection": "Migration pain", "response": "We handle it."}]},
            {"layout": "actions", "title": "Actions",
             "rows": [{"action": "Kick off", "owner": "Jane", "status": "Now"},
                      {"action": "Migrate", "owner": "Team", "status": "Next"}]},
            {"layout": "twocol", "title": "Two columns",
             "cols": [{"heading": "Now", "body": "Where we are.", "bullets": ["One", "Two"]},
                      {"heading": "Next", "body": "Where we go."}]},
            {"layout": "compose", "title": "Compose",
             "blocks": [{"type": "stats", "stats": [{"value": "3", "label": "Big"}]},
                        {"type": "text", "content": "A *mixed* slide."},
                        {"type": "callout", "label": "Note", "text": "Composed."}]},
            {"layout": "content", "title": "Takeaway slide", "body": "The point.",
             "takeaway": "Ship it.", "takeaway_label": "Bottom line"}
        ]
    }"#;
    let rendered = gw_typst::compile(
        deck,
        &[("deck".to_string(), deck_json.to_string())],
        1,
        None,
    )
    .await
    .expect("all-layouts deck compiles");
    assert!(rendered.pdf.starts_with(b"%PDF-"), "PDF magic missing");
    let pages = pdf_pages(&rendered.pdf);
    assert_eq!(
        pages, 18,
        "18 slides rendered as {pages} pages — a layout is overflowing onto extra slides"
    );
}

/// The one-pager's structured `blocks` system: every block type renders
/// through the gateway path (`--input blocks=<JSON string>`) and the whole
/// sheet stays on ONE page — the blocks are compact by design.
#[tokio::test]
async fn onepager_all_block_types_render() {
    if !typst_available() {
        eprintln!("skipping: typst CLI not on PATH");
        return;
    }
    let templates = gw_typst::discover_templates(&example_dir()).expect("discover");
    let sheet = templates
        .iter()
        .find(|t| t.id == "onepager")
        .expect("onepager template not found under examples/typst-templates");
    let blocks = r#"[
        {"type": "section", "heading": "Capabilities", "body": "A short *paragraph*."},
        {"type": "text", "content": "A plain paragraph."},
        {"type": "bullets", "items": ["One", "Two"]},
        {"type": "stats", "stats": [{"value": "42%", "label": "Faster", "note": "vs last q"}, {"value": "3x", "label": "Faster still"}]},
        {"type": "table", "columns": ["A", "B"], "rows": [["1", "2"], ["3", "4"]]},
        {"type": "cards", "cards": [{"title": "One", "body": "First."}, {"title": "Two", "body": "Second."}]},
        {"type": "image", "src": "assets/brand-bar.png", "size": "medium", "caption": "Brand bar"},
        {"type": "quote", "text": "A quote.", "attribution": "Someone"},
        {"type": "callout", "label": "Note", "text": "A callout."}
    ]"#;
    let inputs = vec![
        ("title".to_string(), "Block probe".to_string()),
        ("subtitle".to_string(), "All nine block types".to_string()),
        ("body".to_string(), "An intro paragraph.".to_string()),
        ("blocks".to_string(), blocks.to_string()),
    ];
    let rendered = gw_typst::compile(sheet, &inputs, 1, None)
        .await
        .expect("onepager blocks compile");
    assert!(rendered.pdf.starts_with(b"%PDF-"), "PDF magic missing");
    let pages = pdf_pages(&rendered.pdf);
    assert_eq!(
        pages, 1,
        "one-pager with nine blocks rendered as {pages} pages — blocks should stay compact"
    );
}
