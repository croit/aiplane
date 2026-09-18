// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Lowercase hex.
//!
//! One copy, because the three call sites that needed it encode hashes that
//! are **stored**: RAG content hashes decide whether a file changed, the
//! sync-token hash is what a webhook is matched against, and the OCR cache key
//! addresses derivatives already on disk. A change in this output is a silent
//! re-index, a dead trigger URL and a cold cache, so it is pinned by a
//! known-answer test rather than left to whatever a digest crate's `LowerHex`
//! happens to do this release.
//!
//! `format!("{:x}", digest)` used to do this inline, until RustCrypto's
//! `digest` 0.11 stopped implementing `LowerHex` on its output type.

/// Encode to lowercase hex, two characters per byte.
pub fn encode_lower(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(DIGITS[(b >> 4) as usize] as char);
        out.push(DIGITS[(b & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_every_nibble() {
        assert_eq!(encode_lower(&[0x00, 0x0f, 0xf0, 0xff]), "000ff0ff");
        assert_eq!(encode_lower(&[]), "");
        assert_eq!(
            encode_lower(b"\x01\x23\x45\x67\x89\xab\xcd\xef"),
            "0123456789abcdef"
        );
    }

    /// Byte for byte what `format!("{:x}", …)` produced before, for every
    /// possible byte — this is the property the stored hashes depend on.
    #[test]
    fn matches_the_formatting_it_replaced() {
        let all: Vec<u8> = (0..=255u8).collect();
        let expected: String = all.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(encode_lower(&all), expected);
    }
}
