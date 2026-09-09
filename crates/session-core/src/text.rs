// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Small text helpers shared across the crates.
//!
//! What is left of the old `render` module: the SPA renders markdown and
//! chat turns itself, so the server-side HTML builders are gone and only
//! this string utility outlived them.

/// Char-bounded truncation with a trailing `…` when the string was cut.
/// Char-based (not byte-based) so it never splits a UTF-8 sequence.
pub fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::truncate_chars;

    #[test]
    fn truncates_on_char_boundaries() {
        assert_eq!(truncate_chars("hello", 10), "hello");
        assert_eq!(truncate_chars("hello", 3), "hel…");
        // Multi-byte: cutting by bytes here would split a UTF-8 sequence.
        assert_eq!(truncate_chars("héllo wörld", 4), "héll…");
    }
}
