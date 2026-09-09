// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 croit GmbH

//! Drift guard between the Fluent catalogs and the SPA's generated ones.
//!
//! `crates/session-core/locales/<lang>/*.ftl` is canonical for the whole
//! product: the server renders some strings itself (tool prompts, the OAuth
//! error page, proxy errors) and the SPA renders the rest, and both must name
//! a message identically or the two halves disagree in front of the user.
//!
//! The SPA cannot read `.ftl` at runtime — it is static files behind a service
//! worker — so `web/scripts/ftl-to-ts.py` converts them into TypeScript
//! catalogs at authoring time. A generated file that nobody regenerates is a
//! file that quietly goes stale, which is what this test exists to prevent:
//! add a key to a `.ftl` and forget `mise run gen-locales`, and CI says so
//! rather than a German reader finding a raw key in production.
//!
//! Cross-language consistency — every language defining the same keys — is
//! NOT checked here: `build.rs` already fails the build on it, which is
//! earlier and stricter than a test.

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    const LANGS: [&str; 6] = ["en", "de", "fr", "es", "ru", "zh"];

    fn repo_root() -> PathBuf {
        // `CARGO_MANIFEST_DIR` is crates/session-core.
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("resolve repo root")
    }

    /// Every message key a language's `.ftl` files define.
    ///
    /// Same shape the converter parses: `key = value` at column zero, plus
    /// selector blocks whose variants are indented (and so are not keys).
    fn ftl_keys(root: &Path, lang: &str) -> BTreeSet<String> {
        let dir = root.join("crates/session-core/locales").join(lang);
        let mut keys = BTreeSet::new();
        for entry in std::fs::read_dir(&dir).expect("read locale dir") {
            let path = entry.expect("dir entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("ftl") {
                continue;
            }
            let body = std::fs::read_to_string(&path).expect("read ftl");
            for line in body.lines() {
                // Indented lines are selector variants; `#` is a comment.
                if line.starts_with(char::is_whitespace) || line.starts_with('#') {
                    continue;
                }
                if let Some((key, _)) = line.split_once(" =") {
                    let key = key.trim();
                    if !key.is_empty() && !key.contains(' ') {
                        keys.insert(key.to_string());
                    }
                }
            }
        }
        keys
    }

    /// Every key the generated TypeScript catalog carries.
    ///
    /// Read as text rather than parsed as JS: the generator emits a JSON
    /// object body, and the keys sit at a known indent, so a regex is enough
    /// and this test needs no JavaScript toolchain to run.
    fn generated_keys(root: &Path, lang: &str) -> BTreeSet<String> {
        let path = root.join("web/src/lib/locales").join(format!("{lang}.ts"));
        let body = std::fs::read_to_string(&path).unwrap_or_else(|err| {
            panic!(
                "read {}: {err} — run `mise run gen-locales`",
                path.display()
            )
        });
        body.lines()
            .filter_map(|line| {
                // Top-level entries are indented by exactly one space.
                let rest = line.strip_prefix(' ')?;
                if rest.starts_with(' ') {
                    return None; // a nested variant
                }
                let rest = rest.strip_prefix('"')?;
                let (key, _) = rest.split_once("\":")?;
                Some(key.to_string())
            })
            .collect()
    }

    /// No Fluent syntax survives into a generated catalog.
    ///
    /// The key-set test above compares names, so a message whose *value* the
    /// converter failed to translate passes it happily and then renders its
    /// own source code to the user — which is how `scheduled-next-runs-prefix`
    /// reached the browser as `Next runs:{ " " }`, braces and quotes included.
    /// `{ $var }` becomes `{var}`, `{ "…" }` becomes its contents, and a
    /// selector becomes a variant object; anything left holding Fluent's
    /// `{ $`, `{ "` or `->` did not survive the conversion.
    #[test]
    fn no_unconverted_fluent_syntax_reaches_the_spa() {
        let root = repo_root();
        for lang in LANGS {
            let path = root.join("web/src/lib/locales").join(format!("{lang}.ts"));
            let body = std::fs::read_to_string(&path).expect("read generated catalog");
            for (number, line) in body.lines().enumerate() {
                // `{ $` and `{ "` cannot occur in a converted message; `->`
                // only ever came from a selector head.
                for needle in ["{ $", "{ \"", "->"] {
                    assert!(
                        !line.contains(needle),
                        "{lang}.ts:{} carries unconverted Fluent syntax {needle:?} — the \
                         converter did not understand this message and the user would see \
                         it verbatim:\n  {}",
                        number + 1,
                        line.trim()
                    );
                }
            }
        }
    }

    /// The generated catalogs are current with the `.ftl` sources.
    #[test]
    fn the_spa_catalogs_match_the_fluent_sources() {
        let root = repo_root();
        for lang in LANGS {
            let source = ftl_keys(&root, lang);
            let generated = generated_keys(&root, lang);
            let missing: Vec<_> = source.difference(&generated).take(8).collect();
            let extra: Vec<_> = generated.difference(&source).take(8).collect();
            assert!(
                missing.is_empty(),
                "{lang}: {} key(s) exist in the .ftl files but not in the generated \
                 catalog — run `mise run gen-locales`. First few: {missing:?}",
                source.difference(&generated).count()
            );
            assert!(
                extra.is_empty(),
                "{lang}: {} key(s) in the generated catalog no longer exist in the .ftl \
                 files — run `mise run gen-locales`. First few: {extra:?}",
                generated.difference(&source).count()
            );
        }
    }
}
