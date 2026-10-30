// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Reading and writing data classification on files and text.
//!
//! Pure functions over bytes and strings: they read digital markings, map
//! them to levels, find patterns and word-list terms, and mark generated
//! output. They take the scheme and the operator's mappings as arguments and
//! never touch the database, `AppState` or the tool registry. The formats
//! and limits are described in `docs/classification.md`.

mod filename;
mod markings;
mod patterns;
mod resolve;
mod stamp;
mod terms;
mod text;
mod xml;
mod zip_io;

#[cfg(test)]
mod tests;

pub use markings::{Marking, Markings, TSCP_CATEGORY_NAME, read_markings};
pub use patterns::{DE_TAX_ID, EMAIL, IBAN, PATTERN_IDS, PAYMENT_CARD, find_patterns};
pub use resolve::{Cause, Mapping, Resolved, resolve};
pub use stamp::{StampOptions, Stamped, stamp_output};
pub use terms::{MIN_TERM_CHARS, TermError, TermMatcher, TermUpload, parse_term_upload};
pub use text::{STAMP_WINDOW_BYTES, detection_text, stamp_matches};

#[derive(Debug, thiserror::Error)]
pub enum StampError {
    #[error(
        "the document could not be marked because its zip package could not be rewritten ({reason}); \
         the file was not changed, so generate it again or deliver it unmarked"
    )]
    Package { reason: String },
    #[error(
        "the document could not be marked because its `{part}` is unusable ({reason}); \
         the file was not changed, so generate it again or deliver it unmarked"
    )]
    Metadata { part: String, reason: String },
}
