// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The built-in patterns a data class can switch on. They run locally and
//! report which pattern matched, never what text matched.

use std::sync::LazyLock;

use regex::Regex;

pub const EMAIL: &str = "email";
pub const IBAN: &str = "iban";
pub const PAYMENT_CARD: &str = "payment_card";
pub const DE_TAX_ID: &str = "de_tax_id";

/// Every id `find_patterns` understands.
pub const PATTERN_IDS: [&str; 4] = [EMAIL, IBAN, PAYMENT_CARD, DE_TAX_ID];

static EMAIL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[A-Za-z0-9._%+\-]+@[A-Za-z0-9\-]+(?:\.[A-Za-z0-9\-]+)*\.[A-Za-z]{2,}")
        .expect("static pattern")
});

static IBAN_START_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b[A-Z]{2}[0-9]{2}").expect("static pattern"));

/// The ids of the requested patterns that occur in `text`, in request order.
/// An id this module does not know is ignored.
pub fn find_patterns(text: &str, ids: &[&str]) -> Vec<&'static str> {
    let mut found = Vec::new();
    for id in ids {
        let Some(&known) = PATTERN_IDS.iter().find(|known| *known == id) else {
            continue;
        };
        if found.contains(&known) {
            continue;
        }
        let hit = match known {
            EMAIL => EMAIL_RE.is_match(text),
            IBAN => contains_iban(text),
            PAYMENT_CARD => digit_runs(text)
                .iter()
                .any(|digits| (13..=19).contains(&digits.len()) && luhn_valid(digits)),
            _ => digit_runs(text)
                .iter()
                .any(|digits| digits.len() == 11 && de_tax_id_valid(digits)),
        };
        if hit {
            found.push(known);
        }
    }
    found
}

fn contains_iban(text: &str) -> bool {
    IBAN_START_RE.find_iter(text).any(|start| {
        let mut compact = String::new();
        let mut chars = text[start.start()..].chars().peekable();
        let mut previous_space = false;
        while let Some(c) = chars.next() {
            if c.is_ascii_alphanumeric() {
                previous_space = false;
                compact.push(c.to_ascii_uppercase());
                let token_ends = chars
                    .peek()
                    .is_none_or(|next| !next.is_ascii_alphanumeric());
                if token_ends && (15..=34).contains(&compact.len()) && iban_mod97(&compact) {
                    return true;
                }
                if compact.len() >= 34 {
                    return false;
                }
            } else if c == ' ' && !previous_space {
                previous_space = true;
            } else {
                return false;
            }
        }
        false
    })
}

fn iban_mod97(iban: &str) -> bool {
    let (head, tail) = iban.split_at(4);
    let mut remainder = 0u32;
    for c in tail.chars().chain(head.chars()) {
        let Some(value) = c.to_digit(36) else {
            return false;
        };
        remainder = if value >= 10 {
            (remainder * 100 + value) % 97
        } else {
            (remainder * 10 + value) % 97
        };
    }
    remainder == 1
}

/// Every digit string a reader would take for one number: a group of digits,
/// or several groups joined by single spaces or dashes, in every contiguous
/// combination up to 19 digits.
fn digit_runs(text: &str) -> Vec<String> {
    let mut groups: Vec<(usize, usize)> = Vec::new();
    let mut start = None;
    for (i, c) in text.char_indices() {
        match (c.is_ascii_digit(), start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                groups.push((s, i));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        groups.push((s, text.len()));
    }
    let joined = |a: usize, b: usize| {
        let gap = &text[groups[a].1..groups[b].0];
        gap == " " || gap == "-"
    };
    let mut runs = Vec::new();
    for first in 0..groups.len() {
        let mut digits = String::new();
        for next in first..groups.len() {
            if next > first && !joined(next - 1, next) {
                break;
            }
            digits.push_str(&text[groups[next].0..groups[next].1]);
            if digits.len() > 19 {
                break;
            }
            runs.push(digits.clone());
        }
    }
    runs
}

fn luhn_valid(digits: &str) -> bool {
    let sum: u32 = digits
        .chars()
        .rev()
        .enumerate()
        .map(|(i, c)| {
            let d = c.to_digit(10).unwrap_or(0);
            if i % 2 == 1 {
                let doubled = d * 2;
                if doubled > 9 { doubled - 9 } else { doubled }
            } else {
                d
            }
        })
        .sum();
    sum.is_multiple_of(10)
}

/// German Steuerliche Identifikationsnummer: eleven digits, no leading zero,
/// among the first ten exactly one digit occurs two or three times (never
/// three in a row) and the rest at most once, and an ISO 7064 MOD 11,10
/// check digit.
fn de_tax_id_valid(digits: &str) -> bool {
    let d: Vec<u32> = digits.chars().filter_map(|c| c.to_digit(10)).collect();
    if d.len() != 11 || d[0] == 0 {
        return false;
    }
    let mut counts = [0u8; 10];
    for &digit in &d[..10] {
        counts[digit as usize] += 1;
    }
    let repeated: Vec<u8> = counts.iter().copied().filter(|&n| n > 1).collect();
    if repeated.len() != 1 || repeated[0] > 3 {
        return false;
    }
    if d[..10].windows(3).any(|w| w[0] == w[1] && w[1] == w[2]) {
        return false;
    }
    let mut product = 10;
    for &digit in &d[..10] {
        let mut sum = (digit + product) % 10;
        if sum == 0 {
            sum = 10;
        }
        product = (2 * sum) % 11;
    }
    let check = match 11 - product {
        10 => 0,
        other => other,
    };
    check == d[10]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hits(text: &str, id: &str) -> bool {
        !find_patterns(text, &[id]).is_empty()
    }

    #[test]
    fn an_email_address_is_found_and_a_bare_at_sign_is_not() {
        assert!(hits("write to jane.roe+tax@example.co.uk today", EMAIL));
        assert!(!hits("meet @ noon, price 5@3", EMAIL));
    }

    #[test]
    fn an_iban_needs_the_mod_97_check_and_may_carry_spaces() {
        assert!(hits("IBAN DE89 3704 0044 0532 0130 00 bitte", IBAN));
        assert!(hits("de89370400440532013000", IBAN));
        assert!(hits("GB82 WEST 1234 5698 7654 32 Bank", IBAN));
        assert!(!hits("DE89 3704 0044 0532 0130 01", IBAN));
        assert!(!hits("DE00 0000", IBAN));
    }

    #[test]
    fn a_payment_card_needs_the_luhn_check_in_13_to_19_digits() {
        assert!(hits("card 4111 1111 1111 1111 exp 12/30", PAYMENT_CARD));
        assert!(hits("4111-1111-1111-1111", PAYMENT_CARD));
        assert!(hits("5500005555555559", PAYMENT_CARD));
        assert!(!hits("4111 1111 1111 1112", PAYMENT_CARD));
        assert!(!hits("order 1234567890", PAYMENT_CARD));
    }

    #[test]
    fn a_card_after_other_numbers_is_still_found() {
        assert!(hits("2026 10 30 4111111111111111", PAYMENT_CARD));
    }

    #[test]
    fn a_german_tax_id_needs_the_repetition_rule_and_the_check_digit() {
        assert!(hits("Steuer-ID 65929970489", DE_TAX_ID));
        assert!(hits("86 095 742 719", DE_TAX_ID));
        assert!(hits("47036892816", DE_TAX_ID));
        assert!(!hits("65929970480", DE_TAX_ID), "wrong check digit");
        assert!(!hits("01234567891", DE_TAX_ID), "leading zero");
        assert!(!hits("12345678901", DE_TAX_ID), "no repeated digit");
        assert!(!hits("11123456785", DE_TAX_ID), "three in a row");
    }

    #[test]
    fn only_the_requested_known_patterns_are_reported_once_and_in_order() {
        let text = "a@b.example 4111111111111111";
        assert_eq!(
            find_patterns(text, &[PAYMENT_CARD, "nope", EMAIL, PAYMENT_CARD]),
            vec![PAYMENT_CARD, EMAIL]
        );
        assert!(find_patterns(text, &[IBAN]).is_empty());
        assert!(find_patterns(text, &[]).is_empty());
    }
}
