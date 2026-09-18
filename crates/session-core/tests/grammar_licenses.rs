// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! The syntax-highlighting grammars must stay license-compatible.
//!
//! `lumis` ships its grammar set behind per-language features, and its
//! `all-languages` default includes GPL-3.0 ones. This project is AGPL-3.0;
//! statically linking a GPL-3.0 grammar would force the whole binary to
//! GPL-3.0, which it cannot be. The root `Cargo.toml` therefore enables
//! `all-languages` *minus* those grammars, by hand.
//!
//! By hand is the problem. The 0.9 → 0.13 bump added `lang-toon`
//! (tree-sitter-toon, GPL-3.0-or-later) to `all-languages`, and the documented
//! upgrade procedure — re-sync the list with `all-languages` — would have
//! pulled it straight in. The comment warning about that had been there since
//! the first exclusion and did not prevent the second one from being possible.
//!
//! So this test checks the outcome rather than the intent: it reads the
//! committed `Cargo.lock`, which is what the build actually resolves, and fails
//! if a known GPL grammar is in it at all — whether someone added the feature
//! deliberately, re-synced the list mechanically, or picked it up through a
//! transitive default.
//!
//! If this fails after a `lumis` upgrade, the fix is to keep the grammar out,
//! not to extend the allowlist: check the new grammar's license with
//! `curl -s https://crates.io/api/v1/crates/<crate> | jq '.versions[0].license'`
//! and, if it is GPL/AGPL, leave its `lang-*` feature disabled.

/// Grammar crates known to be GPL-licensed, with the license as published.
/// Adding one here does not make it allowed — it makes it *checked*.
const GPL_GRAMMARS: &[(&str, &str)] = &[
    ("tree-sitter-caddy", "GPL-3.0"),
    ("tree-sitter-toon", "GPL-3.0-or-later"),
];

#[test]
fn no_gpl_grammar_is_linked_into_this_agpl_binary() {
    let lock = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../Cargo.lock")
        .canonicalize()
        .expect("Cargo.lock is committed at the workspace root");
    let lock = std::fs::read_to_string(&lock).expect("reading Cargo.lock");

    let mut found = Vec::new();
    for (crate_name, license) in GPL_GRAMMARS {
        // Cargo.lock entries are `name = "<crate>"` on their own line.
        if lock
            .lines()
            .any(|l| l.trim() == format!("name = \"{crate_name}\""))
        {
            found.push(format!("{crate_name} ({license})"));
        }
    }

    assert!(
        found.is_empty(),
        "GPL grammar(s) resolved into the build: {}.\n\
         This project is AGPL-3.0 and cannot statically link GPL-3.0 code.\n\
         Disable the corresponding `lang-*` feature on `lumis` in the root \
         Cargo.toml — see the comment there.",
        found.join(", ")
    );
}

/// The exclusions are spelled out in the manifest, so a reader who is about to
/// re-sync the list sees *why* the list is not simply `all-languages`.
#[test]
fn the_manifest_still_explains_the_exclusions() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../Cargo.toml")
        .canonicalize()
        .expect("workspace manifest");
    let manifest = std::fs::read_to_string(&manifest).expect("reading Cargo.toml");

    for (crate_name, _) in GPL_GRAMMARS {
        assert!(
            manifest.contains(crate_name),
            "{crate_name} is checked for by this test but the root Cargo.toml no \
             longer says why it is excluded — the next person re-syncing the \
             grammar list will have no warning"
        );
    }
    for feature in ["\"lang-caddy\"", "\"lang-toon\""] {
        assert!(
            !manifest.contains(feature),
            "{feature} is enabled on lumis, which links a GPL grammar"
        );
    }
}
