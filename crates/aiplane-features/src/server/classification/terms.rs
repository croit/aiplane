// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Word lists: matching terms in text, and reading an administrator's upload.

use std::collections::HashSet;

use regex::{Regex, RegexBuilder};

/// Terms shorter than this many characters match too much to be useful.
pub const MIN_TERM_CHARS: usize = 3;

const COMPILED_SIZE_LIMIT: usize = 256 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum TermError {
    #[error(
        "the word list is not valid UTF-8 text, so it could not be read; save the file as UTF-8 and upload it again"
    )]
    NotUtf8,
    #[error(
        "the word list has no column named `{column}`; the columns are: {available}. Pick one of them"
    )]
    ColumnNotFound { column: String, available: String },
    #[error("the word list is empty or has no header row, so no column could be read")]
    NoHeader,
    #[error(
        "the {count} terms of this word list are too many to match efficiently; split the list or remove terms"
    )]
    TooManyTerms { count: usize },
}

/// Case-insensitive, whole-word matching of many terms at once. Built once
/// per word list, then reused for every text.
#[derive(Debug, Clone)]
pub struct TermMatcher {
    pattern: Option<Regex>,
}

impl TermMatcher {
    /// Terms shorter than [`MIN_TERM_CHARS`] and empty terms are ignored. A
    /// word boundary is any character that is not a letter, digit or
    /// underscore.
    pub fn new(terms: &[String]) -> Result<Self, TermError> {
        let mut lowered: Vec<String> = terms
            .iter()
            .map(|term| term.trim().to_lowercase())
            .filter(|term| term.chars().count() >= MIN_TERM_CHARS)
            .collect();
        lowered.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        lowered.dedup();
        if lowered.is_empty() {
            return Ok(Self { pattern: None });
        }
        let alternation = lowered
            .iter()
            .map(|term| regex::escape(term))
            .collect::<Vec<_>>()
            .join("|");
        let pattern = RegexBuilder::new(&format!(
            r"(?:^|[^\p{{L}}\p{{N}}_])(?:{alternation})(?:$|[^\p{{L}}\p{{N}}_])"
        ))
        .size_limit(COMPILED_SIZE_LIMIT)
        .build()
        .map_err(|_| TermError::TooManyTerms {
            count: lowered.len(),
        })?;
        Ok(Self {
            pattern: Some(pattern),
        })
    }

    pub fn matches(&self, text: &str) -> bool {
        self.pattern
            .as_ref()
            .is_some_and(|pattern| pattern.is_match(&text.to_lowercase()))
    }
}

/// What an uploaded word list holds, for the preview before it is saved.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TermUpload {
    /// The terms that will be matched, without duplicates, in file order.
    pub terms: Vec<String>,
    /// Terms that appeared again, case-insensitively, each reported once.
    pub duplicates: Vec<String>,
    /// Terms under [`MIN_TERM_CHARS`] characters: kept out of `terms`.
    pub too_short: Vec<String>,
}

/// Read a word list. With no `csv_column` the file is plain text, one term
/// per line. With a column name it is CSV: the first row is the header, the
/// delimiter is `,` or, when the header has no comma but a semicolon, `;`,
/// and quoted fields may hold the delimiter, doubled quotes and line breaks.
pub fn parse_term_upload(bytes: &[u8], csv_column: Option<&str>) -> Result<TermUpload, TermError> {
    let text = std::str::from_utf8(bytes).map_err(|_| TermError::NotUtf8)?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let raw: Vec<String> = match csv_column {
        None => text.lines().map(str::to_string).collect(),
        Some(column) => csv_column_values(text, column)?,
    };
    let mut upload = TermUpload::default();
    let mut seen = HashSet::new();
    let mut reported = HashSet::new();
    for term in raw {
        let term = term.trim().to_string();
        if term.is_empty() {
            continue;
        }
        if term.chars().count() < MIN_TERM_CHARS {
            upload.too_short.push(term);
            continue;
        }
        let key = term.to_lowercase();
        if !seen.insert(key.clone()) {
            if reported.insert(key) {
                upload.duplicates.push(term);
            }
            continue;
        }
        upload.terms.push(term);
    }
    Ok(upload)
}

fn csv_column_values(text: &str, column: &str) -> Result<Vec<String>, TermError> {
    let header_line = text.lines().next().ok_or(TermError::NoHeader)?;
    let delimiter = if !header_line.contains(',') && header_line.contains(';') {
        ';'
    } else {
        ','
    };
    let mut rows = parse_csv(text, delimiter).into_iter();
    let header = rows.next().ok_or(TermError::NoHeader)?;
    let index = header
        .iter()
        .position(|name| name.trim().eq_ignore_ascii_case(column.trim()))
        .ok_or_else(|| TermError::ColumnNotFound {
            column: column.to_string(),
            available: header.join(", "),
        })?;
    Ok(rows
        .filter_map(|mut row| (index < row.len()).then(|| row.swap_remove(index)))
        .collect())
}

fn parse_csv(text: &str, delimiter: char) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, quoted) {
            ('"', true) if chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            ('"', true) => quoted = false,
            ('"', false) if field.is_empty() => quoted = true,
            (c, false) if c == delimiter => row.push(std::mem::take(&mut field)),
            ('\r', false) => {}
            ('\n', false) => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            (c, _) => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matcher(terms: &[&str]) -> TermMatcher {
        TermMatcher::new(&terms.iter().map(|t| t.to_string()).collect::<Vec<_>>()).unwrap()
    }

    #[test]
    fn a_term_matches_case_insensitively_on_word_boundaries() {
        let m = matcher(&["Project Falcon", "Müller"]);
        assert!(m.matches("status of PROJECT falcon: green"));
        assert!(m.matches("Herr MÜLLER sagt"));
        assert!(m.matches("(Project Falcon)"));
        assert!(!m.matches("the project falconry club"));
        assert!(!m.matches("Müllerei"));
    }

    #[test]
    fn a_shorter_alternative_still_matches_when_the_longer_one_fails_its_boundary() {
        let m = matcher(&["foo bar", "foo"]);
        assert!(m.matches("foo barn"));
    }

    #[test]
    fn regex_metacharacters_in_a_term_are_literal() {
        let m = matcher(&["a.b+c"]);
        assert!(m.matches("see a.b+c here"));
        assert!(!m.matches("see aXb+c here"));
    }

    #[test]
    fn terms_under_three_characters_and_an_empty_list_match_nothing() {
        assert!(!matcher(&["ab", " "]).matches("ab ab"));
        assert!(!matcher(&[]).matches("anything"));
    }

    #[test]
    fn thousands_of_terms_build_and_match() {
        let terms: Vec<String> = (0..5000).map(|i| format!("customer{i:05}x")).collect();
        let m = TermMatcher::new(&terms).unwrap();
        assert!(m.matches("order for Customer04242X is late"));
        assert!(!m.matches("order for customer04242y is late"));
    }

    #[test]
    fn a_text_upload_trims_skips_empty_lines_and_reports_duplicates_and_short_terms() {
        let upload = parse_term_upload(
            "\u{feff}Falcon \n\n  acme corp\nFALCON\nAB\nXY\nacme corp\nFalcon\n".as_bytes(),
            None,
        )
        .unwrap();
        assert_eq!(upload.terms, vec!["Falcon", "acme corp"]);
        assert_eq!(upload.duplicates, vec!["FALCON", "acme corp"]);
        assert_eq!(upload.too_short, vec!["AB", "XY"]);
    }

    #[test]
    fn a_csv_upload_reads_the_named_column_with_quoted_fields() {
        let csv = "id,customer,note\n1,\"Acme, Inc.\",\"says \"\"hi\"\"\"\n2,Globex,\n3,\"Multi\nline\",x\n";
        let upload = parse_term_upload(csv.as_bytes(), Some("Customer")).unwrap();
        assert_eq!(upload.terms, vec!["Acme, Inc.", "Globex", "Multi\nline"]);
    }

    #[test]
    fn a_semicolon_csv_is_read_like_a_comma_csv() {
        let upload = parse_term_upload(
            "id;name\r\n1;Falcon\r\n2;Osprey\r\n".as_bytes(),
            Some("name"),
        )
        .unwrap();
        assert_eq!(upload.terms, vec!["Falcon", "Osprey"]);
    }

    #[test]
    fn a_missing_column_names_the_columns_that_exist() {
        let err = parse_term_upload(b"id,name\n1,Falcon\n", Some("customer")).unwrap_err();
        assert!(matches!(err, TermError::ColumnNotFound { .. }));
        assert!(err.to_string().contains("id, name"));
    }

    #[test]
    fn a_file_that_is_not_utf8_is_refused() {
        assert!(matches!(
            parse_term_upload(&[0xff, 0xfe, 0x00], None),
            Err(TermError::NotUtf8)
        ));
    }
}
