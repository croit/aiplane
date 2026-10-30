// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! A small flat view of an XML document: every element with its local name,
//! local-named attributes and the text of itself and its descendants.

use quick_xml::events::{BytesRef, BytesStart, Event};
use quick_xml::{Reader, XmlVersion};

#[derive(Debug, Default, Clone)]
pub(super) struct Element {
    pub name: String,
    pub attributes: Vec<(String, String)>,
    pub text: String,
}

impl Element {
    pub fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

fn element_of(start: &BytesStart<'_>) -> Element {
    let attributes = start
        .attributes()
        .filter_map(Result::ok)
        .filter_map(|attribute| {
            let key = attribute.key.local_name().into_inner().to_string();
            let value = attribute
                .normalized_value(XmlVersion::Implicit1_0)
                .ok()?
                .into_owned();
            Some((key, value))
        })
        .collect();
    Element {
        name: start.local_name().into_inner().to_string(),
        attributes,
        text: String::new(),
    }
}

fn entity(reference: &BytesRef<'_>) -> String {
    if let Ok(Some(c)) = reference.resolve_char_ref() {
        return c.to_string();
    }
    match reference.xml10_content().as_ref() {
        "amp" => "&".into(),
        "lt" => "<".into(),
        "gt" => ">".into(),
        "quot" => "\"".into(),
        "apos" => "'".into(),
        _ => String::new(),
    }
}

/// Every element in document order of its end. `None` when the XML is not
/// well-formed.
pub(super) fn elements(xml: &[u8]) -> Option<Vec<Element>> {
    let mut reader = Reader::from_reader(xml);
    let mut stack: Vec<Element> = Vec::new();
    let mut out = Vec::new();
    loop {
        match reader.read_event().ok()? {
            Event::Start(start) => stack.push(element_of(&start)),
            Event::Empty(start) => {
                let element = element_of(&start);
                out.push(element);
            }
            Event::End(_) => {
                let element = stack.pop()?;
                if let Some(parent) = stack.last_mut() {
                    parent.text.push_str(&element.text);
                }
                out.push(element);
            }
            Event::Text(text) => {
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&text.xml10_content());
                }
            }
            Event::CData(text) => {
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&text.xml10_content());
                }
            }
            Event::GeneralRef(reference) => {
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&entity(&reference));
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    stack.is_empty().then_some(out)
}

/// Append the character data of `xml` to `out`, a line break after each
/// paragraph, row or shared string, until `out` holds `cap` bytes. A part that
/// is not well-formed contributes the text before the break.
pub(super) fn append_text(xml: &[u8], out: &mut String, cap: usize) {
    let mut reader = Reader::from_reader(xml);
    while out.len() < cap {
        match reader.read_event() {
            Ok(Event::Text(text)) => {
                let text = text.xml10_content();
                if !(text.trim().is_empty() && (out.is_empty() || out.ends_with('\n'))) {
                    out.push_str(&text);
                }
            }
            Ok(Event::CData(text)) => out.push_str(&text.xml10_content()),
            Ok(Event::GeneralRef(reference)) => out.push_str(&entity(&reference)),
            Ok(Event::End(end)) => {
                if matches!(
                    end.local_name().into_inner(),
                    "p" | "h" | "tr" | "row" | "si"
                ) {
                    out.push('\n');
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elements_carry_local_names_attributes_and_nested_text() {
        let xml = br#"<a:root xmlns:a="u"><a:item a:name="n&amp;1"><b:v xmlns:b="v">x &lt; y</b:v></a:item><a:empty a:k="1"/></a:root>"#;
        let all = elements(xml).unwrap();
        let item = all.iter().find(|e| e.name == "item").unwrap();
        assert_eq!(item.attribute("name"), Some("n&1"));
        assert_eq!(item.text, "x < y");
        assert_eq!(
            all.iter()
                .find(|e| e.name == "empty")
                .unwrap()
                .attribute("k"),
            Some("1")
        );
    }

    #[test]
    fn text_is_collected_in_paragraphs_and_stops_at_the_cap() {
        let xml = b"<d><p><t>Hello</t> <t>world</t></p><p>Second &amp; last</p></d>";
        let mut all = String::new();
        append_text(xml, &mut all, 1000);
        assert_eq!(all, "Hello world\nSecond & last\n");
        let mut capped = String::new();
        append_text(xml, &mut capped, 5);
        assert!(capped.starts_with("Hello"));
        assert!(!capped.contains("Second"));
    }

    #[test]
    fn malformed_xml_is_none() {
        assert!(elements(b"<a><b></a>").is_none());
        assert!(elements(b"<a>").is_none());
    }
}
