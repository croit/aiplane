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
pub const DE_SOCIAL_SECURITY: &str = "de_social_security";
pub const US_SSN: &str = "us_ssn";
pub const CREDENTIALS: &str = "credentials";

/// Every id `find_patterns` understands.
pub const PATTERN_IDS: [&str; 7] = [
    EMAIL,
    IBAN,
    PAYMENT_CARD,
    DE_TAX_ID,
    DE_SOCIAL_SECURITY,
    US_SSN,
    CREDENTIALS,
];

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
            DE_TAX_ID => digit_runs(text)
                .iter()
                .any(|digits| digits.len() == 11 && de_tax_id_valid(digits)),
            DE_SOCIAL_SECURITY => contains_de_social_security(text),
            US_SSN => contains_us_ssn(text),
            _ => contains_credentials(text),
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

static DE_SOCIAL_SECURITY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b([0-9]{2}) ?([0-9]{6}) ?([A-Z]) ?([0-9]{2}) ?([0-9])\b")
        .expect("static pattern")
});

fn contains_de_social_security(text: &str) -> bool {
    DE_SOCIAL_SECURITY_RE
        .captures_iter(text)
        .any(|c| de_social_security_valid(&c[1], &c[2], &c[3], &c[4], &c[5]))
}

/// German Sozialversicherungsnummer: area number 02 to 89, a plausible birth
/// date, the initial of the birth name, a serial and the check digit over the
/// twelve digits the initial turns into (A = 01 .. Z = 26), weighted
/// 2,1,2,5,7,1,2,1,2,1,2,1, summing the digits of every product.
fn de_social_security_valid(
    area: &str,
    birth_date: &str,
    letter: &str,
    serial: &str,
    check: &str,
) -> bool {
    let number = |s: &str| s.parse::<u32>().unwrap_or(0);
    let (day, month) = (number(&birth_date[..2]), number(&birth_date[2..4]));
    if !(2..=89).contains(&number(area)) || !(1..=31).contains(&day) || !(1..=12).contains(&month) {
        return false;
    }
    let initial = letter.to_ascii_uppercase().as_bytes()[0] - b'A' + 1;
    let digits: Vec<u32> = [area, birth_date]
        .iter()
        .flat_map(|part| part.chars())
        .chain(format!("{initial:02}").chars())
        .chain(serial.chars())
        .map(|c| c.to_digit(10).unwrap_or(0))
        .collect();
    const WEIGHTS: [u32; 12] = [2, 1, 2, 5, 7, 1, 2, 1, 2, 1, 2, 1];
    let sum: u32 = digits
        .iter()
        .zip(WEIGHTS)
        .map(|(digit, weight)| {
            let product = digit * weight;
            product / 10 + product % 10
        })
        .sum();
    sum % 10 == number(check)
}

static US_SSN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b([0-9]{3})-([0-9]{2})-([0-9]{4})\b").expect("static pattern"));

fn contains_us_ssn(text: &str) -> bool {
    US_SSN_RE.captures_iter(text).any(|c| {
        let whole = c.get(0).expect("whole match");
        let dash_before = text[..whole.start()].ends_with('-');
        let dash_after = text[whole.end()..].starts_with('-');
        let area: u32 = c[1].parse().unwrap_or(0);
        !dash_before
            && !dash_after
            && area != 0
            && area != 666
            && area < 900
            && &c[2] != "00"
            && &c[3] != "0000"
    })
}

/// Secrets with a fixed, self-identifying shape. Each matcher is anchored on
/// the vendor's own prefix, so ordinary prose, hashes and identifiers pass.
static CREDENTIAL_RES: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r"-----BEGIN [A-Z ]*PRIVATE KEY-----",
        r"\b(?:AKIA|ASIA)[A-Z0-9]{16}\b",
        r"\bgh[pousr]_[A-Za-z0-9]{36}\b",
        r"\bgithub_pat_[A-Za-z0-9_]{22,}",
        r"\bxox[baprs]-[A-Za-z0-9-]{10,}",
        r"\beyJ[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}",
    ]
    .iter()
    .map(|pattern| Regex::new(pattern).expect("static pattern"))
    .collect()
});

static API_KEY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bsk-(?:ant-|proj-)?([A-Za-z0-9_-]{20,})").expect("static pattern")
});

static URL_CREDENTIALS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b[A-Za-z][A-Za-z0-9+.\-]*://[^\s/:@]+:([^\s/@]+)@[^\s/]+")
        .expect("static pattern")
});

fn contains_credentials(text: &str) -> bool {
    CREDENTIAL_RES.iter().any(|re| re.is_match(text))
        // A real key mixes in digits; a hyphenated phrase starting "sk-" does not.
        || API_KEY_RE
            .captures_iter(text)
            .any(|c| c[1].chars().any(|ch| ch.is_ascii_digit()))
        || URL_CREDENTIALS_RE
            .captures_iter(text)
            .any(|c| !is_placeholder_password(&c[1]))
}

fn is_placeholder_password(password: &str) -> bool {
    password.starts_with(['<', '{', '$', '%', '['])
        || password.chars().all(|c| matches!(c, '*' | 'x' | 'X'))
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
    fn a_german_social_security_number_needs_the_check_digit_and_a_plausible_date() {
        assert!(hits("SVNR 15070649C103", DE_SOCIAL_SECURITY));
        assert!(hits("15 070649 C 10 3", DE_SOCIAL_SECURITY));
        assert!(hits("15070649c103", DE_SOCIAL_SECURITY));
        assert!(
            !hits("15070649C104", DE_SOCIAL_SECURITY),
            "wrong check digit"
        );
        assert!(
            !hits("15320649C101", DE_SOCIAL_SECURITY),
            "month 06 day 32 is no date"
        );
        assert!(!hits("00070649C103", DE_SOCIAL_SECURITY), "area 00");
        assert!(!hits("15070649C10", DE_SOCIAL_SECURITY), "too short");
        assert!(
            !hits("115070649C103", DE_SOCIAL_SECURITY),
            "inside a longer number"
        );
    }

    #[test]
    fn a_us_ssn_needs_dashes_and_valid_parts() {
        assert!(hits("SSN 123-45-6789.", US_SSN));
        assert!(!hits("123456789", US_SSN), "bare digits");
        assert!(!hits("000-12-3456", US_SSN), "area 000");
        assert!(!hits("666-12-3456", US_SSN), "area 666");
        assert!(!hits("900-12-3456", US_SSN), "area 900");
        assert!(!hits("123-00-6789", US_SSN), "group 00");
        assert!(!hits("123-45-0000", US_SSN), "serial 0000");
        assert!(
            !hits("1-123-45-6789-1", US_SSN),
            "part of a longer dashed number"
        );
    }

    #[test]
    fn credentials_are_found_by_their_vendor_shapes() {
        // Split with concat! so the secret scanner does not flag these fixtures.
        let positives = [
            concat!("-----BEGIN RSA PRIV", "ATE KEY-----"),
            concat!("-----BEGIN PRIV", "ATE KEY-----"),
            concat!("key AK", "IAIOSFODNN7EXAMPLE here"),
            concat!("AS", "IAY34FZKBOKMUTVV7A"),
            concat!("gh", "p_aB3dE6gH9jK2mN5pQ8sT1vW4yZ7cF0hJ3kLm"),
            concat!(
                "github",
                "_pat_11ABCDEFG0abcdefghijkl_mnopqrstuvwxyz0123456789"
            ),
            concat!("s", "k-proj-abcDEF1234567890abcdEFGH"),
            concat!("s", "k-ant-api03-abcdefghij1234567890"),
            concat!("OPENAI_API_KEY=s", "k-1234567890abcdefghij"),
            concat!("xo", "xb-123456789012-abcdefghijkl"),
            concat!(
                "ey",
                "JhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0In0.dBjftJeZ4CVPmB92K27uhbUJU1p1r"
            ),
            concat!("postgres://admin:", "s3cret@db.internal:5432/app"),
        ];
        for text in positives {
            assert!(hits(text, CREDENTIALS), "{text}");
        }
    }

    #[test]
    fn near_miss_credentials_are_not_found() {
        let negatives = [
            "-----BEGIN CERTIFICATE-----",
            "-----BEGIN PUBLIC KEY-----",
            "AKIA is the prefix, AKIAshort",
            "AKIAIOSFODNN7EXAMPLE1234",
            "ghp_tooshort",
            "sk-learn is a library",
            "the sk-learn-compatible-estimator-wrapper-class is fine",
            "risk-adjusted-return-estimates-2025",
            "xoxb-short",
            "eyJhbGci.short",
            "https://example.com:8080/path",
            "https://user@example.com/",
            "postgres://user:@localhost/db",
            "postgres://user:<password>@localhost/db",
            "postgres://user:****@localhost/db",
            "password=hunter2",
        ];
        for text in negatives {
            assert!(!hits(text, CREDENTIALS), "{text}");
        }
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
