// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Minimal standard (padded) base64.
//!
//! Self-contained, matching the convention the gateway-side codecs already
//! follow: this repo carries small hand-rolled base64 rather than taking a
//! dependency for it. It lives in `shared` so both the gateway and the
//! sandbox-runner can use one copy; the older copies under `aiplane-runtime`
//! and `aiplane-tools` can migrate here whenever they are next touched.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encode to standard, padded base64.
pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);
        out.push(ALPHABET[(b0 >> 2) as usize] as char);
        out.push(ALPHABET[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        out.push(if chunk.len() >= 2 {
            ALPHABET[(((b1 & 0x0f) << 2) | (b2 >> 6)) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() >= 3 {
            ALPHABET[(b2 & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// Decode the standard alphabet, optional padding and ASCII whitespace.
pub fn decode(s: &str) -> Result<Vec<u8>, String> {
    let clean: String = s.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    let bytes = clean.as_bytes();
    let trimmed = bytes.iter().rposition(|b| *b != b'=').map_or(0, |i| i + 1);
    let body = &bytes[..trimmed];

    let mut out = Vec::with_capacity(body.len() * 3 / 4);
    let mut acc: u32 = 0;
    let mut acc_bits: u8 = 0;
    for (idx, &c) in body.iter().enumerate() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return Err(format!("invalid base64 character at position {idx}")),
        };
        acc = (acc << 6) | (v as u32);
        acc_bits += 6;
        if acc_bits >= 8 {
            acc_bits -= 8;
            out.push((acc >> acc_bits) as u8);
            acc &= (1 << acc_bits) - 1;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_existing_attachment_payload_conventions() {
        assert_eq!(decode("YQ==").unwrap(), b"a");
        assert_eq!(decode("YQ").unwrap(), b"a");
        assert_eq!(decode(" Y W J j\n").unwrap(), b"abc");
        assert!(decode("?-_").is_err());
        let all: Vec<_> = (0u8..=255).collect();
        assert_eq!(decode(&encode(&all)).unwrap(), all);
    }

    #[test]
    fn matches_the_rfc_test_vectors() {
        assert_eq!(encode(b""), "");
        assert_eq!(encode(b"f"), "Zg==");
        assert_eq!(encode(b"fo"), "Zm8=");
        assert_eq!(encode(b"foo"), "Zm9v");
        assert_eq!(encode(b"foob"), "Zm9vYg==");
        assert_eq!(encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn encodes_every_byte_value_at_the_expected_length() {
        let all: Vec<u8> = (0u8..=255).collect();
        let e = encode(&all);
        assert_eq!(e.len(), 256usize.div_ceil(3) * 4);
        assert!(
            e.chars()
                .all(|c| c.is_ascii_alphanumeric() || "+/=".contains(c))
        );
    }
}
